use std::time::{Duration, Instant};

/// A chunk of PCM audio handed off between pipeline stages.
///
/// hearsay normalizes everything internally to 16 kHz mono `f32` (the format
/// whisper.cpp wants), so sources are responsible for resampling before they
/// emit frames. Frames are deliberately small (a few tens of ms) so that
/// downstream consumers can react quickly without buffering large amounts of
/// audio in RAM.
#[derive(Clone, Debug)]
pub struct AudioFrame {
    pub pcm: Vec<f32>,
    pub sample_rate: u32,
    pub channels: u16,
    /// Wall-clock instant the first sample of this frame was captured.
    pub captured_at: Instant,
}

impl AudioFrame {
    pub fn duration(&self) -> Duration {
        let samples_per_channel = self.pcm.len() as u64 / u64::from(self.channels.max(1));
        Duration::from_nanos(samples_per_channel * 1_000_000_000 / u64::from(self.sample_rate))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duration_for_1s_of_mono_16k() {
        let f = AudioFrame {
            pcm: vec![0.0; 16_000],
            sample_rate: 16_000,
            channels: 1,
            captured_at: Instant::now(),
        };
        assert_eq!(f.duration(), Duration::from_secs(1));
    }

    #[test]
    fn duration_for_500ms_of_stereo_48k() {
        let f = AudioFrame {
            pcm: vec![0.0; 48_000],
            sample_rate: 48_000,
            channels: 2,
            captured_at: Instant::now(),
        };
        assert_eq!(f.duration(), Duration::from_millis(500));
    }
}
