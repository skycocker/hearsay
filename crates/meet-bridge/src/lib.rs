//! Rust side of the Meet sidecar bridge.
//!
//! Supervises the C++ `hearsay-meet` child process (libwebrtc + Meet Media
//! API), speaks a length-prefixed framed protocol over a unix socket, and
//! exposes the incoming PCM frames as an [`AudioFrame`] stream that the rest
//! of the pipeline consumes uniformly.
//!
//! Implementation lands in task #10.

use hearsay_core::AudioFrame;

#[derive(Debug, thiserror::Error)]
pub enum MeetBridgeError {
    #[error("sidecar binary not found at `{0}`")]
    SidecarMissing(String),
    #[error("sidecar exited unexpectedly: {0}")]
    SidecarExited(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("not yet implemented")]
    Unimplemented,
}

/// Frame produced by the sidecar after decoding from libwebrtc.
pub type MeetFrame = AudioFrame;
