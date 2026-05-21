//! Standalone summarize test — loads ONLY the Gemma/Llama model with no
//! whisper init in the same process. Used to confirm whether the
//! whisper-rs + llama-cpp-2 collision is the source of the segfault we
//! see in `hearsay-pipeline`.
//!
//! Usage: hearsay-summarize-test <model.gguf> [--language LANG] [--n-ctx N]

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use hearsay_core::{Segment, SessionId};
use hearsay_summarize::{Summarizer, SummarizerConfig};

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(model_path) = args.next() else {
        eprintln!("usage: hearsay-summarize-test <model.gguf> [--language LANG] [--n-ctx N]");
        return ExitCode::FAILURE;
    };
    let mut language: Option<String> = None;
    let mut n_ctx: u32 = 4096;
    while let Some(flag) = args.next() {
        match flag.as_str() {
            "--language" => language = args.next(),
            "--n-ctx" => n_ctx = args.next().and_then(|s| s.parse().ok()).unwrap_or(4096),
            other => {
                eprintln!("unexpected argument: {other}");
                return ExitCode::FAILURE;
            }
        }
    }

    let session_id = SessionId::new();
    let segments = vec![
        Segment {
            session_id,
            seq: 0,
            start_ms: 0,
            end_ms: 4500,
            text: "Hello everyone, today we will discuss the rollout of the new monitoring system.".into(),
            language: language.clone(),
            confidence: None,
            speaker_id: Some(1),
        },
        Segment {
            session_id,
            seq: 1,
            start_ms: 4500,
            end_ms: 7000,
            text: "Alice will lead the migration starting Monday.".into(),
            language: language.clone(),
            confidence: None,
            speaker_id: Some(1),
        },
        Segment {
            session_id,
            seq: 2,
            start_ms: 7000,
            end_ms: 10500,
            text: "We need to verify compatibility with the existing database.".into(),
            language: language.clone(),
            confidence: None,
            speaker_id: Some(2),
        },
    ];

    let mut cfg = SummarizerConfig::for_model(PathBuf::from(&model_path));
    cfg.n_ctx = n_ctx;
    println!("Loading model: {model_path}");
    println!("n_ctx: {n_ctx}");
    let started = Instant::now();
    let summarizer = match Summarizer::new(cfg) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("load failed: {e}");
            return ExitCode::FAILURE;
        }
    };
    println!("Loaded in {:.1} s", started.elapsed().as_secs_f32());

    let started = Instant::now();
    match summarizer.summarize(&segments, language.as_deref()) {
        Ok(content) => {
            println!("Summarized in {:.1} s", started.elapsed().as_secs_f32());
            println!("\n=== Summary ===\n{content}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("summarize failed: {e}");
            ExitCode::FAILURE
        }
    }
}
