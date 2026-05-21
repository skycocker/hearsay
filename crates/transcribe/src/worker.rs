//! The `TranscriptionWorker` ties Chunker + Transcriber together and runs
//! them on a dedicated OS thread.
//!
//! Daemon-side wiring:
//! - SessionManager spawns one worker per active session, hands it the
//!   model config, and gets back a sender (for audio frames) and a
//!   receiver (for transcribed segments).
//! - When the session stops, dropping the sender causes the worker to
//!   flush its tail chunk and exit cleanly.

use std::thread::JoinHandle;

use crossbeam_channel::{Receiver as XRecv, Sender as XSend};
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

use crate::chunker::Chunker;
use crate::model::{TranscribedSegment, Transcriber, TranscriberConfig};
use crate::{TranscribeError, TranscribeRequest};

#[derive(Debug, thiserror::Error)]
pub enum FeedError {
    #[error("transcription worker has shut down")]
    Closed,
}

/// Audio chunk seconds. 20s feels responsive without making whisper
/// re-warm its KV cache too often. Configurable later if needed.
const CHUNK_SECONDS: u32 = 20;
const SAMPLE_RATE: u32 = 16_000;

pub struct TranscriptionWorker {
    audio_tx: Option<XSend<Vec<f32>>>,
    handle: Option<JoinHandle<()>>,
}

impl TranscriptionWorker {
    /// Build the worker. This loads the model on the calling thread (which
    /// is what we want — failure to load is a startup error, not a runtime
    /// one).
    pub fn start(
        config: TranscriberConfig,
    ) -> Result<(Self, UnboundedReceiver<TranscribedSegment>), TranscribeError> {
        let transcriber = Transcriber::new(config)?;
        let (audio_tx, audio_rx) = crossbeam_channel::unbounded::<Vec<f32>>();
        let (segment_tx, segment_rx) = mpsc::unbounded_channel::<TranscribedSegment>();

        let handle = std::thread::Builder::new()
            .name("hearsay-transcribe".into())
            .spawn(move || run(transcriber, audio_rx, segment_tx))
            .expect("spawn transcribe worker");

        Ok((
            Self {
                audio_tx: Some(audio_tx),
                handle: Some(handle),
            },
            segment_rx,
        ))
    }

    /// Push a batch of PCM samples (16 kHz mono `f32`). Returns
    /// [`FeedError::Closed`] if the worker has already shut down.
    pub fn feed(&self, pcm: Vec<f32>) -> Result<(), FeedError> {
        match &self.audio_tx {
            Some(tx) => tx.send(pcm).map_err(|_| FeedError::Closed),
            None => Err(FeedError::Closed),
        }
    }
}

impl Drop for TranscriptionWorker {
    fn drop(&mut self) {
        // Closing the audio channel signals the worker to flush and exit.
        self.audio_tx.take();
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

fn run(
    transcriber: Transcriber,
    audio_rx: XRecv<Vec<f32>>,
    segment_tx: UnboundedSender<TranscribedSegment>,
) {
    let mut chunker = Chunker::new(SAMPLE_RATE, CHUNK_SECONDS);

    while let Ok(pcm) = audio_rx.recv() {
        for req in chunker.push(&pcm) {
            if !decode_and_emit(&transcriber, req, &segment_tx) {
                return;
            }
        }
    }

    // Channel closed → flush whatever remains in the buffer. Even small
    // tail chunks (< 1s) give whisper something to say.
    if let Some(req) = chunker.flush() {
        let _ = decode_and_emit(&transcriber, req, &segment_tx);
    }
}

/// Returns `false` when the segment receiver has been dropped — caller
/// should bail.
fn decode_and_emit(
    transcriber: &Transcriber,
    req: TranscribeRequest,
    segment_tx: &UnboundedSender<TranscribedSegment>,
) -> bool {
    let TranscribeRequest { pcm, offset_ms } = req;
    let segments = match transcriber.transcribe(&pcm, offset_ms) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(?e, offset_ms, "transcribe failed; dropping chunk");
            return true;
        }
    };
    for seg in segments {
        if segment_tx.send(seg).is_err() {
            return false;
        }
    }
    true
}
