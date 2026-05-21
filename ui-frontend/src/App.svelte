<script lang="ts">
  import { onMount } from "svelte";
  import SessionsList from "./routes/SessionsList.svelte";
  import SessionDetail from "./routes/SessionDetail.svelte";
  import Settings from "./routes/Settings.svelte";

  type Route =
    | { name: "sessions" }
    | { name: "session"; id: string }
    | { name: "settings" };

  function parseHash(): Route {
    const h = location.hash.replace(/^#\/?/, "");
    if (h === "" || h === "/") return { name: "sessions" };
    if (h.startsWith("sessions/")) return { name: "session", id: h.slice("sessions/".length) };
    if (h === "settings") return { name: "settings" };
    return { name: "sessions" };
  }

  let route = $state<Route>(parseHash());

  onMount(() => {
    const handler = () => (route = parseHash());
    window.addEventListener("hashchange", handler);
    return () => window.removeEventListener("hashchange", handler);
  });
</script>

<header>
  <a class="brand" href="#/">hearsay</a>
  <nav>
    <a href="#/" class:active={route.name === "sessions"}>Sessions</a>
    <a href="#/settings" class:active={route.name === "settings"}>Settings</a>
  </nav>
</header>

<main>
  {#if route.name === "sessions"}
    <SessionsList />
  {:else if route.name === "session"}
    <SessionDetail id={route.id} />
  {:else if route.name === "settings"}
    <Settings />
  {/if}
</main>

<style>
  :global(body) {
    margin: 0;
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", system-ui, sans-serif;
    color: #1a1a1a;
    background: #f7f7f5;
  }
  header {
    display: flex;
    align-items: center;
    gap: 2rem;
    padding: 0.75rem 1.5rem;
    background: #fff;
    border-bottom: 1px solid #e5e5e3;
    box-shadow: 0 1px 2px rgba(0, 0, 0, 0.02);
  }
  .brand {
    font-weight: 600;
    font-size: 1.05rem;
    color: #111;
    text-decoration: none;
    letter-spacing: 0.01em;
  }
  nav {
    display: flex;
    gap: 0.25rem;
  }
  nav a {
    padding: 0.35rem 0.7rem;
    border-radius: 5px;
    color: #555;
    text-decoration: none;
    font-size: 0.9rem;
    transition: background 120ms, color 120ms;
  }
  nav a:hover {
    background: #f0f0ee;
    color: #1a1a1a;
  }
  nav a.active {
    background: #1a1a1a;
    color: #fff;
  }
  main {
    max-width: 980px;
    margin: 0 auto;
    padding: 1.5rem;
  }
</style>
