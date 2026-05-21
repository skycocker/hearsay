//! Audio capture: enumerate input devices and produce a normalized
//! 16 kHz mono `f32` stream of [`AudioFrame`]s.
//!
//! Sources (mic via cpal, system audio via ScreenCaptureKit on macOS, Meet
//! audio via the meet-bridge sidecar) all conform to the same trait so that
//! the rest of the pipeline doesn't care where bytes came from.

use hearsay_core::AudioFrame;

#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    #[error("not yet implemented")]
    Unimplemented,
}

/// Implementation lands in task #4.
pub fn _placeholder_frame() -> AudioFrame {
    AudioFrame {
        pcm: Vec::new(),
        sample_rate: 16_000,
        channels: 1,
        captured_at: std::time::Instant::now(),
    }
}
