//! `omw-remote` — GUI-anchored PTY bridge + BYORC daemon.
//!
//! Phase D — auth core (pairing + capability tokens + signed-request verifier).
//! v0.4 implementation gates on
//! [`specs/byorc-protocol.md`](../../../specs/byorc-protocol.md). See
//! [PRD §8.2](../../../PRD.md#82-components),
//! [PRD §5.2](../../../PRD.md#52-byorc--bring-your-own-remote-controller),
//! and [`docs/omw-remote-implementation.md`](../../../docs/omw-remote-implementation.md).

pub mod auth;
pub mod capability;
pub mod db;
pub mod host_key;
pub mod pairing;
pub mod replay;
pub mod request_log;

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
