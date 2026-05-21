use std::sync::Arc;

use axum::Router;
use axum::routing::{get, post};

use crate::state::AppState;

pub mod audio;
pub mod config;
pub mod devices;
pub mod health;
pub mod sessions;
pub mod ws;

pub fn build_router(state: Arc<AppState>) -> Router {
    let api = Router::new()
        .route("/health", get(health::get_health))
        .route("/devices", get(devices::list_devices))
        .route("/config", get(config::get_config))
        .route("/sessions", get(sessions::list).post(sessions::create))
        .route("/sessions/{id}", get(sessions::get_one).delete(sessions::delete))
        .route("/sessions/{id}/stop", post(sessions::stop))
        .route("/sessions/{id}/audio", get(audio::download));

    let ws_routes = Router::new()
        .route("/sessions/{id}/live", get(ws::live))
        .route("/sessions/{id}/replay", get(ws::replay));

    Router::new()
        .nest("/api", api)
        .nest("/ws", ws_routes)
        .with_state(state)
}
