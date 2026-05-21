//! Production summarizer worker. Reads a JSON request from stdin, loads
//! the model, runs summarization, prints ONLY the markdown summary on
//! stdout, and exits.
//!
//! Spawned by hearsayd per summarize request — keeping summarization in a
//! child process sidesteps a llama-cpp-2 + whisper-rs collision that
//! reliably segfaults inside llama.cpp's model load when both libraries
//! share a process (verified on macOS 14.8 / M2 Pro, llama-cpp-2 0.1.146).
//!
//! stdin JSON shape:
//! ```json
//! {
//!     "model_path": "/path/to/model.gguf",
//!     "n_ctx": 4096,
//!     "n_gpu_layers": 999,
//!     "max_tokens": 1500,
//!     "language": "pl",
//!     "segments": [{ ...Segment fields... }, ...]
//! }
//! ```
//! stdout: the model's markdown output.
//! stderr: progress / errors (free-form, ignored by parent).
//! exit code: 0 success, non-zero = stderr explains.

use std::io::Read;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use hearsay_core::Segment;
use hearsay_summarize::{Summarizer, SummarizerConfig};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Request {
    model_path: PathBuf,
    #[serde(default = "default_n_ctx")]
    n_ctx: u32,
    #[serde(default = "default_n_gpu_layers")]
    n_gpu_layers: u32,
    #[serde(default = "default_max_tokens")]
    max_tokens: u32,
    language: Option<String>,
    segments: Vec<Segment>,
}

fn default_n_ctx() -> u32 {
    4_096
}
fn default_n_gpu_layers() -> u32 {
    999
}
fn default_max_tokens() -> u32 {
    1_500
}

fn main() -> ExitCode {
    let mut buf = String::new();
    if let Err(e) = std::io::stdin().read_to_string(&mut buf) {
        eprintln!("read stdin: {e}");
        return ExitCode::from(2);
    }
    let req: Request = match serde_json::from_str(&buf) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("parse json: {e}");
            return ExitCode::from(2);
        }
    };

    eprintln!(
        "[child] model={:?} n_ctx={} segments={} language={:?}",
        req.model_path, req.n_ctx, req.segments.len(), req.language
    );

    let cfg = SummarizerConfig {
        model_path: req.model_path,
        n_ctx: req.n_ctx,
        n_threads: 0,
        n_gpu_layers: req.n_gpu_layers,
        max_tokens: req.max_tokens,
        seed: 42,
    };

    let started = Instant::now();
    let summarizer = match Summarizer::new(cfg) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("load: {e}");
            return ExitCode::from(3);
        }
    };
    eprintln!("[child] loaded in {:.1} s", started.elapsed().as_secs_f32());

    let started = Instant::now();
    match summarizer.summarize(&req.segments, req.language.as_deref()) {
        Ok(content) => {
            eprintln!("[child] summarized in {:.1} s", started.elapsed().as_secs_f32());
            print!("{content}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("summarize: {e}");
            ExitCode::from(4)
        }
    }
}
