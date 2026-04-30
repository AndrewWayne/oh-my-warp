//! Pairing flow. See `specs/byorc-protocol.md` §3.

use std::sync::Mutex;
use std::time::Duration;

use chrono::{DateTime, Utc};
use ed25519_dalek::VerifyingKey;
use rand::rngs::OsRng;
use rand::RngCore;
use rusqlite::{params, Connection, OptionalExtension};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::capability::{Capability, CapabilityToken};
use crate::host_key::HostKey;

/// 256-bit single-use pairing secret.
pub struct PairToken(pub [u8; 32]);

impl PairToken {
    /// Generate a fresh token from the OS RNG.
    pub fn random() -> Self {
        let mut bytes = [0u8; 32];
        OsRng.fill_bytes(&mut bytes);
        Self(bytes)
    }

    /// Decode from Crockford base32.
    pub fn from_base32(s: &str) -> Result<Self, ParseError> {
        let bytes = base32::decode(base32::Alphabet::Crockford, s).ok_or(ParseError::Malformed)?;
        if bytes.len() != 32 {
            return Err(ParseError::Malformed);
        }
        let mut out = [0u8; 32];
        out.copy_from_slice(&bytes);
        Ok(Self(out))
    }

    /// Encode to Crockford base32.
    pub fn to_base32(&self) -> String {
        base32::encode(base32::Alphabet::Crockford, &self.0)
    }

    /// SHA-256 of the raw token bytes (this is what the DB stores).
    pub fn hash(&self) -> PairTokenHash {
        let digest = Sha256::digest(self.0);
        let mut out = [0u8; 32];
        out.copy_from_slice(&digest);
        PairTokenHash(out)
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
    db: Mutex<Connection>,
    now: fn() -> DateTime<Utc>,
}

fn default_now() -> DateTime<Utc> {
    Utc::now()
}

fn device_id_from_pubkey(pk: &[u8; 32]) -> String {
    let digest = Sha256::digest(pk);
    let mut s = String::with_capacity(16);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for b in &digest[..8] {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0x0F) as usize] as char);
    }
    s
}

impl Pairings {
    /// New registry over `db`. Caller is responsible for migrations (`open_db`).
    pub fn new(db: Connection) -> Self {
        Self {
            db: Mutex::new(db),
            now: default_now,
        }
    }

    /// Construct with an injectable clock — used by tests to drive TTL boundaries.
    pub fn new_with_clock(db: Connection, now: fn() -> DateTime<Utc>) -> Self {
        Self {
            db: Mutex::new(db),
            now,
        }
    }

    /// Issue a new pair token with `ttl` (default per spec: 10 min).
    /// Inserts a row into `pairings` keyed by the SHA-256 of the token.
    pub fn issue(&self, ttl: Duration) -> Result<PairToken, RedeemError> {
        let token = PairToken::random();
        let hash = token.hash();
        let now = (self.now)();
        let expires_at =
            now + chrono::Duration::from_std(ttl).unwrap_or(chrono::Duration::seconds(0));
        let db = self.db.lock().expect("pairings db poisoned");
        db.execute(
            "INSERT INTO pairings (token_hash, expires_at, used_at) VALUES (?1, ?2, NULL)",
            params![&hash.0[..], expires_at.to_rfc3339()],
        )
        .map_err(|e| RedeemError::Internal(e.to_string()))?;
        Ok(token)
    }

    /// Test-only: install a fake "now" used for TTL checks.
    pub fn set_clock(&mut self, now: fn() -> DateTime<Utc>) {
        self.now = now;
    }

    /// Redeem a pair token: validate, mark used, mint a capability token.
    pub fn redeem(
        &self,
        token: &PairToken,
        device_pubkey: &[u8; 32],
        device_name: &str,
        host: &HostKey,
        capabilities: &[Capability],
    ) -> Result<PairRedeemResponse, RedeemError> {
        // Validate the pubkey: explicit all-zero reject (degenerate identity
        // point), then library-level Ed25519 point validation.
        if device_pubkey.iter().all(|b| *b == 0) {
            return Err(RedeemError::InvalidPubkey);
        }
        if VerifyingKey::from_bytes(device_pubkey).is_err() {
            return Err(RedeemError::InvalidPubkey);
        }

        let hash = token.hash();
        let now = (self.now)();

        let db = self.db.lock().expect("pairings db poisoned");

        // Look up the pairing row.
        let row = db
            .query_row(
                "SELECT expires_at, used_at FROM pairings WHERE token_hash = ?1",
                params![&hash.0[..]],
                |row| {
                    let expires_at: String = row.get(0)?;
                    let used_at: Option<String> = row.get(1)?;
                    Ok((expires_at, used_at))
                },
            )
            .optional()
            .map_err(|e| RedeemError::Internal(e.to_string()))?;

        let (expires_at_str, used_at) = match row {
            Some(r) => r,
            None => return Err(RedeemError::TokenUnknown),
        };

        if used_at.is_some() {
            return Err(RedeemError::TokenAlreadyUsed);
        }

        let expires_at = DateTime::parse_from_rfc3339(&expires_at_str)
            .map_err(|e| RedeemError::Internal(e.to_string()))?
            .with_timezone(&Utc);
        if now >= expires_at {
            return Err(RedeemError::TokenExpired);
        }

        let device_id = device_id_from_pubkey(device_pubkey);

        // Insert / replace device row first so the FK on pairings.used_by_device_id
        // is satisfied when we mark the pairing used.
        let perms_json = serde_json::to_string(capabilities)
            .map_err(|e| RedeemError::Internal(e.to_string()))?;
        db.execute(
            "INSERT OR REPLACE INTO devices \
             (id, name, public_key, paired_at, last_seen, permissions_json, revoked_at) \
             VALUES (?1, ?2, ?3, ?4, NULL, ?5, NULL)",
            params![
                &device_id,
                device_name,
                &device_pubkey[..],
                now.to_rfc3339(),
                perms_json,
            ],
        )
        .map_err(|e| RedeemError::Internal(e.to_string()))?;

        // Mark used.
        db.execute(
            "UPDATE pairings SET used_at = ?1, used_by_device_id = ?2 WHERE token_hash = ?3",
            params![now.to_rfc3339(), &device_id, &hash.0[..]],
        )
        .map_err(|e| RedeemError::Internal(e.to_string()))?;

        drop(db);

        let cap_token = CapabilityToken::issue(
            host,
            *device_pubkey,
            device_id.clone(),
            capabilities.to_vec(),
            Duration::from_secs(30 * 24 * 3600),
        );

        Ok(PairRedeemResponse {
            device_id,
            capabilities: capabilities.to_vec(),
            capability_token: cap_token,
            host_pubkey: host.pubkey(),
        })
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
