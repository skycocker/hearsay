use std::sync::Arc;

use axum::Json;
use axum::extract::{Path, State};
use hearsay_core::{SessionId, SessionMeta, Summary};
use serde::Deserialize;

use crate::error::{ApiError, ApiResult};
use crate::session_manager::StartParams;
use crate::state::AppState;

#[derive(Debug, Deserialize)]
#[serde(tag = "source", rename_all = "snake_case")]
pub enum CreateRequest {
    Mic {
        name: Option<String>,
        language: Option<String>,
        device_id: Option<String>,
    },
}

pub async fn list(State(state): State<Arc<AppState>>) -> ApiResult<Json<Vec<SessionMeta>>> {
    Ok(Json(state.storage.list_sessions()?))
}

pub async fn create(
    State(state): State<Arc<AppState>>,
    Json(req): Json<CreateRequest>,
) -> ApiResult<Json<SessionMeta>> {
    let (name, language, params) = match req {
        CreateRequest::Mic { name, language, device_id } => (
            name.unwrap_or_else(default_session_name),
            language,
            StartParams::Mic { device_id },
        ),
    };
    Ok(Json(state.sessions.start(name, language, params)?))
}

pub async fn get_one(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> ApiResult<Json<SessionMeta>> {
    let id: SessionId = id
        .parse()
        .map_err(|_| ApiError::BadRequest(format!("invalid session id: {id}")))?;
    state
        .storage
        .get_session(id)?
        .map(Json)
        .ok_or_else(|| ApiError::NotFound(format!("session {id}")))
}

pub async fn delete(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> ApiResult<axum::http::StatusCode> {
    let id: SessionId = id
        .parse()
        .map_err(|_| ApiError::BadRequest(format!("invalid session id: {id}")))?;
    if state.sessions.is_active(id) {
        return Err(ApiError::BadRequest(
            "session is active; stop it first".into(),
        ));
    }
    state.storage.delete_session(id)?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}

pub async fn stop(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> ApiResult<Json<SessionMeta>> {
    let id: SessionId = id
        .parse()
        .map_err(|_| ApiError::BadRequest(format!("invalid session id: {id}")))?;
    Ok(Json(state.sessions.stop(id).await?))
}

pub async fn list_summaries(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> ApiResult<Json<Vec<Summary>>> {
    let id: SessionId = id
        .parse()
        .map_err(|_| ApiError::BadRequest(format!("invalid session id: {id}")))?;
    Ok(Json(state.storage.list_summaries(id)?))
}

pub async fn summarize(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> ApiResult<Json<Summary>> {
    let id: SessionId = id
        .parse()
        .map_err(|_| ApiError::BadRequest(format!("invalid session id: {id}")))?;
    Ok(Json(state.sessions.resummarize(id).await?))
}

pub async fn list_segments(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> ApiResult<Json<Vec<hearsay_core::Segment>>> {
    let id: SessionId = id
        .parse()
        .map_err(|_| ApiError::BadRequest(format!("invalid session id: {id}")))?;
    Ok(Json(state.storage.list_segments(id)?))
}

fn default_session_name() -> String {
    chrono::Local::now().format("Recording %Y-%m-%d %H:%M").to_string()
}
