//! WebSocket integration tests. We bind the router to a real ephemeral
//! port and drive it with tokio-tungstenite — `tower::oneshot` can't
//! perform the WS upgrade handshake.

use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::Utc;
use futures_util::StreamExt;
use hearsay_core::{Segment, SessionId, SessionMeta, SessionStatus, SourceKind};
use hearsay_storage::Storage;
use hearsayd::AppState;
use hearsayd::config::Config;
use serde_json::Value;
use tokio_tungstenite::tungstenite;

async fn spawn_server() -> (std::net::SocketAddr, tempfile::TempDir, Storage) {
    let dir = tempfile::tempdir().unwrap();
    let mut config = Config::default();
    config.paths.data_dir = Some(dir.path().to_path_buf());
    config.server.port = 0; // ephemeral

    let storage = Storage::open(dir.path().join("h.db")).unwrap();
    let state = Arc::new(AppState::new(config, storage.clone()));
    let app = hearsayd::build_router(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    (addr, dir, storage)
}

#[tokio::test]
async fn replay_streams_segments_at_scaled_cadence() {
    let (addr, _dir, storage) = spawn_server().await;

    // Seed a session with 3 segments spanning 3 seconds.
    let id = SessionId::new();
    storage
        .insert_session(&SessionMeta {
            id,
            name: "test".into(),
            source_kind: SourceKind::Mic,
            source_meta: Value::Null,
            language: Some("pl".into()),
            audio_path: "/tmp/x.wav".into(),
            started_at: Utc::now(),
            ended_at: Some(Utc::now()),
            status: SessionStatus::Completed,
        })
        .unwrap();
    for (seq, start, end, text) in [
        (0u32, 0u32, 500u32, "Dzień dobry"),
        (1, 1_000, 1_800, "Druga linia"),
        (2, 2_000, 2_900, "Trzecia linia"),
    ] {
        storage
            .insert_segment(&Segment {
                session_id: id,
                seq,
                start_ms: start,
                end_ms: end,
                text: text.into(),
                language: Some("pl".into()),
                confidence: Some(0.9),
                speaker_id: None,
            })
            .unwrap();
    }

    let url = format!("ws://{addr}/ws/sessions/{id}/replay?speed=20");
    let (mut ws, _resp) = tokio_tungstenite::connect_async(&url).await.unwrap();

    let started = Instant::now();
    let mut events = Vec::new();
    while let Some(msg) = ws.next().await {
        let msg = msg.unwrap();
        let tungstenite::Message::Text(text) = msg else { continue };
        let v: Value = serde_json::from_str(&text).unwrap();
        let kind = v["type"].as_str().unwrap().to_owned();
        events.push((kind.clone(), v));
        if kind == "end" {
            break;
        }
    }
    let elapsed = started.elapsed();

    // We should see: ready, segment, segment, segment, end.
    assert_eq!(events.len(), 5);
    assert_eq!(events[0].0, "ready");
    assert_eq!(events[1].0, "segment");
    assert_eq!(events[1].1["data"]["text"], "Dzień dobry");
    assert_eq!(events[2].1["data"]["text"], "Druga linia");
    assert_eq!(events[3].1["data"]["text"], "Trzecia linia");
    assert_eq!(events[4].0, "end");

    // At 20× speed, 3 seconds of audio should replay in ~150 ms. Generous
    // upper bound to keep the test reliable on slow CI.
    assert!(elapsed < Duration::from_millis(1_500), "took {elapsed:?}");
    // And it shouldn't finish *instantly* — we should observe at least
    // some pacing.
    assert!(elapsed >= Duration::from_millis(50), "too fast: {elapsed:?}");
}

#[tokio::test]
async fn replay_skips_segments_before_start_ms() {
    let (addr, _dir, storage) = spawn_server().await;
    let id = SessionId::new();
    storage
        .insert_session(&SessionMeta {
            id,
            name: "test".into(),
            source_kind: SourceKind::Mic,
            source_meta: Value::Null,
            language: None,
            audio_path: "/tmp/x.wav".into(),
            started_at: Utc::now(),
            ended_at: Some(Utc::now()),
            status: SessionStatus::Completed,
        })
        .unwrap();
    for (seq, start, end, text) in [
        (0u32, 0u32, 500u32, "skip me"),
        (1, 600, 1_000, "still skip"),
        (2, 1_500, 2_000, "keep me"),
    ] {
        storage
            .insert_segment(&Segment {
                session_id: id,
                seq,
                start_ms: start,
                end_ms: end,
                text: text.into(),
                language: None,
                confidence: None,
                speaker_id: None,
            })
            .unwrap();
    }

    let url = format!("ws://{addr}/ws/sessions/{id}/replay?speed=20&start_ms=1200");
    let (mut ws, _) = tokio_tungstenite::connect_async(&url).await.unwrap();

    let mut segments_seen = Vec::new();
    while let Some(Ok(msg)) = ws.next().await {
        let tungstenite::Message::Text(text) = msg else { continue };
        let v: Value = serde_json::from_str(&text).unwrap();
        match v["type"].as_str().unwrap() {
            "segment" => segments_seen.push(v["data"]["text"].as_str().unwrap().to_owned()),
            "end" => break,
            _ => {}
        }
    }
    assert_eq!(segments_seen, vec!["keep me".to_string()]);
}

#[tokio::test]
async fn live_returns_404_for_inactive_session() {
    let (addr, _dir, _) = spawn_server().await;
    let bogus = SessionId::new();
    let url = format!("ws://{addr}/ws/sessions/{bogus}/live");
    let err = tokio_tungstenite::connect_async(&url).await.unwrap_err();
    let msg = format!("{err}");
    // tungstenite reports the HTTP status when the upgrade fails.
    assert!(msg.contains("404"), "expected 404, got: {msg}");
}
