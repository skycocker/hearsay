//! Manual end-to-end verification tool.
//!
//! Reads a 16 kHz mono WAV file (or anything `hound` can decode), runs it
//! through the transcribe pipeline, and — if a Gemma model is also
//! provided — through the summarize pipeline. Prints the segments and
//! summary so you can eyeball whether the AI actually works on real audio
//! on a target machine.
//!
//! This is intentionally separate from the daemon — it bypasses cpal, the
//! HTTP server, and the WAV writer, so you can verify the *AI* pieces in
//! isolation from the audio capture stack.
//!
//! Usage:
//!     hearsay-pipeline <audio.wav> --whisper PATH [--gemma PATH] [--language LANG]

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use hearsay_core::{Segment, SessionId};
use hearsay_summarize::{Summarizer, SummarizerConfig};
use hearsay_transcribe::{TranscribedSegment, TranscriberConfig, TranscriptionWorker};

struct Args {
    audio: PathBuf,
    whisper_model: PathBuf,
    gemma_model: Option<PathBuf>,
    language: Option<String>,
}

fn parse_args() -> Result<Args, String> {
    let raw: Vec<String> = std::env::args().skip(1).collect();
    let mut audio: Option<PathBuf> = None;
    let mut whisper_model: Option<PathBuf> = None;
    let mut gemma_model: Option<PathBuf> = None;
    let mut language: Option<String> = None;

    let mut i = 0;
    while i < raw.len() {
        let arg = &raw[i];
        match arg.as_str() {
            "--whisper" => {
                i += 1;
                whisper_model = raw.get(i).map(PathBuf::from);
            }
            "--gemma" => {
                i += 1;
                gemma_model = raw.get(i).map(PathBuf::from);
            }
            "--language" => {
                i += 1;
                language = raw.get(i).cloned();
            }
            "-h" | "--help" => return Err("help".into()),
            _ if audio.is_none() => audio = Some(PathBuf::from(arg)),
            other => return Err(format!("unexpected argument: {other}")),
        }
        i += 1;
    }

    let audio = audio.ok_or_else(|| "missing audio file path".to_owned())?;
    let whisper_model = whisper_model.ok_or_else(|| "missing --whisper PATH".to_owned())?;
    Ok(Args { audio, whisper_model, gemma_model, language })
}

fn usage() {
    eprintln!(
        "hearsay-pipeline: end-to-end transcribe+summarize verification tool

Usage:
    hearsay-pipeline <audio.wav> --whisper PATH [--gemma PATH] [--language LANG]

    audio.wav      WAV file (any rate; mono is best, stereo gets averaged).
    --whisper      Path to a whisper.cpp ggml-*.bin model.
    --gemma        Optional path to a Gemma .gguf. Runs summarization too.
    --language     ISO-639-1 language hint, e.g. 'pl' or 'en'. Default: auto.

Example:
    hearsay-pipeline meeting.wav \\
        --whisper ~/Library/Application\\ Support/hearsay/models/ggml-tiny.bin \\
        --gemma   ~/Library/Application\\ Support/hearsay/models/gemma-3-1b.gguf \\
        --language pl"
    );
}

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .init();

    let args = match parse_args() {
        Ok(a) => a,
        Err(e) if e == "help" => {
            usage();
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            eprintln!("error: {e}\n");
            usage();
            return ExitCode::FAILURE;
        }
    };

    let segments = match transcribe(&args) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("transcribe failed: {e}");
            return ExitCode::FAILURE;
        }
    };

    println!("\n=== Transcript ({} segments) ===\n", segments.len());
    for s in &segments {
        println!(
            "[{:>5} ms → {:>5} ms]  {}",
            s.start_ms, s.end_ms, s.text
        );
    }

    if let Some(gemma_path) = args.gemma_model.as_ref() {
        match summarize(gemma_path, &segments, args.language.as_deref()) {
            Ok(content) => {
                println!("\n=== Summary ===\n");
                println!("{content}\n");
            }
            Err(e) => {
                eprintln!("\nsummarize failed: {e}");
                return ExitCode::FAILURE;
            }
        }
    } else {
        println!("\n(no --gemma supplied; skipping summarization)");
    }

    ExitCode::SUCCESS
}

fn transcribe(args: &Args) -> Result<Vec<Segment>, String> {
    let pcm = load_wav_as_16k_mono_f32(&args.audio)?;
    println!("Loaded {} samples ({:.1} s of audio)", pcm.len(), pcm.len() as f32 / 16_000.0);

    let language = match args.language.as_deref() {
        Some("auto") | None => None,
        Some(s) => Some(s.to_owned()),
    };

    let cfg = TranscriberConfig {
        model_path: args.whisper_model.clone(),
        language,
        n_threads: 4,
    };
    let started = Instant::now();
    let (worker, mut rx) = TranscriptionWorker::start(cfg).map_err(|e| format!("{e}"))?;
    println!("Whisper model loaded in {:.1} s", started.elapsed().as_secs_f32());

    // Feed everything in one push — the chunker buffers up to its window
    // size and emits chunks as needed.
    let session_id = SessionId::new();
    worker.feed(pcm).map_err(|e| format!("{e}"))?;
    drop(worker);

    let started = Instant::now();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("runtime: {e}"))?;
    let raw: Vec<TranscribedSegment> = rt.block_on(async {
        let mut out = Vec::new();
        while let Some(s) = rx.recv().await {
            out.push(s);
        }
        out
    });
    println!("Transcribed in {:.1} s", started.elapsed().as_secs_f32());

    Ok(raw
        .into_iter()
        .enumerate()
        .map(|(seq, s)| Segment {
            session_id,
            seq: seq as u32,
            start_ms: s.start_ms,
            end_ms: s.end_ms,
            text: s.text,
            language: s.language,
            confidence: None,
            speaker_id: None,
        })
        .collect())
}

fn summarize(model_path: &Path, segments: &[Segment], language: Option<&str>) -> Result<String, String> {
    let cfg = SummarizerConfig::for_model(model_path.to_path_buf());
    println!("\nLoading Gemma…");
    let started = Instant::now();
    let summarizer = Summarizer::new(cfg).map_err(|e| format!("{e}"))?;
    println!("Loaded in {:.1} s", started.elapsed().as_secs_f32());

    let started = Instant::now();
    let out = summarizer
        .summarize(segments, language)
        .map_err(|e| format!("{e}"))?;
    println!("Summarized in {:.1} s", started.elapsed().as_secs_f32());
    Ok(out)
}

/// Decode a WAV via `hound`, mix down to mono, naive linear-interpolation
/// resample to 16 kHz. Naive resample is fine for verification purposes —
/// the daemon uses rubato; this binary doesn't need that quality.
fn load_wav_as_16k_mono_f32(path: &Path) -> Result<Vec<f32>, String> {
    let reader = hound::WavReader::open(path).map_err(|e| format!("open wav: {e}"))?;
    let spec = reader.spec();
    let channels = spec.channels as usize;
    let sample_rate = spec.sample_rate;

    let interleaved: Vec<f32> = match spec.sample_format {
        hound::SampleFormat::Float => reader
            .into_samples::<f32>()
            .filter_map(Result::ok)
            .collect(),
        hound::SampleFormat::Int => {
            let bits = spec.bits_per_sample as u32;
            let scale = (1_i64 << (bits - 1)) as f32;
            reader
                .into_samples::<i32>()
                .filter_map(Result::ok)
                .map(|s| s as f32 / scale)
                .collect()
        }
    };

    // Channel mixdown.
    let mono: Vec<f32> = if channels == 1 {
        interleaved
    } else {
        let scale = 1.0 / channels as f32;
        interleaved
            .chunks_exact(channels)
            .map(|frame| frame.iter().sum::<f32>() * scale)
            .collect()
    };

    if sample_rate == 16_000 {
        return Ok(mono);
    }

    // Naive linear resample to 16 kHz.
    let ratio = 16_000.0_f64 / f64::from(sample_rate);
    let out_len = (mono.len() as f64 * ratio) as usize;
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let src_pos = i as f64 / ratio;
        let lo = src_pos.floor() as usize;
        let hi = (lo + 1).min(mono.len() - 1);
        let t = (src_pos - lo as f64) as f32;
        let a = mono.get(lo).copied().unwrap_or(0.0);
        let b = mono.get(hi).copied().unwrap_or(a);
        out.push(a + (b - a) * t);
    }
    Ok(out)
}
