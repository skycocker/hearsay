// Mirrors the Rust types in hearsay-core. Keep this in sync manually for
// now — if it ever drifts past a few fields, we'll generate it with ts-rs
// or schemars+typify and never touch it again.

export type SessionId = string;

export type SourceKind = "mic" | "system_audio" | "meet";
export type SessionStatus = "active" | "completed" | "failed";

export interface SessionMeta {
  id: SessionId;
  name: string;
  source_kind: SourceKind;
  source_meta: unknown;
  language: string | null;
  audio_path: string;
  started_at: string;
  ended_at: string | null;
  status: SessionStatus;
}

export interface Segment {
  session_id: SessionId;
  seq: number;
  start_ms: number;
  end_ms: number;
  text: string;
  language: string | null;
  confidence: number | null;
  speaker_id: number | null;
}

export interface Speaker {
  session_id: SessionId;
  speaker_id: number;
  label: string | null;
}

export interface Summary {
  session_id: SessionId;
  model: string;
  content: string;
  generated_at: string;
}

export interface InputDevice {
  id: string;
  name: string;
  is_default: boolean;
  default_sample_rate: number;
  default_channels: number;
}

export interface Config {
  server: { host: string; port: number };
  paths: { data_dir: string | null };
  transcription: {
    model: string;
    model_path: string | null;
    n_threads: number;
    default_language: string;
  };
  summarization: { model: string; keep_loaded: boolean };
}

// WS event envelope used by both /live and /replay.
export type WsEvent =
  | { type: "ready"; data: { total_segments: number; speed: number; start_ms: number } }
  | { type: "segment"; data: Segment }
  | { type: "lagged"; data: { skipped: number } }
  | { type: "end"; data: Record<string, never> };
