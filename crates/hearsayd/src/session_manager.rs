//! Tracks live recording sessions in memory and keeps their associated cpal
//! stream + WAV writer + (optional) transcription worker alive for the
//! duration of the recording.
//!
//! The HTTP layer goes through this manager — it never touches cpal or the
//! audio writer directly. That lets us swap sources (system audio, Meet
//! sidecar, synthetic test source) without touching routes.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::Utc;
use hearsay_audio::{MicCapture, start_mic};
use hearsay_core::{Segment, SessionId, SessionMeta, SessionStatus, SourceKind, Summary};
use hearsay_storage::{AudioWriter, Storage, WavAudioWriter};
use hearsay_summarize::Summarizer;
use hearsay_transcribe::{TranscribedSegment, TranscriberConfig, TranscriptionWorker};
use parking_lot::Mutex;
use tokio::sync::broadcast;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::task::JoinHandle;

use crate::config::Config;
use crate::error::ApiError;

const LIVE_BROADCAST_CAPACITY: usize = 256;

/// Per-source-kind parameters needed at session start.
#[derive(Debug, Clone)]
pub enum StartParams {
    Mic { device_id: Option<String> },
}

struct Active {
    /// Dropping this stops the cpal stream and joins the worker thread.
    capture: MicCapture,
    /// Drains audio frames, writes WAV, feeds the transcriber.
    consumer: JoinHandle<()>,
    /// Pumps transcribed segments into storage + the live broadcast.
    /// `None` when no model is available — session still records audio,
    /// transcript is just empty.
    pumper: Option<JoinHandle<()>>,
    /// Producer side for the live-transcript WS. Cloned per subscriber.
    live_tx: broadcast::Sender<Segment>,
}

pub struct SessionManager {
    storage: Storage,
    config: Arc<Config>,
    summarizer: Option<Arc<Summarizer>>,
    active: Mutex<HashMap<SessionId, Active>>,
}

impl SessionManager {
    pub fn new(
        storage: Storage,
        config: Arc<Config>,
        summarizer: Option<Arc<Summarizer>>,
    ) -> Self {
        Self {
            storage,
            config,
            summarizer,
            active: Mutex::new(HashMap::new()),
        }
    }

    pub fn start(
        &self,
        name: String,
        language: Option<String>,
        params: StartParams,
    ) -> Result<SessionMeta, ApiError> {
        match params {
            StartParams::Mic { device_id } => self.start_mic(name, language, device_id),
        }
    }

    fn start_mic(
        &self,
        name: String,
        language: Option<String>,
        device_id: Option<String>,
    ) -> Result<SessionMeta, ApiError> {
        let id = SessionId::new();
        let data_dir = self.config.resolved_data_dir();
        let dir = data_dir.join("sessions").join(id.to_string());
        std::fs::create_dir_all(&dir).map_err(|e| ApiError::Internal(e.to_string()))?;
        let audio_path = dir.join("audio.wav");

        let meta = SessionMeta {
            id,
            name,
            source_kind: SourceKind::Mic,
            source_meta: serde_json::json!({ "device_id": device_id }),
            language: language.clone(),
            audio_path: audio_path.clone(),
            started_at: Utc::now(),
            ended_at: None,
            status: SessionStatus::Active,
        };
        self.storage.insert_session(&meta)?;

        let (capture, mut frame_rx) = start_mic(device_id.as_deref())?;
        let writer = WavAudioWriter::create(&audio_path)?;
        let mut writer: Box<dyn AudioWriter> = Box::new(writer);

        // Best-effort transcription start. If the model isn't downloaded,
        // we still record audio and warn — the user can transcribe later
        // (once the post-hoc transcribe endpoint lands).
        let (transcribe_worker, transcribe_rx): (Option<TranscriptionWorker>, Option<UnboundedReceiver<TranscribedSegment>>) = {
            let model_path = self.config.transcription.resolved_model_path(&data_dir);
            let lang = match language.as_deref() {
                Some("auto") | None => None,
                Some(other) => Some(other.to_owned()),
            };
            let cfg = TranscriberConfig {
                model_path: model_path.clone(),
                language: lang,
                n_threads: self.config.transcription.n_threads,
            };
            match TranscriptionWorker::start(cfg) {
                Ok((w, rx)) => (Some(w), Some(rx)),
                Err(e) => {
                    tracing::warn!(?e, %id, model_path = %model_path.display(),
                        "transcription disabled for this session; audio still recording");
                    (None, None)
                }
            }
        };

        let (live_tx, _initial_rx) = broadcast::channel(LIVE_BROADCAST_CAPACITY);

        // Consumer task: drains audio frames, writes WAV, feeds transcribe.
        // Owns transcribe_worker — when this task ends, dropping the worker
        // closes its audio channel, triggering a flush.
        let storage_consumer = self.storage.clone();
        let consumer = tokio::spawn(async move {
            let worker = transcribe_worker;
            while let Some(frame) = frame_rx.recv().await {
                if let Err(e) = writer.write_pcm(&frame.pcm) {
                    tracing::error!(?e, %id, "WAV writer failed; ending session");
                    let _ = storage_consumer.finish_session(id, SessionStatus::Failed, Utc::now());
                    return;
                }
                if let Some(w) = &worker {
                    // Feeding a closed transcriber returns Err — drop the
                    // reference so we stop trying.
                    if w.feed(frame.pcm).is_err() {
                        tracing::warn!(%id, "transcribe worker closed early");
                    }
                }
            }
            if let Err(e) = writer.finalize() {
                tracing::error!(?e, %id, "WAV finalize failed");
            }
            // Dropping `worker` here lets it flush its tail chunk.
            drop(worker);
        });

        // Pumper task: forwards transcribed segments → storage + live WS.
        // Spawned only when transcription is active.
        let pumper = transcribe_rx.map(|mut rx| {
            let storage_pump = self.storage.clone();
            let live = live_tx.clone();
            tokio::spawn(async move {
                let mut seq: u32 = 0;
                while let Some(seg) = rx.recv().await {
                    let stored = Segment {
                        session_id: id,
                        seq,
                        start_ms: seg.start_ms,
                        end_ms: seg.end_ms,
                        text: seg.text,
                        language: seg.language,
                        confidence: None,
                        speaker_id: None,
                    };
                    if let Err(e) = storage_pump.insert_segment(&stored) {
                        tracing::error!(?e, %id, "failed to persist segment");
                    }
                    // Broadcast errors are normal (no live viewers) — ignore.
                    let _ = live.send(stored);
                    seq += 1;
                }
            })
        });

        self.active.lock().insert(
            id,
            Active {
                capture,
                consumer,
                pumper,
                live_tx,
            },
        );

        Ok(meta)
    }

    /// Subscribe to the live transcript stream for an active session.
    pub fn subscribe_live(&self, id: SessionId) -> Option<broadcast::Receiver<Segment>> {
        self.active.lock().get(&id).map(|a| a.live_tx.subscribe())
    }

    pub async fn stop(&self, id: SessionId) -> Result<SessionMeta, ApiError> {
        let active = self
            .active
            .lock()
            .remove(&id)
            .ok_or_else(|| ApiError::NotFound(format!("active session {id}")))?;
        // Drop capture → cpal stops → frame_rx returns None → consumer task
        // ends → drops transcribe_worker → transcriber flushes tail →
        // segment_rx closes → pumper exits.
        drop(active.capture);
        let _ = active.consumer.await;
        if let Some(pumper) = active.pumper {
            let _ = pumper.await;
        }

        self.storage.finish_session(id, SessionStatus::Completed, Utc::now())?;
        let meta = self
            .storage
            .get_session(id)?
            .ok_or_else(|| ApiError::NotFound(format!("session {id}")))?;

        // Fire-and-forget auto-summarize. The session is already marked
        // Completed; the summary appears in storage when it's ready.
        if let Some(summarizer) = self.summarizer.clone() {
            let storage = self.storage.clone();
            let language = meta.language.clone();
            let model_label = self.config.summarization.model.clone();
            tokio::spawn(async move {
                run_summarization(storage, summarizer, id, language, model_label).await;
            });
        }

        Ok(meta)
    }

    /// Synchronously trigger summarization for a completed session. Used
    /// by `POST /api/sessions/:id/summarize` so the user can re-summarize
    /// or pick a different model later without restarting recording.
    pub async fn resummarize(&self, id: SessionId) -> Result<Summary, ApiError> {
        let Some(summarizer) = self.summarizer.clone() else {
            return Err(ApiError::BadRequest(
                "summarization model not loaded — set summarization.model_path and restart".into(),
            ));
        };
        let meta = self
            .storage
            .get_session(id)?
            .ok_or_else(|| ApiError::NotFound(format!("session {id}")))?;
        let segments = self.storage.list_segments(id)?;
        let language = meta.language;
        let storage = self.storage.clone();

        let result = tokio::task::spawn_blocking(move || {
            summarizer.summarize(&segments, language.as_deref())
        })
        .await
        .map_err(|e| ApiError::Internal(format!("summarize join: {e}")))?;

        let content = result.map_err(|e| match e {
            hearsay_summarize::SummarizeError::EmptyTranscript => {
                ApiError::BadRequest("no transcript yet".into())
            }
            other => ApiError::Internal(format!("summarize: {other}")),
        })?;

        let summary = Summary {
            session_id: id,
            model: self.config.summarization.model.clone(),
            content,
            generated_at: Utc::now(),
        };
        storage.upsert_summary(&summary)?;
        Ok(summary)
    }

    pub fn is_active(&self, id: SessionId) -> bool {
        self.active.lock().contains_key(&id)
    }

    pub fn active_ids(&self) -> Vec<SessionId> {
        self.active.lock().keys().copied().collect()
    }
}

async fn run_summarization(
    storage: Storage,
    summarizer: Arc<Summarizer>,
    id: SessionId,
    language: Option<String>,
    model_label: String,
) {
    let segments = match storage.list_segments(id) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(?e, %id, "list_segments failed during summarization");
            return;
        }
    };
    if segments.is_empty() {
        tracing::info!(%id, "no segments to summarize; skipping");
        return;
    }

    let language_for_call = language.clone();
    let result = tokio::task::spawn_blocking(move || {
        summarizer.summarize(&segments, language_for_call.as_deref())
    })
    .await;

    let content = match result {
        Ok(Ok(c)) => c,
        Ok(Err(e)) => {
            tracing::warn!(?e, %id, "summarization failed");
            return;
        }
        Err(e) => {
            tracing::error!(?e, %id, "summarize task panicked");
            return;
        }
    };

    let summary = Summary {
        session_id: id,
        model: model_label,
        content,
        generated_at: Utc::now(),
    };
    if let Err(e) = storage.upsert_summary(&summary) {
        tracing::error!(?e, %id, "failed to store summary");
    } else {
        tracing::info!(%id, "summary stored");
    }
}
