//! hearsayd — the hearsay daemon.

use std::sync::Arc;

use anyhow::Context;
use hearsay_storage::Storage;
use hearsayd::{AppState, build_router};
use hearsayd::config::Config;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let config = Config::load().context("loading config")?;
    let data_dir = config.resolved_data_dir();
    std::fs::create_dir_all(&data_dir).context("creating data dir")?;

    let db_path = data_dir.join("hearsay.db");
    let storage = Storage::open(&db_path).context("opening storage")?;

    let state = Arc::new(AppState::new(config.clone(), storage));
    let app = build_router(Arc::clone(&state));

    let addr = format!("{}:{}", config.server.host, config.server.port);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .with_context(|| format!("binding {addr}"))?;
    tracing::info!(%addr, data_dir = ?data_dir, "hearsayd listening");

    axum::serve(listener, app).await?;
    Ok(())
}
