//! WebSocket endpoints: live transcript stream and timed replay.

use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path, Query, State};
use axum::response::Response;
use hearsay_core::{Segment, SessionId};
use serde::Deserialize;
use tokio::sync::broadcast::error::RecvError;

use crate::error::{ApiError, ApiResult};
use crate::state::AppState;

/// Query params for the replay endpoint.
#[derive(Debug, Deserialize)]
pub struct ReplayParams {
    /// Playback speed multiplier. 1.0 = original cadence, 2.0 = twice as fast.
    /// Clamped to [0.25, 16.0] server-side so a bad client can't ask for
    /// effectively-paused or astronomically-fast streams.
    #[serde(default = "default_speed")]
    pub speed: f32,
    /// Start position in milliseconds from session start. Segments ending
    /// before this point are skipped entirely.
    #[serde(default)]
    pub start_ms: u32,
}

fn default_speed() -> f32 {
    1.0
}

pub async fn live(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    ws: WebSocketUpgrade,
) -> ApiResult<Response> {
    let id: SessionId = id
        .parse()
        .map_err(|_| ApiError::BadRequest(format!("invalid session id: {id}")))?;
    let rx = state
        .sessions
        .subscribe_live(id)
        .ok_or_else(|| ApiError::NotFound(format!("active session {id}")))?;
    Ok(ws.on_upgrade(move |socket| live_loop(socket, rx)))
}

pub async fn replay(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
    Query(params): Query<ReplayParams>,
    ws: WebSocketUpgrade,
) -> ApiResult<Response> {
    let id: SessionId = id
        .parse()
        .map_err(|_| ApiError::BadRequest(format!("invalid session id: {id}")))?;
    // Verify the session exists; otherwise clients get a confusing
    // "connected then nothing" experience.
    state
        .storage
        .get_session(id)?
        .ok_or_else(|| ApiError::NotFound(format!("session {id}")))?;
    let segments = state.storage.list_segments(id)?;
    Ok(ws.on_upgrade(move |socket| replay_loop(socket, segments, params)))
}

async fn live_loop(mut ws: WebSocket, mut rx: tokio::sync::broadcast::Receiver<Segment>) {
    loop {
        tokio::select! {
            recv = rx.recv() => match recv {
                Ok(seg) => {
                    if send_event(&mut ws, "segment", &seg).await.is_err() {
                        return;
                    }
                }
                Err(RecvError::Lagged(n)) => {
                    let _ = send_event(&mut ws, "lagged", &serde_json::json!({ "skipped": n })).await;
                }
                Err(RecvError::Closed) => {
                    let _ = send_event(&mut ws, "end", &serde_json::json!({})).await;
                    return;
                }
            },
            client = ws.recv() => {
                // We don't accept commands on /live — but we do need to
                // notice when the client disconnects so we don't loop
                // forever on a dead socket.
                match client {
                    Some(Ok(Message::Close(_))) | None => return,
                    Some(Ok(_)) => continue,
                    Some(Err(_)) => return,
                }
            }
        }
    }
}

async fn replay_loop(mut ws: WebSocket, segments: Vec<Segment>, params: ReplayParams) {
    let speed = params.speed.clamp(0.25, 16.0);
    let start_ms = params.start_ms;
    let real_start = Instant::now();

    let _ = send_event(
        &mut ws,
        "ready",
        &serde_json::json!({
            "total_segments": segments.len(),
            "speed": speed,
            "start_ms": start_ms,
        }),
    )
    .await;

    for seg in segments.into_iter().filter(|s| s.end_ms > start_ms) {
        // When in real time should this segment appear?
        let virtual_ms = seg.start_ms.saturating_sub(start_ms) as f32 / speed;
        let target = real_start + Duration::from_millis(virtual_ms as u64);
        let now = Instant::now();
        if target > now {
            tokio::select! {
                _ = tokio::time::sleep(target - now) => {}
                msg = ws.recv() => {
                    match msg {
                        Some(Ok(Message::Close(_))) | None => return,
                        Some(Ok(_)) => {} // ignore other messages for v1
                        Some(Err(_)) => return,
                    }
                }
            }
        }

        if send_event(&mut ws, "segment", &seg).await.is_err() {
            return;
        }
    }

    let _ = send_event(&mut ws, "end", &serde_json::json!({})).await;
}

async fn send_event<T: serde::Serialize>(
    ws: &mut WebSocket,
    kind: &str,
    payload: &T,
) -> Result<(), axum::Error> {
    let msg = serde_json::json!({ "type": kind, "data": payload });
    ws.send(Message::Text(msg.to_string().into())).await
}
