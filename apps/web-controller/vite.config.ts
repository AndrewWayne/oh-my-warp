/// <reference types="vitest" />
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { VitePWA } from "vite-plugin-pwa";

export default defineConfig({
  plugins: [
    react(),
    VitePWA({
      registerType: "autoUpdate",
      // Registered manually in src/main.tsx via `virtual:pwa-register`.
      injectRegister: null,
      // Keep the hand-authored public/manifest.webmanifest (linked in
      // index.html); the plugin only manages the service worker.
      manifest: false,
      includeAssets: ["icon-192.png", "icon-512.png"],
      workbox: {
        // Precache the built app shell — cache-first for static assets only.
        globPatterns: ["**/*.{js,css,html,ico,png,svg,webmanifest}"],
        // SPA navigation fallback, but NEVER for API or WS paths.
        navigateFallback: "/index.html",
        navigateFallbackDenylist: [/^\/api\//, /^\/ws\//],
        // Auth + stream data must never be cached (BYORC requests are signed
        // and replay-protected; a cached response would be a stale/replayed
        // auth artifact). NetworkOnly for the signed HTTP API. WebSocket
        // upgrades (`/ws/v1/*`) bypass the service-worker fetch handler
        // entirely, so they need no rule — but see the QA checklist.
        runtimeCaching: [
          {
            urlPattern: ({ url }) => url.pathname.startsWith("/api/v1/"),
            handler: "NetworkOnly",
          },
        ],
      },
      devOptions: {
        enabled: false,
      },
    }),
  ],
  test: {
    environment: "jsdom",
    globals: true,
    setupFiles: ["./tests/setup.ts"],
  },
});
