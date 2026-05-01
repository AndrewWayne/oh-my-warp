//! HTTP route handlers for `omw-remote`. See `specs/byorc-protocol.md` §3
//! (pairing) and the wiring plan for `/api/v1/host-info` and `/api/v1/sessions`.

pub mod host_info;
pub mod pair_redeem;
pub mod sessions;
