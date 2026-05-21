import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

// During `vite` dev: proxy REST + WS calls to a running hearsayd on :7717
// so the dev UI works without rebuilding the daemon.
//
// During `vite build`: emits to dist/, which the daemon bakes into its
// binary at compile time via rust-embed.
export default defineConfig({
  plugins: [svelte()],
  server: {
    proxy: {
      "/api": "http://127.0.0.1:7717",
      "/ws": {
        target: "ws://127.0.0.1:7717",
        ws: true,
      },
    },
  },
  build: {
    outDir: "dist",
    emptyOutDir: true,
  },
});
