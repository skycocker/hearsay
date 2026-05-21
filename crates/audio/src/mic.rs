//! Live microphone capture via cpal.
//!
//! `cpal::Stream` is `!Send` on macOS (it holds Core Audio handles with
//! thread affinity). To keep `MicCapture` `Send + Sync` so it can live in
//! the daemon's session map behind a mutex, we confine the Stream to a
//! dedicated worker thread that also runs the resampler. The HTTP layer
//! only ever sees the `MicCapture` handle, which is just channel ends.
//!
//! Data path:
//!   cpal callback (audio thread)
//!     → crossbeam unbounded → worker thread
//!       (worker also owns the Stream; drop = stop)
//!     → rubato resample → tokio mpsc → consumer

use std::time::Instant;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, Sample, SampleFormat, Stream, StreamConfig};
use crossbeam_channel::{Receiver as XRecv, Sender as XSend};
use hearsay_core::AudioFrame;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

use crate::resample::{INPUT_CHUNK_FRAMES, Normalizer};
use crate::{AudioError, TARGET_SAMPLE_RATE};

/// ~20 ms at 16 kHz.
const FRAME_SAMPLES: usize = 320;

/// Handle on a running mic capture. Dropping it stops the stream and joins
/// the worker thread. Holds only `Send + Sync` types so callers can park it
/// behind a `Mutex<HashMap<_, _>>` without contortions.
pub struct MicCapture {
    stop_tx: Option<XSend<()>>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl Drop for MicCapture {
    fn drop(&mut self) {
        if let Some(tx) = self.stop_tx.take() {
            let _ = tx.send(());
        }
        if let Some(w) = self.worker.take() {
            let _ = w.join();
        }
    }
}

pub fn start_mic(
    device_id: Option<&str>,
) -> Result<(MicCapture, UnboundedReceiver<AudioFrame>), AudioError> {
    // Resolve the device + config on the calling thread (these types are
    // Send), then move them into the worker which builds the Stream.
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
    let config: StreamConfig = supported.into();

    tracing::info!(
        device = %device.name().unwrap_or_default(),
        sample_rate,
        channels,
        ?sample_format,
        "opening input stream"
    );

    let (raw_tx, raw_rx) = crossbeam_channel::unbounded::<Vec<f32>>();
    let (frame_tx, frame_rx) = mpsc::unbounded_channel::<AudioFrame>();
    let (stop_tx, stop_rx) = crossbeam_channel::bounded::<()>(1);

    let worker = std::thread::Builder::new()
        .name("hearsay-mic".into())
        .spawn(move || {
            // The Stream gets built and dropped on THIS thread — the entire
            // !Send lifetime stays here.
            let stream = match build_stream(&device, &config, sample_format, raw_tx.clone()) {
                Ok(s) => s,
                Err(e) => {
                    tracing::error!(?e, "failed to build cpal stream");
                    return;
                }
            };
            if let Err(e) = stream.play() {
                tracing::error!(?e, "failed to start cpal stream");
                return;
            }
            // Drop our copy so only the cpal callback holds a sender; when
            // the stream drops, the callback's sender drops too.
            drop(raw_tx);

            run_worker(raw_rx, frame_tx, sample_rate, channels, stop_rx);

            // stream drops here → cpal stops the callback
            drop(stream);
        })
        .expect("spawn mic worker");

    Ok((
        MicCapture {
            stop_tx: Some(stop_tx),
            worker: Some(worker),
        },
        frame_rx,
    ))
}

fn build_stream(
    device: &Device,
    config: &StreamConfig,
    sample_format: SampleFormat,
    raw_tx: XSend<Vec<f32>>,
) -> Result<Stream, AudioError> {
    let err_cb = |e: cpal::StreamError| {
        tracing::error!(?e, "cpal stream error");
    };
    let stream = match sample_format {
        SampleFormat::F32 => {
            let tx = raw_tx;
            device.build_input_stream(
                config,
                move |data: &[f32], _| {
                    let _ = tx.send(data.to_vec());
                },
                err_cb,
                None,
            )?
        }
        SampleFormat::I16 => {
            let tx = raw_tx;
            device.build_input_stream(
                config,
                move |data: &[i16], _| {
                    let v: Vec<f32> = data.iter().map(|&s| s.to_sample::<f32>()).collect();
                    let _ = tx.send(v);
                },
                err_cb,
                None,
            )?
        }
        SampleFormat::U16 => {
            let tx = raw_tx;
            device.build_input_stream(
                config,
                move |data: &[u16], _| {
                    let v: Vec<f32> = data.iter().map(|&s| s.to_sample::<f32>()).collect();
                    let _ = tx.send(v);
                },
                err_cb,
                None,
            )?
        }
        other => {
            tracing::error!(?other, "unsupported sample format");
            return Err(AudioError::NoSupportedConfig);
        }
    };
    Ok(stream)
}

fn run_worker(
    raw_rx: XRecv<Vec<f32>>,
    frame_tx: UnboundedSender<AudioFrame>,
    source_rate: u32,
    channels: u16,
    stop_rx: XRecv<()>,
) {
    let mut normalizer = match Normalizer::new(source_rate, channels) {
        Ok(n) => n,
        Err(e) => {
            tracing::error!(?e, "failed to build resampler");
            return;
        }
    };
    let mut out_buf: Vec<f32> = Vec::with_capacity(INPUT_CHUNK_FRAMES * 2);

    loop {
        crossbeam_channel::select! {
            recv(stop_rx) -> _ => return,
            recv(raw_rx) -> msg => {
                let raw = match msg {
                    Ok(v) => v,
                    Err(_) => return, // sender side closed (stream dropped)
                };
                if let Err(e) = normalizer.push(&raw, &mut out_buf) {
                    tracing::error!(?e, "resampler failed; worker exiting");
                    return;
                }

                let captured_at = Instant::now();
                let mut idx = 0;
                while idx + FRAME_SAMPLES <= out_buf.len() {
                    let mut pcm = Vec::with_capacity(FRAME_SAMPLES);
                    pcm.extend_from_slice(&out_buf[idx..idx + FRAME_SAMPLES]);
                    let frame = AudioFrame {
                        pcm,
                        sample_rate: TARGET_SAMPLE_RATE,
                        channels: 1,
                        captured_at,
                    };
                    if frame_tx.send(frame).is_err() {
                        return;
                    }
                    idx += FRAME_SAMPLES;
                }
                out_buf.drain(..idx);
            }
        }
    }
}
