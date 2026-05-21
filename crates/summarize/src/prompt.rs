//! Prompt construction for meeting summarization.
//!
//! Two responsibilities:
//! - Pick a language-appropriate system prompt so Gemma replies in the same
//!   language the meeting was in.
//! - Render the transcript as `[HH:MM:SS] Speaker N: text` lines so the
//!   model has timeline anchors and speaker attribution to ground its
//!   summary.
//!
//! Output structure is hard-coded to three sections — TL;DR / Decisions /
//! Action items — across all languages, because mixing structure with
//! language tends to confuse smaller quants.

use hearsay_core::Segment;

const SUPPORTED_LANGUAGES: &[(&str, &str, &str, &str)] = &[
    // ISO-639-1, system prompt template
    (
        "en",
        "You are an assistant that summarizes meeting transcripts. \
         Read the transcript below and produce a structured summary in English.",
        "Transcript:",
        "Use this exact structure (Markdown):\n\
         ## TL;DR\n2-3 sentences capturing the main outcomes.\n\n\
         ## Decisions\nBulleted list of decisions made. Omit the section if none.\n\n\
         ## Action items\nBulleted `Owner: task` where owners are mentioned, otherwise just the task. Omit the section if none.",
    ),
    (
        "pl",
        "Jesteś asystentem podsumowującym transkrypcje spotkań. \
         Przeczytaj poniższą transkrypcję i przygotuj uporządkowane podsumowanie po polsku.",
        "Transkrypcja:",
        "Użyj dokładnie tej struktury (Markdown):\n\
         ## TL;DR\n2-3 zdania ujmujące główne wnioski.\n\n\
         ## Decyzje\nLista wypunktowana podjętych decyzji. Pomiń sekcję, jeśli nie ma decyzji.\n\n\
         ## Zadania\nLista wypunktowana `Osoba: zadanie` jeśli wskazano osobę, w innym przypadku samo zadanie. Pomiń sekcję, jeśli nie ma zadań.",
    ),
];

pub struct SummaryPrompt {
    pub system: String,
    pub user: String,
}

pub fn build(segments: &[Segment], language: Option<&str>) -> SummaryPrompt {
    let lang = language.unwrap_or("en");
    let (_, intro, transcript_header, structure) = SUPPORTED_LANGUAGES
        .iter()
        .find(|(code, _, _, _)| *code == lang)
        .copied()
        .unwrap_or(SUPPORTED_LANGUAGES[0]);

    let system = format!("{intro}\n\n{structure}");
    let transcript = format_transcript(segments);
    let user = format!("{transcript_header}\n{transcript}");

    SummaryPrompt { system, user }
}

fn format_transcript(segments: &[Segment]) -> String {
    let mut out = String::with_capacity(segments.len() * 64);
    for seg in segments {
        let ts = format_timestamp(seg.start_ms);
        let speaker = seg
            .speaker_id
            .map(|s| format!("Speaker {s}: "))
            .unwrap_or_default();
        let text = seg.text.trim();
        if text.is_empty() {
            continue;
        }
        out.push_str(&format!("[{ts}] {speaker}{text}\n"));
    }
    out
}

fn format_timestamp(ms: u32) -> String {
    let total = ms / 1000;
    let h = total / 3600;
    let m = (total % 3600) / 60;
    let s = total % 60;
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m:02}:{s:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hearsay_core::SessionId;

    fn seg(seq: u32, start_ms: u32, end_ms: u32, text: &str, speaker: Option<u32>) -> Segment {
        Segment {
            session_id: SessionId::new(),
            seq,
            start_ms,
            end_ms,
            text: text.to_owned(),
            language: None,
            confidence: None,
            speaker_id: speaker,
        }
    }

    #[test]
    fn timestamp_under_hour_uses_mm_ss() {
        assert_eq!(format_timestamp(0), "00:00");
        assert_eq!(format_timestamp(5_000), "00:05");
        assert_eq!(format_timestamp(125_000), "02:05");
    }

    #[test]
    fn timestamp_over_hour_uses_h_mm_ss() {
        assert_eq!(format_timestamp(3_600_000), "1:00:00");
        assert_eq!(format_timestamp(3_725_000), "1:02:05");
    }

    #[test]
    fn transcript_drops_blank_segments() {
        let s = vec![
            seg(0, 0, 1_000, "Hello", Some(1)),
            seg(1, 1_000, 2_000, "  ", Some(2)),
            seg(2, 2_000, 3_000, "World", None),
        ];
        let p = build(&s, Some("en"));
        assert!(p.user.contains("[00:00] Speaker 1: Hello"));
        assert!(!p.user.contains("[00:01]")); // blank segment skipped
        assert!(p.user.contains("[00:02] World"));
    }

    #[test]
    fn polish_uses_polish_intro_and_structure_headers() {
        let p = build(&[seg(0, 0, 1_000, "Dzień dobry", Some(1))], Some("pl"));
        assert!(p.system.contains("polsku"));
        assert!(p.system.contains("Decyzje"));
        assert!(p.system.contains("Zadania"));
        assert!(p.user.contains("Transkrypcja:"));
    }

    #[test]
    fn unsupported_language_falls_back_to_english_template() {
        // Whisper might tag a segment with a language we don't have a
        // template for — better to summarize in English than refuse.
        let p = build(&[seg(0, 0, 1_000, "Bonjour", None)], Some("fr"));
        assert!(p.system.contains("English"));
    }

    #[test]
    fn no_language_specified_falls_back_to_english() {
        let p = build(&[seg(0, 0, 1_000, "hi", None)], None);
        assert!(p.system.contains("English"));
    }
}
