use std::sync::Arc;

use axum::Router;
use axum::routing::{get, post};

use crate::state::AppState;

pub mod devices;
pub mod health;
pub mod sessions;

pub fn build_router(state: Arc<AppState>) -> Router {
    let api = Router::new()
        .route("/health", get(health::get_health))
        .route("/devices", get(devices::list_devices))
        .route("/sessions", get(sessions::list).post(sessions::create))
        .route("/sessions/{id}", get(sessions::get_one).delete(sessions::delete))
        .route("/sessions/{id}/stop", post(sessions::stop));

    Router::new()
        .nest("/api", api)
        .with_state(state)
}
