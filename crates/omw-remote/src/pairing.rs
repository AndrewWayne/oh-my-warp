//! Pairing flow. See `specs/byorc-protocol.md` §3.

use std::time::Duration;

use chrono::{DateTime, Utc};
use rusqlite::Connection;
use thiserror::Error;

use crate::capability::{Capability, CapabilityToken};
use crate::host_key::HostKey;

/// 256-bit single-use pairing secret.
pub struct PairToken(pub [u8; 32]);

impl PairToken {
    /// Generate a fresh token from the OS RNG.
    pub fn random() -> Self {
        unimplemented!("PairToken::random")
    }

    /// Decode from Crockford base32 (52 chars).
    pub fn from_base32(_s: &str) -> Result<Self, ParseError> {
        unimplemented!("PairToken::from_base32")
    }

    /// Encode to Crockford base32.
    pub fn to_base32(&self) -> String {
        unimplemented!("PairToken::to_base32")
    }

    /// SHA-256 of the raw token bytes (this is what the DB stores).
    pub fn hash(&self) -> PairTokenHash {
        unimplemented!("PairToken::hash")
    }
}

/// SHA-256 digest of a `PairToken`. Stored in `pairings.token_hash`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PairTokenHash(pub [u8; 32]);

#[derive(Debug, Error)]
pub enum ParseError {
    #[error("malformed pair token")]
    Malformed,
}

/// Pairings registry, backed by SQLite.
pub struct Pairings {
    // db connection + clock
}

impl Pairings {
    /// New registry over `db`. Caller is responsible for migrations (`open_db`).
    pub fn new(_db: Connection) -> Self {
        unimplemented!("Pairings::new")
    }

    /// Construct with an injectable clock — used by tests to drive TTL boundaries.
    pub fn new_with_clock(_db: Connection, _now: fn() -> DateTime<Utc>) -> Self {
        unimplemented!("Pairings::new_with_clock")
    }

    /// Issue a new pair token with `ttl` (default per spec: 10 min).
    /// Inserts a row into `pairings` keyed by the SHA-256 of the token.
    pub fn issue(&self, _ttl: Duration) -> Result<PairToken, RedeemError> {
        unimplemented!("Pairings::issue")
    }

    /// Test-only: install a fake "now" used for TTL checks.
    pub fn set_clock(&mut self, _now: fn() -> DateTime<Utc>) {
        unimplemented!("Pairings::set_clock")
    }

    /// Redeem a pair token: validate, mark used, mint a capability token.
    pub fn redeem(
        &self,
        _token: &PairToken,
        _device_pubkey: &[u8; 32],
        _device_name: &str,
        _host: &HostKey,
        _capabilities: &[Capability],
    ) -> Result<PairRedeemResponse, RedeemError> {
        unimplemented!("Pairings::redeem")
    }
}

#[derive(Debug, PartialEq, Eq, Error)]
pub enum RedeemError {
    #[error("token_unknown")]
    TokenUnknown,
    #[error("token_expired")]
    TokenExpired,
    #[error("token_already_used")]
    TokenAlreadyUsed,
    #[error("invalid_pubkey")]
    InvalidPubkey,
    #[error("invalid_body")]
    InvalidBody,
    #[error("internal: {0}")]
    Internal(String),
}

/// 200 response body for `POST /api/v1/pair/redeem`. See spec §3.2.
#[derive(Debug)]
pub struct PairRedeemResponse {
    pub device_id: String,
    pub capabilities: Vec<Capability>,
    pub capability_token: CapabilityToken,
    pub host_pubkey: [u8; 32],
}
