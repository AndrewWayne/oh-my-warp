//! `omw-remote` — GUI-anchored PTY bridge + BYORC daemon.
//!
//! Phase D — auth core (pairing + capability tokens + signed-request verifier).
//! Phase E — WS framing for PTY sessions: signed `Frame` envelope, per-frame
//! verification ladder (§7.3), and `/ws/v1/pty/:session_id` route.
//!
//! v0.4 implementation gates on
//! [`specs/byorc-protocol.md`](../../../specs/byorc-protocol.md). See
//! [PRD §8.2](../../../PRD.md#82-components),
//! [PRD §5.2](../../../PRD.md#52-byorc--bring-your-own-remote-controller),
//! and [`docs/omw-remote-implementation.md`](../../../docs/omw-remote-implementation.md).

pub mod auth;
pub mod capability;
pub mod db;
pub mod host_key;
pub mod http;
pub mod pairing;
pub mod replay;
pub mod request_log;
pub mod revocations;
pub mod server;
pub mod web_assets;
pub mod ws;

pub use auth::{AuthError, CanonicalRequest, DeviceId, Signer, Verifier};
pub use capability::{Capability, CapabilityError, CapabilityToken, ParseError as CapParseError};
pub use db::{open_db, schema_version};
pub use host_key::HostKey;
pub use pairing::{
    PairRedeemResponse, PairToken, PairTokenHash, Pairings, ParseError as PairParseError,
    RedeemError,
};
pub use replay::{NonceError, NonceStore};
pub use request_log::{RequestLog, RequestLogEntry};
pub use revocations::RevocationList;
pub use server::{make_router, serve, ServerConfig};
pub use ws::{Frame, FrameAuthError, FrameError, FrameKind, ShellSpec, WsSessionAuth};
