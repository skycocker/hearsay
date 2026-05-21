//! hearsayd — the hearsay daemon.

use std::sync::Arc;

use anyhow::Context;
use hearsay_storage::Storage;
use hearsayd::config::Config;
use hearsayd::{AppState, build_router};
use tracing_subscriber::EnvFilter;

fn main() -> anyhow::Result<()> {
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

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;

    // macOS with tray: tray loop OWNS the main thread (AppKit requirement),
    // server runs on a worker. Tray.run() never returns.
    #[cfg(all(feature = "tray", target_os = "macos"))]
    {
        let tray_state = Arc::clone(&state);
        let port = config.server.port;
        runtime.spawn(serve(addr, app));
        tracing::info!("hearsayd listening; macOS tray on main thread");
        hearsayd::tray::run(port, tray_state);
        unreachable!("tray::run blocks forever");
    }

    // Linux with tray: GTK on a dedicated thread, server on main.
    #[cfg(all(feature = "tray", target_os = "linux"))]
    {
        let tray_state = Arc::clone(&state);
        let port = config.server.port;
        std::thread::Builder::new()
            .name("hearsay-tray".into())
            .spawn(move || {
                if let Err(e) = gtk::init() {
                    tracing::warn!(?e, "GTK init failed; system tray disabled");
                    return;
                }
                hearsayd::tray::run(port, tray_state);
            })
            .expect("spawn tray thread");
    }

    // Headless / unsupported-tray-platform / `--no-default-features`: server
    // on the main thread, no tray.
    #[cfg(any(not(feature = "tray"), all(feature = "tray", target_os = "linux")))]
    {
        tracing::info!(%addr, data_dir = ?data_dir, "hearsayd listening");
        runtime.block_on(serve(addr, app))?;
        Ok(())
    }
}

async fn serve(addr: String, app: axum::Router) -> anyhow::Result<()> {
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .with_context(|| format!("binding {addr}"))?;
    axum::serve(listener, app).await?;
    Ok(())
}
