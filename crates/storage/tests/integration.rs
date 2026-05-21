use std::path::PathBuf;

use chrono::Utc;
use hearsay_core::{Segment, SessionId, SessionMeta, SessionStatus, SourceKind, Speaker, Summary};
use hearsay_storage::Storage;

fn sample_session(audio_dir: &std::path::Path) -> SessionMeta {
    SessionMeta {
        id: SessionId::new(),
        name: "team standup".into(),
        source_kind: SourceKind::Mic,
        source_meta: serde_json::json!({ "device": "AT2020 USB" }),
        language: Some("pl".into()),
        audio_path: audio_dir.join("audio.wav"),
        started_at: Utc::now(),
        ended_at: None,
        status: SessionStatus::Active,
    }
}

#[test]
fn full_session_lifecycle() {
    let dir = tempfile::tempdir().unwrap();
    let store = Storage::open(dir.path().join("hearsay.db")).unwrap();

    let session = sample_session(dir.path());
    let id = session.id;
    store.insert_session(&session).unwrap();

    // Listing immediately includes the active session.
    let listed = store.list_sessions().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, id);
    assert_eq!(listed[0].source_kind, SourceKind::Mic);
    assert_eq!(listed[0].language.as_deref(), Some("pl"));

    // Append a few segments as if Whisper produced them.
    let fixture: &[(u32, u32, u32, &str)] = &[
        (0, 0, 2_500, "Dzień dobry"),
        (1, 2_500, 5_200, "Zaczynamy spotkanie"),
        (2, 5_200, 9_900, "Pierwsza sprawa to wdrożenie"),
    ];
    for &(seq, start, end, text) in fixture {
        store
            .insert_segment(&Segment {
                session_id: id,
                seq,
                start_ms: start,
                end_ms: end,
                text: text.to_owned(),
                language: Some("pl".into()),
                confidence: Some(0.95),
                speaker_id: None,
            })
            .unwrap();
    }

    let segments = store.list_segments(id).unwrap();
    assert_eq!(segments.len(), 3);
    assert_eq!(segments[0].text, "Dzień dobry");
    assert_eq!(segments[2].seq, 2);
    assert!(segments.iter().all(|s| s.session_id == id));
    assert!(segments.iter().all(|s| s.speaker_id.is_none()));

    // Pretend diarization ran — assign speakers to segments.
    store
        .upsert_speaker(&Speaker { session_id: id, speaker_id: 1, label: None })
        .unwrap();
    store
        .upsert_speaker(&Speaker { session_id: id, speaker_id: 2, label: None })
        .unwrap();
    store.set_segment_speaker(id, 0, 1).unwrap();
    store.set_segment_speaker(id, 1, 1).unwrap();
    store.set_segment_speaker(id, 2, 2).unwrap();

    let speakers = store.list_speakers(id).unwrap();
    assert_eq!(speakers.len(), 2);

    let segments = store.list_segments(id).unwrap();
    assert_eq!(segments[0].speaker_id, Some(1));
    assert_eq!(segments[2].speaker_id, Some(2));

    // User edits one speaker label.
    store
        .upsert_speaker(&Speaker {
            session_id: id,
            speaker_id: 1,
            label: Some("Anna".into()),
        })
        .unwrap();
    let speakers = store.list_speakers(id).unwrap();
    assert_eq!(speakers[0].label.as_deref(), Some("Anna"));
    assert!(speakers[1].label.is_none());

    // Store a summary.
    store
        .upsert_summary(&Summary {
            session_id: id,
            model: "gemma3-12b-q4".into(),
            content: "## TL;DR\nWdrożenie ruszy w przyszłym tygodniu.".into(),
            generated_at: Utc::now(),
        })
        .unwrap();
    let summaries = store.list_summaries(id).unwrap();
    assert_eq!(summaries.len(), 1);
    assert!(summaries[0].content.starts_with("## TL;DR"));

    // Finish the session.
    store.finish_session(id, SessionStatus::Completed, Utc::now()).unwrap();
    let after = store.get_session(id).unwrap().unwrap();
    assert_eq!(after.status, SessionStatus::Completed);
    assert!(after.ended_at.is_some());

    // Deleting the session cascades.
    store.delete_session(id).unwrap();
    assert!(store.get_session(id).unwrap().is_none());
    assert!(store.list_segments(id).unwrap().is_empty());
    assert!(store.list_speakers(id).unwrap().is_empty());
    assert!(store.list_summaries(id).unwrap().is_empty());
}

#[test]
fn foreign_keys_block_orphan_segments() {
    let store = Storage::in_memory().unwrap();
    let orphan = Segment {
        session_id: SessionId::new(),
        seq: 0,
        start_ms: 0,
        end_ms: 1_000,
        text: "hello".into(),
        language: None,
        confidence: None,
        speaker_id: None,
    };
    let err = store.insert_segment(&orphan).unwrap_err();
    let msg = format!("{err}");
    assert!(msg.contains("FOREIGN KEY") || msg.contains("foreign key"), "got: {msg}");
}

#[test]
fn list_sessions_sorted_newest_first() {
    let dir = tempfile::tempdir().unwrap();
    let store = Storage::open(dir.path().join("h.db")).unwrap();

    let a = SessionMeta {
        id: SessionId::new(),
        name: "older".into(),
        source_kind: SourceKind::Mic,
        source_meta: serde_json::Value::Null,
        language: None,
        audio_path: PathBuf::from("/tmp/a.wav"),
        started_at: Utc::now() - chrono::Duration::hours(2),
        ended_at: None,
        status: SessionStatus::Completed,
    };
    let b = SessionMeta {
        id: SessionId::new(),
        name: "newer".into(),
        source_kind: SourceKind::Meet,
        source_meta: serde_json::Value::Null,
        language: None,
        audio_path: PathBuf::from("/tmp/b.wav"),
        started_at: Utc::now(),
        ended_at: None,
        status: SessionStatus::Active,
    };
    store.insert_session(&a).unwrap();
    store.insert_session(&b).unwrap();

    let list = store.list_sessions().unwrap();
    assert_eq!(list[0].name, "newer");
    assert_eq!(list[1].name, "older");
}
