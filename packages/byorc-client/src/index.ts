// Public entry for @oh-my-warp/byorc-client.
//
// The environment-agnostic BYORC protocol client: Ed25519 request signing,
// the signed HTTP API client, session management, pairing, and the PTY
// WebSocket bridge. Depends only on `@noble/*` plus the standard `fetch` and
// `WebSocket` globals (present in browsers and Node >= 22), so it is consumable
// from both the Web Controller and Node daemon clients such as omw-mcp.
// Browser-only concerns (IndexedDB persistence, xterm, and the visibility-
// driven reconnect wrapper) deliberately stay in the Web Controller.
//
// Subpath exports (e.g. "@oh-my-warp/byorc-client/pty-ws") mirror the original
// module layout; this barrel re-exports the whole surface for convenience.
export * from "./crypto/canonical";
export * from "./crypto/ed25519";
export * from "./api/types";
export * from "./api/client";
export * from "./pairing";
export * from "./sessions";
export * from "./pty-ws";
