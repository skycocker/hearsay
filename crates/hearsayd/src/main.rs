//! hearsayd — the hearsay daemon.
//!
//! Owns audio capture, transcription, summarization, storage, and the
//! HTTP/WS server. Implementation across tasks #4–#13.

use tracing_subscriber::EnvFilter;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    tracing::info!("hearsayd v{} — scaffolding only, see tasks for implementation", env!("CARGO_PKG_VERSION"));
}
