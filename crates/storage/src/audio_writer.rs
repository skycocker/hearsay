//! Streaming PCM-to-disk writers.
//!
//! Sources hand us 16 kHz mono `f32` frames; an [`AudioWriter`] writes them
//! straight to disk without buffering whole sessions in RAM. Currently only
//! WAV is implemented; Opus support is task #15.

use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use hound::{SampleFormat, WavSpec, WavWriter};

use crate::StorageError;

/// Target sample rate for everything in hearsay.
pub const TARGET_SAMPLE_RATE: u32 = 16_000;

pub trait AudioWriter: Send {
    /// Append PCM samples (16 kHz mono `f32` in `[-1.0, 1.0]`).
    fn write_pcm(&mut self, samples: &[f32]) -> Result<(), StorageError>;

    /// Total number of samples written so far. Useful for callers that need
    /// to compute the session's current duration without polling the OS.
    fn samples_written(&self) -> u64;

    /// Flush and close the file. Consuming the writer (rather than taking
    /// `&mut self`) makes it impossible to keep writing after finalization.
    fn finalize(self: Box<Self>) -> Result<(), StorageError>;
}

pub struct WavAudioWriter {
    writer: Option<WavWriter<BufWriter<File>>>,
    path: PathBuf,
    samples_written: u64,
}

impl WavAudioWriter {
    pub fn create<P: AsRef<Path>>(path: P) -> Result<Self, StorageError> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let spec = WavSpec {
            channels: 1,
            sample_rate: TARGET_SAMPLE_RATE,
            // 16-bit PCM produces ~half the bytes of 32-bit float for the same
            // perceptual quality on speech, and decodes faster everywhere.
            bits_per_sample: 16,
            sample_format: SampleFormat::Int,
        };
        let writer = WavWriter::create(&path, spec)?;
        Ok(Self {
            writer: Some(writer),
            path,
            samples_written: 0,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl AudioWriter for WavAudioWriter {
    fn write_pcm(&mut self, samples: &[f32]) -> Result<(), StorageError> {
        let writer = self.writer.as_mut().ok_or(StorageError::AlreadyFinalized)?;
        for &s in samples {
            // Clamp before scaling — values slightly outside `[-1, 1]` can
            // come from resamplers and would wrap around if we just cast.
            let clamped = s.clamp(-1.0, 1.0);
            let pcm = (clamped * f32::from(i16::MAX)) as i16;
            writer.write_sample(pcm)?;
        }
        self.samples_written += samples.len() as u64;
        Ok(())
    }

    fn samples_written(&self) -> u64 {
        self.samples_written
    }

    fn finalize(mut self: Box<Self>) -> Result<(), StorageError> {
        if let Some(w) = self.writer.take() {
            w.finalize()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn roundtrip_sine_wave() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("sine.wav");

        // 1 second of 440 Hz sine wave at 16 kHz.
        let samples: Vec<f32> = (0..TARGET_SAMPLE_RATE)
            .map(|n| (2.0 * std::f32::consts::PI * 440.0 * n as f32 / TARGET_SAMPLE_RATE as f32).sin() * 0.5)
            .collect();

        let mut w: Box<dyn AudioWriter> = Box::new(WavAudioWriter::create(&path).unwrap());
        w.write_pcm(&samples).unwrap();
        assert_eq!(w.samples_written(), u64::from(TARGET_SAMPLE_RATE));
        w.finalize().unwrap();

        // Read it back with hound and check we got the same count back at the
        // right rate. We don't compare per-sample because i16 quantization
        // makes that lossy, but RMS should be close.
        let reader = hound::WavReader::open(&path).unwrap();
        let spec = reader.spec();
        assert_eq!(spec.sample_rate, TARGET_SAMPLE_RATE);
        assert_eq!(spec.channels, 1);
        assert_eq!(spec.bits_per_sample, 16);
        assert_eq!(reader.duration(), TARGET_SAMPLE_RATE);
    }

    #[test]
    fn clamps_out_of_range_samples() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("clamp.wav");

        let samples = vec![5.0_f32, -5.0, 0.5, -0.5];
        let mut w: Box<dyn AudioWriter> = Box::new(WavAudioWriter::create(&path).unwrap());
        w.write_pcm(&samples).unwrap();
        w.finalize().unwrap();

        let mut reader = hound::WavReader::open(&path).unwrap();
        let read: Vec<i16> = reader.samples::<i16>().map(Result::unwrap).collect();
        assert_eq!(read[0], i16::MAX); // 5.0 → clamped to 1.0 → MAX
        assert_eq!(read[1], -i16::MAX); // -5.0 → clamped to -1.0 → -MAX
    }
}
