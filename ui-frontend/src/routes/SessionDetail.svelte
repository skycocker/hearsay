<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import type { Segment, SessionMeta, WsEvent } from "../lib/types";
  import { api, ws } from "../lib/api";
  import { formatDate, formatDuration } from "../lib/format";

  let { id }: { id: string } = $props();

  let session = $state<SessionMeta | null>(null);
  let segments = $state<Segment[]>([]);
  let error = $state<string | null>(null);
  let socket: WebSocket | null = null;
  let stopping = $state(false);
  let speed = $state(1);

  async function refreshMeta() {
    try {
      session = await api.getSession(id);
    } catch (e) {
      error = (e as Error).message;
    }
  }

  function openSocket() {
    if (!session) return;
    if (socket) {
      socket.close();
      socket = null;
    }
    segments = [];
    if (session.status === "active") {
      socket = ws.live(id);
    } else {
      socket = ws.replay(id, { speed });
    }
    socket.onmessage = (ev) => {
      try {
        const evt = JSON.parse(ev.data) as WsEvent;
        if (evt.type === "segment") {
          segments = [...segments, evt.data];
        } else if (evt.type === "end") {
          socket?.close();
          socket = null;
        }
      } catch {}
    };
    socket.onerror = () => {
      // Live socket on a non-active session 404s — that's expected.
    };
  }

  async function stop() {
    stopping = true;
    try {
      await api.stopSession(id);
      await refreshMeta();
      openSocket();
    } catch (e) {
      error = (e as Error).message;
    } finally {
      stopping = false;
    }
  }

  function changeSpeed(next: number) {
    speed = next;
    openSocket();
  }

  $effect(() => {
    if (session) openSocket();
  });

  onMount(refreshMeta);
  onDestroy(() => socket?.close());
</script>

<a class="back" href="#/">← All sessions</a>

{#if error}
  <div class="error">{error}</div>
{/if}

{#if !session}
  <div class="muted">Loading…</div>
{:else}
  <header class="detail">
    <div>
      <h1>{session.name}</h1>
      <div class="meta">
        <span class="status" data-status={session.status}>{session.status}</span>
        <span>•</span>
        <span>{session.source_kind === "mic" ? "Microphone" : session.source_kind === "system_audio" ? "System audio" : "Meet"}</span>
        <span>•</span>
        <span>{session.language ?? "auto"}</span>
        <span>•</span>
        <span>started {formatDate(session.started_at)}</span>
      </div>
    </div>
    <div class="actions">
      {#if session.status === "active"}
        <button class="stop" onclick={stop} disabled={stopping}>
          {stopping ? "Stopping…" : "Stop recording"}
        </button>
      {:else}
        <a class="download" href={api.audioUrl(id)}>Download audio</a>
      {/if}
    </div>
  </header>

  {#if session.status !== "active"}
    <div class="controls">
      Replay speed:
      {#each [0.5, 1, 1.5, 2, 4] as s (s)}
        <button class:active={speed === s} onclick={() => changeSpeed(s)}>{s}×</button>
      {/each}
    </div>
  {/if}

  <section class="transcript">
    {#if segments.length === 0}
      <div class="muted">
        {session.status === "active"
          ? "Listening… transcript will appear as soon as the first segment lands."
          : "No transcript yet — transcription runs once the session is stopped (task #6)."}
      </div>
    {:else}
      <ul>
        {#each segments as seg (seg.seq)}
          <li>
            <span class="ts">{formatDuration(seg.start_ms)}</span>
            {#if seg.speaker_id !== null}
              <span class="speaker">Speaker {seg.speaker_id}</span>
            {/if}
            <span class="text">{seg.text}</span>
          </li>
        {/each}
      </ul>
    {/if}
  </section>
{/if}

<style>
  .back {
    display: inline-block;
    color: #555;
    text-decoration: none;
    margin-bottom: 1rem;
    font-size: 0.9rem;
  }
  .back:hover {
    color: #1a1a1a;
  }
  .detail {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    background: #fff;
    border: 1px solid #e5e5e3;
    border-radius: 8px;
    padding: 1rem 1.25rem;
    margin-bottom: 0.75rem;
  }
  h1 {
    font-size: 1.2rem;
    margin: 0 0 0.3rem 0;
  }
  .meta {
    display: flex;
    gap: 0.4rem;
    color: #777;
    font-size: 0.85rem;
    align-items: center;
  }
  .status {
    font-size: 0.7rem;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    padding: 0.15rem 0.5rem;
    border-radius: 3px;
    background: #efeeec;
    color: #555;
  }
  .status[data-status="active"] {
    background: #d9f0d1;
    color: #2a6720;
  }
  .actions button,
  .actions a {
    padding: 0.5rem 1rem;
    border-radius: 5px;
    font: inherit;
    text-decoration: none;
    cursor: pointer;
    border: 1px solid #d0d0cc;
    background: #fff;
    color: #1a1a1a;
  }
  .actions button.stop {
    background: #b73a2b;
    color: #fff;
    border-color: #b73a2b;
  }
  .controls {
    display: flex;
    gap: 0.3rem;
    align-items: center;
    margin-bottom: 0.75rem;
    color: #666;
    font-size: 0.88rem;
  }
  .controls button {
    padding: 0.2rem 0.6rem;
    border: 1px solid #d0d0cc;
    border-radius: 4px;
    background: #fff;
    font: inherit;
    cursor: pointer;
    color: #555;
  }
  .controls button.active {
    background: #1a1a1a;
    color: #fff;
    border-color: #1a1a1a;
  }
  .transcript {
    background: #fff;
    border: 1px solid #e5e5e3;
    border-radius: 8px;
    padding: 0.5rem 0;
    max-height: 70vh;
    overflow-y: auto;
  }
  .transcript ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .transcript li {
    display: grid;
    grid-template-columns: 4.5rem auto 1fr;
    gap: 0.6rem;
    padding: 0.4rem 1rem;
    border-bottom: 1px solid #f3f3f1;
    align-items: baseline;
  }
  .transcript li:last-child {
    border-bottom: none;
  }
  .ts {
    color: #999;
    font-variant-numeric: tabular-nums;
    font-size: 0.85rem;
  }
  .speaker {
    font-size: 0.78rem;
    color: #555;
    padding: 0.1rem 0.4rem;
    background: #efeeec;
    border-radius: 3px;
  }
  .text {
    line-height: 1.45;
  }
  .muted {
    color: #999;
    padding: 1rem;
    text-align: center;
  }
  .error {
    color: #872a1f;
    background: #fbe7e4;
    padding: 0.6rem 0.8rem;
    border-radius: 5px;
    margin-bottom: 0.75rem;
    font-size: 0.9rem;
  }
</style>
