//! Integration tests for the HTTP layer. Uses `tower::ServiceExt::oneshot`
//! to drive the router directly without binding a TCP socket. We don't try
//! to start mic capture in tests — that requires real hardware/permissions
//! and the CI host won't have a USB mic plugged in.

use std::sync::Arc;

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use chrono::Utc;
use hearsay_core::{SessionId, SessionMeta, SessionStatus, SourceKind};
use hearsay_storage::Storage;
use hearsayd::AppState;
use hearsayd::config::Config;
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;

fn build(data_dir: &std::path::Path) -> Router {
    let mut config = Config::default();
    config.paths.data_dir = Some(data_dir.to_path_buf());

    let storage = Storage::open(data_dir.join("h.db")).unwrap();
    let state = Arc::new(AppState::new(config, storage));
    hearsayd::build_router(state)
}

async fn body_json(resp: axum::response::Response) -> Value {
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn health_returns_ok() {
    let dir = tempfile::tempdir().unwrap();
    let app = build(dir.path());
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["status"], "ok");
    assert!(body["version"].is_string());
}

#[tokio::test]
async fn devices_endpoint_returns_array() {
    let dir = tempfile::tempdir().unwrap();
    let app = build(dir.path());
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/devices")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    // On CI without audio devices it may return an empty list, but the
    // call itself should succeed.
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert!(body.is_array(), "expected array, got {body:?}");
}

#[tokio::test]
async fn sessions_list_empty_initially() {
    let dir = tempfile::tempdir().unwrap();
    let app = build(dir.path());
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/sessions")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body.as_array().unwrap().len(), 0);
}

#[tokio::test]
async fn session_lifecycle_via_storage() {
    // We can't drive the create endpoint without real audio hardware, so
    // we go around it: seed a session through Storage and verify that GET,
    // list, and DELETE all see it.
    let dir = tempfile::tempdir().unwrap();
    let app = build(dir.path());

    let storage = Storage::open(dir.path().join("h.db")).unwrap();
    let id = SessionId::new();
    storage
        .insert_session(&SessionMeta {
            id,
            name: "seeded".into(),
            source_kind: SourceKind::Mic,
            source_meta: serde_json::Value::Null,
            language: Some("pl".into()),
            audio_path: dir.path().join("a.wav"),
            started_at: Utc::now(),
            ended_at: Some(Utc::now()),
            status: SessionStatus::Completed,
        })
        .unwrap();

    // GET by id
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/sessions/{id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["name"], "seeded");
    assert_eq!(body["language"], "pl");

    // List
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/sessions")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = body_json(resp).await;
    assert_eq!(body.as_array().unwrap().len(), 1);

    // DELETE
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/api/sessions/{id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // GET after delete returns 404
    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/sessions/{id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn bad_session_id_returns_400() {
    let dir = tempfile::tempdir().unwrap();
    let app = build(dir.path());
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/sessions/not-a-ulid")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}
