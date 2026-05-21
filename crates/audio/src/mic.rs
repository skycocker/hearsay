//! Live microphone capture via cpal.
//!
//! cpal delivers samples on a high-priority OS audio thread. That thread
//! must never block, so the data callback only does cheap work — convert
//! the buffer to `f32` and shove it across a crossbeam channel to a worker.
//! The worker resamples to 16 kHz mono and emits [`AudioFrame`]s to the
//! consumer over a tokio mpsc, decoupling the audio thread from any async
//! runtime stalls in the consumer.

use std::sync::Arc;
use std::time::Instant;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Sample, SampleFormat, Stream};
use hearsay_core::AudioFrame;
use parking_lot::Mutex;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

use crate::resample::{INPUT_CHUNK_FRAMES, Normalizer};
use crate::{AudioError, TARGET_SAMPLE_RATE};

/// ~20 ms at 16 kHz. Picked so live transcript UI feels responsive without
/// stalling the worker on every callback.
const FRAME_SAMPLES: usize = 320;

/// Live handle on a running mic capture. Dropping it stops the stream.
pub struct MicCapture {
    _stream: Stream,
    _worker: std::thread::JoinHandle<()>,
}

pub fn start_mic(
    device_id: Option<&str>,
) -> Result<(MicCapture, UnboundedReceiver<AudioFrame>), AudioError> {
    let host = cpal::default_host();
    let device = match device_id {
        Some(id) => host
            .input_devices()?
            .find(|d| d.name().ok().as_deref() == Some(id))
            .ok_or_else(|| AudioError::DeviceNotFound(id.to_owned()))?,
        None => host.default_input_device().ok_or(AudioError::NoDevices)?,
    };

    let supported = device.default_input_config()?;
    let sample_rate = supported.sample_rate().0;
    let channels = supported.channels();
    let sample_format = supported.sample_format();
    let config: cpal::StreamConfig = supported.into();

    tracing::info!(
        device = %device.name().unwrap_or_default(),
        sample_rate,
        channels,
        ?sample_format,
        "opening input stream"
    );

    let (raw_tx, raw_rx) = crossbeam_channel::unbounded::<Vec<f32>>();
    let (frame_tx, frame_rx) = mpsc::unbounded_channel::<AudioFrame>();

    let err_state = Arc::new(Mutex::new(None::<cpal::StreamError>));
    let err_state_cb = Arc::clone(&err_state);
    let err_cb = move |e: cpal::StreamError| {
        tracing::error!(?e, "cpal stream error");
        *err_state_cb.lock() = Some(e);
    };

    let stream = match sample_format {
        SampleFormat::F32 => device.build_input_stream(
            &config,
            move |data: &[f32], _| {
                let _ = raw_tx.send(data.to_vec());
            },
            err_cb,
            None,
        )?,
        SampleFormat::I16 => device.build_input_stream(
            &config,
            move |data: &[i16], _| {
                let v: Vec<f32> = data.iter().map(|&s| s.to_sample::<f32>()).collect();
                let _ = raw_tx.send(v);
            },
            err_cb,
            None,
        )?,
        SampleFormat::U16 => device.build_input_stream(
            &config,
            move |data: &[u16], _| {
                let v: Vec<f32> = data.iter().map(|&s| s.to_sample::<f32>()).collect();
                let _ = raw_tx.send(v);
            },
            err_cb,
            None,
        )?,
        other => {
            tracing::error!(?other, "unsupported sample format");
            return Err(AudioError::NoSupportedConfig);
        }
    };

    stream.play()?;

    let worker = std::thread::Builder::new()
        .name("hearsay-mic-worker".into())
        .spawn(move || worker_loop(raw_rx, frame_tx, sample_rate, channels))
        .expect("spawn mic worker");

    Ok((
        MicCapture {
            _stream: stream,
            _worker: worker,
        },
        frame_rx,
    ))
}

fn worker_loop(
    raw_rx: crossbeam_channel::Receiver<Vec<f32>>,
    frame_tx: UnboundedSender<AudioFrame>,
    source_rate: u32,
    channels: u16,
) {
    let mut normalizer = match Normalizer::new(source_rate, channels) {
        Ok(n) => n,
        Err(e) => {
            tracing::error!(?e, "failed to build resampler; worker exiting");
            return;
        }
    };

    let mut out_buf: Vec<f32> = Vec::with_capacity(INPUT_CHUNK_FRAMES * 2);
    let mut frame_pcm: Vec<f32> = Vec::with_capacity(FRAME_SAMPLES);

    while let Ok(raw) = raw_rx.recv() {
        if let Err(e) = normalizer.push(&raw, &mut out_buf) {
            tracing::error!(?e, "resampler failed; worker exiting");
            return;
        }

        // Drain `out_buf` into fixed-size AudioFrames.
        let captured_at = Instant::now();
        let mut idx = 0;
        while idx + FRAME_SAMPLES <= out_buf.len() {
            frame_pcm.clear();
            frame_pcm.extend_from_slice(&out_buf[idx..idx + FRAME_SAMPLES]);
            let frame = AudioFrame {
                pcm: std::mem::replace(&mut frame_pcm, Vec::with_capacity(FRAME_SAMPLES)),
                sample_rate: TARGET_SAMPLE_RATE,
                channels: 1,
                captured_at,
            };
            if frame_tx.send(frame).is_err() {
                // Consumer dropped — clean shutdown.
                return;
            }
            idx += FRAME_SAMPLES;
        }
        out_buf.drain(..idx);
    }
}
