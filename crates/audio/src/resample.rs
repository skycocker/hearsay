//! Convert arbitrary (rate, channel-count, f32) input into 16 kHz mono `f32`.
//!
//! The pipeline expects 16 kHz mono `f32` everywhere — that's what
//! whisper.cpp wants and it keeps downstream code free of "what format is
//! this" branches. This module owns the conversion from whatever the device
//! gives us (commonly 44.1/48 kHz stereo) to that target.
//!
//! Lives in its own module so we can unit-test it with synthetic input
//! without needing real audio hardware.

use rubato::{
    Resampler, SincFixedIn, SincInterpolationParameters, SincInterpolationType, WindowFunction,
};

use crate::{AudioError, TARGET_SAMPLE_RATE};

/// Pulled out as a constant so the audio-thread → worker hop and the
/// resampler's input size stay in sync.
pub(crate) const INPUT_CHUNK_FRAMES: usize = 1024;

pub(crate) struct Normalizer {
    source_channels: u16,
    /// `None` if the source is already at the target rate — we skip rubato
    /// entirely in that case (free perf win for USB mics that happen to
    /// support 16 kHz natively).
    resampler: Option<SincFixedIn<f32>>,
    /// Mono buffer accumulating samples until we have enough for the
    /// resampler's fixed input size.
    mono_buf: Vec<f32>,
}

impl Normalizer {
    pub(crate) fn new(source_rate: u32, source_channels: u16) -> Result<Self, AudioError> {
        let resampler = if source_rate == TARGET_SAMPLE_RATE {
            None
        } else {
            let params = SincInterpolationParameters {
                sinc_len: 128,
                f_cutoff: 0.95,
                interpolation: SincInterpolationType::Linear,
                oversampling_factor: 128,
                window: WindowFunction::BlackmanHarris2,
            };
            let r = SincFixedIn::<f32>::new(
                f64::from(TARGET_SAMPLE_RATE) / f64::from(source_rate),
                1.0,
                params,
                INPUT_CHUNK_FRAMES,
                1,
            )
            .map_err(|e| AudioError::Resampler(format!("{e:?}")))?;
            Some(r)
        };
        Ok(Self {
            source_channels: source_channels.max(1),
            resampler,
            mono_buf: Vec::with_capacity(INPUT_CHUNK_FRAMES * 4),
        })
    }

    /// Feed interleaved multi-channel samples. Output is appended to `out`.
    pub(crate) fn push(&mut self, interleaved: &[f32], out: &mut Vec<f32>) -> Result<(), AudioError> {
        // 1) Mix channels down to mono.
        let ch = usize::from(self.source_channels);
        if ch == 1 {
            self.mono_buf.extend_from_slice(interleaved);
        } else {
            let scale = 1.0_f32 / ch as f32;
            self.mono_buf.reserve(interleaved.len() / ch);
            for frame in interleaved.chunks_exact(ch) {
                let sum: f32 = frame.iter().copied().sum();
                self.mono_buf.push(sum * scale);
            }
        }

        // 2) If we're already at target rate, hand mono straight through.
        let Some(resampler) = self.resampler.as_mut() else {
            out.append(&mut self.mono_buf);
            return Ok(());
        };

        // 3) Otherwise feed the resampler in fixed-size chunks.
        while self.mono_buf.len() >= INPUT_CHUNK_FRAMES {
            let chunk: Vec<f32> = self.mono_buf.drain(..INPUT_CHUNK_FRAMES).collect();
            let resampled = resampler
                .process(&[chunk], None)
                .map_err(|e| AudioError::Resampler(format!("{e:?}")))?;
            out.extend(resampled[0].iter().copied());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine_at(rate: u32, secs: f32, hz: f32, channels: u16) -> Vec<f32> {
        let total_frames = (rate as f32 * secs) as usize;
        let ch = usize::from(channels);
        let mut out = Vec::with_capacity(total_frames * ch);
        for n in 0..total_frames {
            let v = (2.0 * std::f32::consts::PI * hz * n as f32 / rate as f32).sin() * 0.5;
            for _ in 0..ch {
                out.push(v);
            }
        }
        out
    }

    #[test]
    fn passthrough_when_rate_matches() {
        let mut n = Normalizer::new(TARGET_SAMPLE_RATE, 1).unwrap();
        let input = sine_at(TARGET_SAMPLE_RATE, 0.5, 440.0, 1);
        let mut out = Vec::new();
        n.push(&input, &mut out).unwrap();
        assert_eq!(out.len(), input.len(), "passthrough should not change length");
    }

    #[test]
    fn stereo_to_mono_averages_channels() {
        let mut n = Normalizer::new(TARGET_SAMPLE_RATE, 2).unwrap();
        // Interleaved L,R = (1.0, -1.0) → mono = 0.0
        let input = vec![1.0, -1.0, 0.5, 0.5];
        let mut out = Vec::new();
        n.push(&input, &mut out).unwrap();
        assert_eq!(out.len(), 2);
        assert!((out[0] - 0.0).abs() < 1e-6);
        assert!((out[1] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn downsamples_48k_to_16k() {
        // Feed 3 input chunks worth of 48 kHz audio → expect ~1/3 output samples.
        let mut n = Normalizer::new(48_000, 1).unwrap();
        let input = sine_at(48_000, 1.0, 440.0, 1); // 48,000 samples
        let mut out = Vec::new();
        n.push(&input, &mut out).unwrap();
        // Output should be close to 1/3 of input (16,000), within a chunk's worth
        // of slack because we only process full INPUT_CHUNK_FRAMES at a time.
        let expected = 16_000;
        let slack = (INPUT_CHUNK_FRAMES * 16_000 / 48_000) as i64;
        let actual = out.len() as i64;
        assert!(
            (actual - expected).abs() <= slack,
            "got {actual}, expected ~{expected} ± {slack}"
        );
    }

    #[test]
    fn handles_44100_to_16000() {
        // 44.1k → 16k is a non-integer ratio; this is the realistic USB-mic
        // case and the one most likely to expose rubato config bugs.
        let mut n = Normalizer::new(44_100, 2).unwrap();
        let input = sine_at(44_100, 1.0, 440.0, 2);
        let mut out = Vec::new();
        n.push(&input, &mut out).unwrap();
        // 44_100 stereo input frames → 44_100 mono frames → ~16_000 at 16 kHz
        let actual = out.len() as i64;
        let expected = 16_000_i64;
        let slack = (INPUT_CHUNK_FRAMES * 16_000 / 44_100) as i64 + 16;
        assert!(
            (actual - expected).abs() <= slack,
            "got {actual}, expected ~{expected} ± {slack}"
        );
    }
}
