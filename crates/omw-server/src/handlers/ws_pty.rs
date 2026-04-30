//! Handler for `GET /internal/v1/sessions/:id/pty` (WebSocket upgrade).
//!
//! On upgrade:
//! - Spawn a forwarder pumping `SessionRegistry::subscribe(id)` → WS binary
//!   frames.
//! - Spawn a forwarder pumping inbound WS binary frames →
//!   `SessionRegistry::write_input(id, …)`.
//! - Close cleanly when either side hangs up.

// Intentionally empty: skeleton only.
