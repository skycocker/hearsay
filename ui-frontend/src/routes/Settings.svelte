<script lang="ts">
  import { onMount } from "svelte";
  import type { Config } from "../lib/types";
  import { api } from "../lib/api";

  let config = $state<Config | null>(null);
  let health = $state<{ version: string } | null>(null);
  let error = $state<string | null>(null);

  onMount(async () => {
    try {
      [config, health] = await Promise.all([api.config(), api.health()]);
    } catch (e) {
      error = (e as Error).message;
    }
  });
</script>

<h2>Settings</h2>
<p class="muted">Read-only for now — edit <code>~/.config/hearsay/config.toml</code> and restart the daemon.</p>

{#if error}
  <div class="error">{error}</div>
{/if}

{#if config && health}
  <dl>
    <dt>Version</dt><dd>{health.version}</dd>

    <dt>Server</dt><dd>{config.server.host}:{config.server.port}</dd>

    <dt>Data directory</dt><dd>{config.paths.data_dir ?? "(platform default)"}</dd>

    <dt>Transcription model</dt><dd>{config.transcription.model}</dd>
    <dt>Model path</dt><dd>{config.transcription.model_path ?? "(derived from model name)"}</dd>
    <dt>Whisper threads</dt><dd>{config.transcription.n_threads}</dd>
    <dt>Default language</dt><dd>{config.transcription.default_language}</dd>

    <dt>Summarization model</dt><dd>{config.summarization.model}</dd>
    <dt>Summarizer model path</dt><dd>{config.summarization.model_path ?? "(derived from model name)"}</dd>
    <dt>Context window</dt><dd>{config.summarization.n_ctx.toLocaleString()} tokens</dd>
    <dt>GPU layers</dt><dd>{config.summarization.n_gpu_layers}</dd>
    <dt>Max summary tokens</dt><dd>{config.summarization.max_tokens.toLocaleString()}</dd>
    <dt>Keep summarizer loaded</dt><dd>{config.summarization.keep_loaded ? "yes" : "no"}</dd>
  </dl>
{:else if !error}
  <div class="muted">Loading…</div>
{/if}

<style>
  h2 {
    margin: 0 0 0.4rem 0;
    font-size: 1.1rem;
  }
  .muted {
    color: #777;
    margin-bottom: 1rem;
    font-size: 0.88rem;
  }
  code {
    background: #efeeec;
    padding: 0.1rem 0.3rem;
    border-radius: 3px;
    font-size: 0.85rem;
  }
  dl {
    display: grid;
    grid-template-columns: max-content 1fr;
    gap: 0.35rem 1.5rem;
    background: #fff;
    border: 1px solid #e5e5e3;
    border-radius: 8px;
    padding: 1rem 1.25rem;
  }
  dt {
    color: #777;
    font-size: 0.85rem;
  }
  dd {
    margin: 0;
    font-variant-numeric: tabular-nums;
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
