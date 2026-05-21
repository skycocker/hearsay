<script lang="ts">
  import { onMount } from "svelte";
  import type { InputDevice, SessionMeta } from "../lib/types";
  import { api } from "../lib/api";
  import { formatRelative } from "../lib/format";

  let sessions = $state<SessionMeta[]>([]);
  let devices = $state<InputDevice[]>([]);
  let selectedDevice = $state<string>("");
  let language = $state<string>("auto");
  let loading = $state(true);
  let error = $state<string | null>(null);
  let starting = $state(false);
  // Which session id has the "really delete?" prompt open. `null` = none.
  let pendingDelete = $state<string | null>(null);
  let deletingId = $state<string | null>(null);

  async function refresh() {
    try {
      const [s, d] = await Promise.all([api.listSessions(), api.listDevices()]);
      sessions = s;
      devices = d;
      if (!selectedDevice && d.length > 0) {
        selectedDevice = d.find((x) => x.is_default)?.id ?? d[0].id;
      }
      error = null;
    } catch (e) {
      error = (e as Error).message;
    } finally {
      loading = false;
    }
  }

  function askDelete(id: string, event: Event) {
    event.preventDefault();
    event.stopPropagation();
    pendingDelete = id;
  }

  function cancelDelete(event: Event) {
    event.preventDefault();
    event.stopPropagation();
    pendingDelete = null;
  }

  async function confirmDelete(id: string, event: Event) {
    event.preventDefault();
    event.stopPropagation();
    deletingId = id;
    try {
      await api.deleteSession(id);
      sessions = sessions.filter((s) => s.id !== id);
      pendingDelete = null;
    } catch (e) {
      error = `Failed to delete: ${(e as Error).message}`;
    } finally {
      deletingId = null;
    }
  }

  async function startRecording() {
    starting = true;
    try {
      const created = await api.startMic({
        device_id: selectedDevice || undefined,
        language: language === "auto" ? undefined : language,
      });
      location.hash = `#/sessions/${created.id}`;
    } catch (e) {
      error = (e as Error).message;
    } finally {
      starting = false;
    }
  }

  onMount(() => {
    refresh();
    const interval = setInterval(refresh, 4000);
    return () => clearInterval(interval);
  });
</script>

<section class="start">
  <h2>New recording</h2>
  <div class="form">
    <label>
      <span>Microphone</span>
      <select bind:value={selectedDevice} disabled={devices.length === 0}>
        {#if devices.length === 0}
          <option value="">No input devices found</option>
        {/if}
        {#each devices as d (d.id)}
          <option value={d.id}>
            {d.name}
            {d.is_default ? " (default)" : ""}
          </option>
        {/each}
      </select>
    </label>
    <label>
      <span>Language</span>
      <select bind:value={language}>
        <option value="auto">Auto-detect</option>
        <option value="pl">Polish</option>
        <option value="en">English</option>
        <option value="de">German</option>
        <option value="fr">French</option>
        <option value="es">Spanish</option>
      </select>
    </label>
    <button type="button" onclick={startRecording} disabled={starting || devices.length === 0}>
      {starting ? "Starting…" : "Start recording"}
    </button>
  </div>
</section>

<section>
  <h2>Sessions</h2>
  {#if error}
    <div class="error">{error}</div>
  {/if}
  {#if loading}
    <div class="muted">Loading…</div>
  {:else if sessions.length === 0}
    <div class="muted">No sessions yet. Start your first recording above.</div>
  {:else}
    <ul class="cards">
      {#each sessions as s (s.id)}
        <li>
          <a href="#/sessions/{s.id}" class="card">
            <div class="row">
              <span class="name">{s.name}</span>
              <span class="card-actions">
                <span class="status" data-status={s.status}>{s.status}</span>
                {#if pendingDelete === s.id}
                  <span class="confirm-row">
                    <span class="confirm-text">Delete this recording?</span>
                    <button
                      type="button"
                      class="delete-confirm"
                      disabled={deletingId === s.id}
                      onclick={(e) => confirmDelete(s.id, e)}
                    >
                      {deletingId === s.id ? "Deleting…" : "Yes, destroy"}
                    </button>
                    <button
                      type="button"
                      class="delete-cancel"
                      disabled={deletingId === s.id}
                      onclick={cancelDelete}
                    >
                      Cancel
                    </button>
                  </span>
                {:else}
                  <button
                    type="button"
                    class="delete-btn"
                    aria-label="Delete recording"
                    onclick={(e) => askDelete(s.id, e)}
                  >
                    ×
                  </button>
                {/if}
              </span>
            </div>
            <div class="meta">
              <span>{s.source_kind === "mic" ? "Mic" : s.source_kind === "system_audio" ? "System audio" : "Meet"}</span>
              <span>•</span>
              <span>{s.language ?? "auto"}</span>
              <span>•</span>
              <span>{formatRelative(s.started_at)}</span>
            </div>
          </a>
        </li>
      {/each}
    </ul>
  {/if}
</section>

<style>
  h2 {
    font-size: 0.78rem;
    font-weight: 600;
    color: #666;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    margin: 0 0 0.6rem 0;
  }
  .start {
    margin-bottom: 2rem;
  }
  .form {
    display: flex;
    gap: 0.75rem;
    align-items: end;
    background: #fff;
    padding: 1rem;
    border: 1px solid #e5e5e3;
    border-radius: 8px;
  }
  label {
    display: flex;
    flex-direction: column;
    gap: 0.25rem;
    flex: 1;
  }
  label span {
    font-size: 0.75rem;
    color: #666;
  }
  select {
    padding: 0.4rem 0.6rem;
    border: 1px solid #d0d0cc;
    border-radius: 5px;
    background: #fff;
    font: inherit;
  }
  button {
    padding: 0.5rem 1.1rem;
    background: #1a1a1a;
    color: #fff;
    border: none;
    border-radius: 5px;
    font: inherit;
    cursor: pointer;
  }
  button:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
  .cards {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 0.5rem;
  }
  .card {
    display: block;
    background: #fff;
    border: 1px solid #e5e5e3;
    border-radius: 8px;
    padding: 0.85rem 1rem;
    color: inherit;
    text-decoration: none;
    transition: border-color 120ms;
  }
  .card:hover {
    border-color: #c9c9c5;
  }
  .row {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 0.25rem;
  }
  .name {
    font-weight: 500;
  }
  .status {
    font-size: 0.72rem;
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
  .status[data-status="failed"] {
    background: #f5d6d1;
    color: #872a1f;
  }
  .card-actions {
    display: flex;
    align-items: center;
    gap: 0.4rem;
  }
  .delete-btn {
    /* Subtle by default; gets prominent on hover so a casual click doesn't
       wipe a meeting. */
    background: transparent;
    border: 1px solid transparent;
    color: #b0b0ad;
    font-size: 1.1rem;
    line-height: 1;
    padding: 0 0.45rem;
    border-radius: 4px;
    cursor: pointer;
    transition: color 120ms, background 120ms, border-color 120ms;
  }
  .delete-btn:hover {
    color: #872a1f;
    background: #fbe7e4;
    border-color: #f0c8c2;
  }
  .confirm-row {
    display: inline-flex;
    align-items: center;
    gap: 0.35rem;
  }
  .confirm-text {
    font-size: 0.78rem;
    color: #555;
  }
  .delete-confirm,
  .delete-cancel {
    font: inherit;
    font-size: 0.78rem;
    padding: 0.2rem 0.55rem;
    border-radius: 4px;
    border: 1px solid #d0d0cc;
    background: #fff;
    cursor: pointer;
  }
  .delete-confirm {
    background: #b73a2b;
    color: #fff;
    border-color: #b73a2b;
  }
  .delete-confirm:hover {
    background: #9c2e22;
  }
  .delete-confirm:disabled,
  .delete-cancel:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }
  .meta {
    display: flex;
    gap: 0.4rem;
    color: #777;
    font-size: 0.82rem;
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
