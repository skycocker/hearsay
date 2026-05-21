//! Tracks live recording sessions in memory and keeps their associated cpal
//! stream + WAV writer alive for the duration of the recording.
//!
//! The HTTP layer goes through this manager — it never touches cpal or the
//! audio writer directly. That lets us swap sources (system audio, Meet
//! sidecar, synthetic test source) without touching routes.

use std::collections::HashMap;
use std::path::PathBuf;

use chrono::Utc;
use hearsay_audio::{MicCapture, start_mic};
use hearsay_core::{Segment, SessionId, SessionMeta, SessionStatus, SourceKind};
use hearsay_storage::{AudioWriter, Storage, WavAudioWriter};
use parking_lot::Mutex;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;

use crate::error::ApiError;

/// Capacity of the per-session live transcript broadcast. Big enough that
/// a slow WS client doesn't drop segments under normal speaking pace, small
/// enough that a totally-stalled subscriber doesn't pin the whole session's
/// transcript in RAM.
const LIVE_BROADCAST_CAPACITY: usize = 256;

/// Per-source-kind parameters needed at session start.
#[derive(Debug, Clone)]
pub enum StartParams {
    Mic { device_id: Option<String> },
}

struct Active {
    /// Dropping this stops the cpal stream and joins the worker thread.
    capture: MicCapture,
    consumer: JoinHandle<()>,
    /// Producer side for the live-transcript WS. Dropping this when the
    /// session ends gives all live subscribers a clean disconnect signal.
    live_tx: broadcast::Sender<Segment>,
}

pub struct SessionManager {
    storage: Storage,
    data_dir: PathBuf,
    active: Mutex<HashMap<SessionId, Active>>,
}

impl SessionManager {
    pub fn new(storage: Storage, data_dir: PathBuf) -> Self {
        Self {
            storage,
            data_dir,
            active: Mutex::new(HashMap::new()),
        }
    }

    /// Begin a new session. Persists [`SessionMeta`] in storage, opens the
    /// audio source, and spawns a task that drains frames into the WAV
    /// writer. Returns the metadata so the HTTP layer can hand it back.
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
        let dir = self.data_dir.join("sessions").join(id.to_string());
        std::fs::create_dir_all(&dir).map_err(|e| ApiError::Internal(e.to_string()))?;
        let audio_path = dir.join("audio.wav");

        let meta = SessionMeta {
            id,
            name,
            source_kind: SourceKind::Mic,
            source_meta: serde_json::json!({ "device_id": device_id }),
            language,
            audio_path: audio_path.clone(),
            started_at: Utc::now(),
            ended_at: None,
            status: SessionStatus::Active,
        };
        self.storage.insert_session(&meta)?;

        let (capture, mut frame_rx) = start_mic(device_id.as_deref())?;
        let writer = WavAudioWriter::create(&audio_path)?;
        let mut writer: Box<dyn AudioWriter> = Box::new(writer);

        let storage = self.storage.clone();
        let consumer = tokio::spawn(async move {
            while let Some(frame) = frame_rx.recv().await {
                if let Err(e) = writer.write_pcm(&frame.pcm) {
                    tracing::error!(?e, %id, "wav writer failed; ending session");
                    let _ = storage.finish_session(id, SessionStatus::Failed, Utc::now());
                    return;
                }
                // TODO(task #6): forward frame to transcription queue here.
            }
            // Channel closed (stream stopped). Finalize the WAV.
            if let Err(e) = writer.finalize() {
                tracing::error!(?e, %id, "finalize failed");
            }
        });

        let (live_tx, _live_rx_drop) = broadcast::channel(LIVE_BROADCAST_CAPACITY);
        self.active
            .lock()
            .insert(id, Active { capture, consumer, live_tx });

        Ok(meta)
    }

    /// Subscribe to the live transcript stream for an active session.
    /// Returns `None` if the session isn't active.
    pub fn subscribe_live(&self, id: SessionId) -> Option<broadcast::Receiver<Segment>> {
        self.active.lock().get(&id).map(|a| a.live_tx.subscribe())
    }

    /// Publish a transcript segment to the live broadcast. Returns the
    /// number of subscribers reached (0 is normal — nobody might be
    /// watching). Called by the transcription pipeline (task #6).
    #[allow(dead_code)]
    pub fn publish_segment(&self, id: SessionId, seg: Segment) -> usize {
        self.active
            .lock()
            .get(&id)
            .map(|a| a.live_tx.send(seg).unwrap_or(0))
            .unwrap_or(0)
    }

    /// Stop a session: drop the capture (closes cpal stream), let the
    /// consumer task finalize the WAV, mark the session Completed.
    pub async fn stop(&self, id: SessionId) -> Result<SessionMeta, ApiError> {
        let active = self
            .active
            .lock()
            .remove(&id)
            .ok_or_else(|| ApiError::NotFound(format!("active session {id}")))?;
        // Drop capture → closes cpal stream → frame_rx returns None → consumer
        // task ends. We just need to await it so the WAV is fully flushed
        // before we return.
        drop(active.capture);
        let _ = active.consumer.await;

        self.storage.finish_session(id, SessionStatus::Completed, Utc::now())?;
        let meta = self
            .storage
            .get_session(id)?
            .ok_or_else(|| ApiError::NotFound(format!("session {id}")))?;
        Ok(meta)
    }

    pub fn is_active(&self, id: SessionId) -> bool {
        self.active.lock().contains_key(&id)
    }

    pub fn active_ids(&self) -> Vec<SessionId> {
        self.active.lock().keys().copied().collect()
    }
}

