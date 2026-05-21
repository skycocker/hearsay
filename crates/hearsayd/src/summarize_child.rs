//! Spawn `hearsay-summarize-child` as a subprocess to run summarization.
//!
//! Required to dodge the llama-cpp-2 + whisper-rs ggml-state collision in
//! one process — see `hearsay-summarize-child` for the gory details. The
//! daemon never loads llama directly; it shells out for every summary.

use std::path::PathBuf;
use std::process::Stdio;

use hearsay_core::Segment;
use serde::Serialize;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

use crate::config::SummarizationConfig;

#[derive(Debug, thiserror::Error)]
pub enum SummarizeChildError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("serialize request: {0}")]
    Serialize(#[from] serde_json::Error),
    #[error("child exited with status {status}: {stderr}")]
    ChildFailed { status: i32, stderr: String },
    #[error("child binary `{0}` not found")]
    BinaryMissing(PathBuf),
}

#[derive(Debug, Serialize)]
struct Request<'a> {
    model_path: PathBuf,
    n_ctx: u32,
    n_gpu_layers: u32,
    max_tokens: u32,
    language: Option<&'a str>,
    segments: &'a [Segment],
}

pub async fn run(
    config: &SummarizationConfig,
    data_dir: &std::path::Path,
    child_binary: &PathBuf,
    segments: &[Segment],
    language: Option<&str>,
) -> Result<String, SummarizeChildError> {
    if !child_binary.exists() {
        return Err(SummarizeChildError::BinaryMissing(child_binary.clone()));
    }

    let req = Request {
        model_path: config.resolved_model_path(data_dir),
        n_ctx: config.n_ctx,
        n_gpu_layers: config.n_gpu_layers,
        max_tokens: config.max_tokens,
        language,
        segments,
    };
    let payload = serde_json::to_vec(&req)?;

    let mut child = Command::new(child_binary)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(&payload).await?;
        // Flush + close so the child sees EOF and starts work.
        stdin.shutdown().await?;
    }

    let output = child.wait_with_output().await?;
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    // The child's stderr is useful diagnostic noise — forward it to our
    // tracing layer so it ends up alongside the daemon's own logs.
    for line in stderr.lines() {
        tracing::debug!(target: "hearsay::summarize::child", "{line}");
    }

    if !output.status.success() {
        return Err(SummarizeChildError::ChildFailed {
            status: output.status.code().unwrap_or(-1),
            stderr: stderr.trim().to_owned(),
        });
    }

    let content = String::from_utf8_lossy(&output.stdout).into_owned();
    Ok(content)
}

/// Default location for the worker binary: alongside the daemon's binary.
/// Falls back to "hearsay-summarize-child" on PATH if that fails.
pub fn default_child_path() -> PathBuf {
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            let candidate = parent.join("hearsay-summarize-child");
            if candidate.exists() {
                return candidate;
            }
        }
    }
    PathBuf::from("hearsay-summarize-child")
}
