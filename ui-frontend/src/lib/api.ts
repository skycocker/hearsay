// Tiny typed fetch wrapper around the hearsayd REST API. Reads the host
// from window.location so the same bundle works whether it's served by the
// daemon (production) or proxied by Vite (dev).

import type { Config, InputDevice, Segment, SessionMeta, Summary } from "./types";

async function json<T>(res: Response): Promise<T> {
  if (!res.ok) {
    let detail = "";
    try {
      const body = await res.json();
      detail = body?.error ?? "";
    } catch {
      detail = await res.text().catch(() => "");
    }
    throw new Error(`${res.status} ${res.statusText}${detail ? ` — ${detail}` : ""}`);
  }
  return res.json() as Promise<T>;
}

export const api = {
  health: () => fetch("/api/health").then(json<{ status: string; version: string }>),

  config: () => fetch("/api/config").then(json<Config>),

  listDevices: () => fetch("/api/devices").then(json<InputDevice[]>),

  listSessions: () => fetch("/api/sessions").then(json<SessionMeta[]>),

  getSession: (id: string) => fetch(`/api/sessions/${id}`).then(json<SessionMeta>),

  startMic: (input: {
    name?: string;
    language?: string;
    device_id?: string;
  }) =>
    fetch("/api/sessions", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ source: "mic", ...input }),
    }).then(json<SessionMeta>),

  stopSession: (id: string) =>
    fetch(`/api/sessions/${id}/stop`, { method: "POST" }).then(json<SessionMeta>),

  deleteSession: (id: string) =>
    fetch(`/api/sessions/${id}`, { method: "DELETE" }).then((r) => {
      if (!r.ok) throw new Error(`${r.status} ${r.statusText}`);
    }),

  audioUrl: (id: string) => `/api/sessions/${id}/audio`,

  listSegments: (id: string) =>
    fetch(`/api/sessions/${id}/segments`).then(json<Segment[]>),

  listSummaries: (id: string) =>
    fetch(`/api/sessions/${id}/summaries`).then(json<Summary[]>),

  summarize: (id: string) =>
    fetch(`/api/sessions/${id}/summarize`, { method: "POST" }).then(json<Summary>),
};

// Build a ws:// or wss:// URL against the host the page was served from.
// Falls back to localhost when running under Vite's dev server.
function wsBase(): string {
  const proto = location.protocol === "https:" ? "wss:" : "ws:";
  return `${proto}//${location.host}`;
}

export const ws = {
  live: (id: string) => new WebSocket(`${wsBase()}/ws/sessions/${id}/live`),
  replay: (id: string, opts?: { speed?: number; start_ms?: number }) => {
    const qs = new URLSearchParams();
    if (opts?.speed) qs.set("speed", String(opts.speed));
    if (opts?.start_ms) qs.set("start_ms", String(opts.start_ms));
    const q = qs.toString();
    return new WebSocket(`${wsBase()}/ws/sessions/${id}/replay${q ? `?${q}` : ""}`);
  },
};
