<script lang="ts">
  import { onMount, onDestroy } from "svelte";
  import type { Segment, SessionMeta, Summary, WsEvent } from "../lib/types";
  import { api, ws } from "../lib/api";
  import { formatDate, formatDuration } from "../lib/format";

  let { id }: { id: string } = $props();

  let session = $state<SessionMeta | null>(null);
  let segments = $state<Segment[]>([]);
  let summaries = $state<Summary[]>([]);
  let error = $state<string | null>(null);
  let socket: WebSocket | null = null;
  let stopping = $state(false);
  let speed = $state(1);
  let summarizing = $state(false);

  async function refreshMeta() {
    try {
      session = await api.getSession(id);
      summaries = await api.listSummaries(id);
      // For completed sessions, also fetch stored segments so we have them
      // before WS replay arrives.
      if (session?.status !== "active") {
        segments = await api.listSegments(id);
      }
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
    if (session.status === "active") {
      // Keep already-stored segments visible, append new ones from WS.
      socket = ws.live(id);
    } else {
      // Replay: clear and re-stream at cadence.
      segments = [];
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
    socket.onerror = () => {};
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

  async function generateSummary() {
    summarizing = true;
    try {
      const fresh = await api.summarize(id);
      summaries = [fresh, ...summaries.filter((s) => s.model !== fresh.model)];
    } catch (e) {
      error = (e as Error).message;
    } finally {
      summarizing = false;
    }
  }

  function changeSpeed(next: number) {
    speed = next;
    openSocket();
  }

  $effect(() => {
    if (session) openSocket();
  });

  onMount(() => {
    refreshMeta();
    // Poll for summary updates on completed sessions — auto-summary
    // is fire-and-forget and lands when ready.
    const interval = setInterval(async () => {
      if (session && session.status !== "active") {
        try {
          summaries = await api.listSummaries(id);
        } catch {}
      }
    }, 5000);
    return () => clearInterval(interval);
  });
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
    <section class="summary">
      <div class="summary-head">
        <h2>Summary</h2>
        <button onclick={generateSummary} disabled={summarizing}>
          {summarizing ? "Generating…" : summaries.length > 0 ? "Re-generate" : "Generate"}
        </button>
      </div>
      {#if summaries.length === 0}
        <p class="muted small">No summary yet. The daemon auto-summarizes after stop if the Gemma model is loaded — otherwise click Generate.</p>
      {:else}
        {#each summaries as s (s.model)}
          <article class="summary-card">
            <div class="summary-meta">
              <span>{s.model}</span>
              <span>•</span>
              <span>{formatDate(s.generated_at)}</span>
            </div>
            <div class="summary-content">{@html renderMarkdown(s.content)}</div>
          </article>
        {/each}
      {/if}
    </section>

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
          : "No transcript stored — transcription may not have run."}
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

<script lang="ts" module>
  // Minimal Markdown rendering: headings (## / ###), bullets (- / *), bold
  // (**x**), and italics (*x*). Newlines become <br>. Enough to render
  // Gemma's summary output without pulling in marked/markdown-it.
  export function renderMarkdown(src: string): string {
    const esc = (s: string) =>
      s.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
    let html = esc(src);
    html = html.replace(/^### (.+)$/gm, "<h4>$1</h4>");
    html = html.replace(/^## (.+)$/gm, "<h3>$1</h3>");
    html = html.replace(/^# (.+)$/gm, "<h2>$1</h2>");
    html = html.replace(/^\s*[-*] (.+)$/gm, "<li>$1</li>");
    // Wrap consecutive <li> into <ul>.
    html = html.replace(/(<li>[\s\S]*?<\/li>)(\n(?=<li>))/g, "$1");
    html = html.replace(/(<li>[\s\S]*?<\/li>(?:\n<li>[\s\S]*?<\/li>)*)/g, "<ul>$1</ul>");
    html = html.replace(/\*\*(.+?)\*\*/g, "<strong>$1</strong>");
    html = html.replace(/(^|[^*])\*([^*\n]+)\*/g, "$1<em>$2</em>");
    html = html.replace(/\n{2,}/g, "</p><p>");
    html = `<p>${html}</p>`;
    return html;
  }
</script>

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
  .summary {
    background: #fff;
    border: 1px solid #e5e5e3;
    border-radius: 8px;
    padding: 0.85rem 1.1rem;
    margin-bottom: 0.75rem;
  }
  .summary-head {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 0.5rem;
  }
  .summary-head h2 {
    margin: 0;
    font-size: 0.85rem;
    font-weight: 600;
    color: #666;
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }
  .summary-head button {
    padding: 0.3rem 0.8rem;
    font: inherit;
    border-radius: 4px;
    border: 1px solid #d0d0cc;
    background: #fff;
    cursor: pointer;
    color: #1a1a1a;
    font-size: 0.85rem;
  }
  .summary-head button:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
  .summary-card {
    border-top: 1px solid #f0f0ee;
    padding-top: 0.6rem;
    margin-top: 0.6rem;
  }
  .summary-card:first-of-type {
    border-top: none;
    padding-top: 0;
    margin-top: 0;
  }
  .summary-meta {
    display: flex;
    gap: 0.4rem;
    font-size: 0.78rem;
    color: #999;
    margin-bottom: 0.4rem;
  }
  .summary-content :global(h2),
  .summary-content :global(h3) {
    font-size: 0.95rem;
    margin: 0.6rem 0 0.25rem 0;
  }
  .summary-content :global(ul) {
    margin: 0.2rem 0 0.6rem 1.2rem;
    padding: 0;
  }
  .summary-content :global(li) {
    margin: 0.15rem 0;
  }
  .summary-content :global(p) {
    margin: 0.2rem 0;
    line-height: 1.45;
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
  .muted.small {
    padding: 0.5rem 0;
    font-size: 0.85rem;
    text-align: left;
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
