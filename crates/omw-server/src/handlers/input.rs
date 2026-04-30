//! Handler for `POST /internal/v1/sessions/:id/input`.
//!
//! Body shape is `{ "bytes": "<base64>" }`. Executor will decode and forward
//! to `SessionRegistry::write_input`.

// Intentionally empty: skeleton only.
