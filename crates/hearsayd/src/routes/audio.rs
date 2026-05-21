//! Download the recorded WAV for a session.
//!
//! For v1 this streams the full file with no Range header support; browsers
//! can play it but can't seek without buffering to the seek point. Range
//! support is a small follow-on once we have a real testing story for it.

use std::sync::Arc;

use axum::body::Body;
use axum::extract::{Path, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use hearsay_core::SessionId;
use tokio_util::io::ReaderStream;

use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

pub async fn download(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> ApiResult<Response> {
    let id: SessionId = id
        .parse()
        .map_err(|_| ApiError::BadRequest(format!("invalid session id: {id}")))?;
    let meta = state
        .storage
        .get_session(id)?
        .ok_or_else(|| ApiError::NotFound(format!("session {id}")))?;

    if !meta.audio_path.exists() {
        return Err(ApiError::NotFound(format!(
            "audio file for session {id} missing on disk"
        )));
    }

    let file = tokio::fs::File::open(&meta.audio_path)
        .await
        .map_err(|e| ApiError::Internal(format!("opening audio: {e}")))?;
    let size = file
        .metadata()
        .await
        .map_err(|e| ApiError::Internal(format!("stat audio: {e}")))?
        .len();

    let stream = ReaderStream::new(file);
    let body = Body::from_stream(stream);

    let filename = format!("{id}.wav");
    Ok((
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, HeaderValue::from_static("audio/wav")),
            (
                header::CONTENT_LENGTH,
                HeaderValue::from_str(&size.to_string()).unwrap(),
            ),
            (
                header::CONTENT_DISPOSITION,
                HeaderValue::from_str(&format!("inline; filename=\"{filename}\""))
                    .unwrap_or_else(|_| HeaderValue::from_static("inline")),
            ),
        ],
        body,
    )
        .into_response())
}
