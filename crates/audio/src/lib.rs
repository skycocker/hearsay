//! Audio capture: enumerate input devices and produce a normalized
//! 16 kHz mono `f32` stream of [`AudioFrame`]s.
//!
//! Sources (mic via cpal, ScreenCaptureKit on macOS, the Meet sidecar) all
//! converge on a single [`AudioFrame`] contract so the rest of the pipeline
//! doesn't care where bytes came from.

mod device;
mod mic;
mod resample;

pub use device::{InputDevice, list_input_devices};
pub use mic::{MicCapture, start_mic};

/// Target sample rate for everything downstream of the audio crate. Mirrors
/// `hearsay_storage::TARGET_SAMPLE_RATE` — both crates have to agree, and
/// duplicating the constant avoids a non-obvious dependency edge.
pub const TARGET_SAMPLE_RATE: u32 = 16_000;

#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    #[error("no input devices available")]
    NoDevices,
    #[error("input device `{0}` not found")]
    DeviceNotFound(String),
    #[error("device has no supported input config")]
    NoSupportedConfig,
    #[error("cpal: build stream: {0}")]
    BuildStream(#[from] cpal::BuildStreamError),
    #[error("cpal: play stream: {0}")]
    PlayStream(#[from] cpal::PlayStreamError),
    #[error("cpal: devices: {0}")]
    Devices(#[from] cpal::DevicesError),
    #[error("cpal: default config: {0}")]
    DefaultConfig(#[from] cpal::DefaultStreamConfigError),
    #[error("cpal: device name: {0}")]
    DeviceName(#[from] cpal::DeviceNameError),
    #[error("rubato: {0}")]
    Resampler(String),
}
