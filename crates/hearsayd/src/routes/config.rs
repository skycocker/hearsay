use std::sync::Arc;

use axum::Json;
use axum::extract::State;

use crate::config::Config;
use crate::error::ApiResult;
use crate::state::AppState;

pub async fn get_config(State(state): State<Arc<AppState>>) -> ApiResult<Json<Config>> {
    // Returns whatever was loaded at startup. PUT is task TBD — we'll
    // surface "restart required" fields when we add it.
    Ok(Json((*state.config).clone()))
}
