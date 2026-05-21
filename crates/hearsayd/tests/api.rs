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

#[tokio::test]
async fn get_config_returns_loaded_defaults() {
    let dir = tempfile::tempdir().unwrap();
    let app = build(dir.path());
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/config")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_json(resp).await;
    assert_eq!(body["server"]["port"], 7717);
    assert_eq!(body["summarization"]["model"], "gemma-3-12b");
}

#[tokio::test]
async fn audio_download_streams_wav_file() {
    use hearsay_storage::{AudioWriter, WavAudioWriter};

    let dir = tempfile::tempdir().unwrap();
    let app = build(dir.path());

    // Write a tiny real WAV so the audio download has something to serve.
    let audio_path = dir.path().join("audio.wav");
    let mut w: Box<dyn AudioWriter> = Box::new(WavAudioWriter::create(&audio_path).unwrap());
    w.write_pcm(&vec![0.1_f32; 1_600]).unwrap();
    w.finalize().unwrap();
    let expected_bytes = std::fs::read(&audio_path).unwrap();

    // Seed the session pointing at that file.
    let storage = Storage::open(dir.path().join("h.db")).unwrap();
    let id = SessionId::new();
    storage
        .insert_session(&SessionMeta {
            id,
            name: "audio test".into(),
            source_kind: SourceKind::Mic,
            source_meta: serde_json::Value::Null,
            language: None,
            audio_path,
            started_at: Utc::now(),
            ended_at: Some(Utc::now()),
            status: SessionStatus::Completed,
        })
        .unwrap();

    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/sessions/{id}/audio"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        resp.headers().get("content-type").unwrap(),
        "audio/wav"
    );
    let bytes = resp.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(bytes.as_ref(), expected_bytes.as_slice());
}

#[tokio::test]
async fn summarize_400_when_no_model_loaded() {
    // In tests, no Gemma model is present on disk, so AppState::new
    // resolves summarizer to None. /summarize should reject cleanly.
    let dir = tempfile::tempdir().unwrap();
    let app = build(dir.path());

    let storage = Storage::open(dir.path().join("h.db")).unwrap();
    let id = SessionId::new();
    storage
        .insert_session(&SessionMeta {
            id,
            name: "test".into(),
            source_kind: SourceKind::Mic,
            source_meta: serde_json::Value::Null,
            language: None,
            audio_path: dir.path().join("a.wav"),
            started_at: Utc::now(),
            ended_at: Some(Utc::now()),
            status: SessionStatus::Completed,
        })
        .unwrap();

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/sessions/{id}/summarize"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let body = body_json(resp).await;
    assert!(body["error"].as_str().unwrap().contains("model"));
}

#[tokio::test]
async fn segments_and_summaries_endpoints_return_arrays() {
    let dir = tempfile::tempdir().unwrap();
    let app = build(dir.path());
    let storage = Storage::open(dir.path().join("h.db")).unwrap();
    let id = SessionId::new();
    storage
        .insert_session(&SessionMeta {
            id,
            name: "seg".into(),
            source_kind: SourceKind::Mic,
            source_meta: serde_json::Value::Null,
            language: None,
            audio_path: dir.path().join("x.wav"),
            started_at: Utc::now(),
            ended_at: Some(Utc::now()),
            status: SessionStatus::Completed,
        })
        .unwrap();

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/sessions/{id}/segments"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(body_json(resp).await.is_array());

    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/sessions/{id}/summaries"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(body_json(resp).await.is_array());
}

#[tokio::test]
async fn audio_download_404_for_missing_file() {
    let dir = tempfile::tempdir().unwrap();
    let app = build(dir.path());

    // Session exists but its audio_path points nowhere on disk.
    let storage = Storage::open(dir.path().join("h.db")).unwrap();
    let id = SessionId::new();
    storage
        .insert_session(&SessionMeta {
            id,
            name: "ghost".into(),
            source_kind: SourceKind::Mic,
            source_meta: serde_json::Value::Null,
            language: None,
            audio_path: dir.path().join("missing.wav"),
            started_at: Utc::now(),
            ended_at: Some(Utc::now()),
            status: SessionStatus::Completed,
        })
        .unwrap();

    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/sessions/{id}/audio"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}
