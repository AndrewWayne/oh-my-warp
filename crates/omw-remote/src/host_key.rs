//! Host pairing key (Ed25519). See `specs/byorc-protocol.md` §3.1.
//!
//! In production the secret half lives in the OS keychain via `omw-keychain`.
//! For Phase D tests we use a temp-file-backed loader; the keychain wiring lands later.

use std::io;
use std::path::Path;

/// Long-lived Ed25519 keypair used to sign capability tokens.
pub struct HostKey {
    // ed25519_dalek::SigningKey
}

impl HostKey {
    /// Generate a fresh host key from the OS RNG.
    pub fn generate() -> Self {
        unimplemented!("HostKey::generate")
    }

    /// Load a host key from `path`, generating + saving one if `path` does not exist.
    pub fn load_or_create(_path: &Path) -> io::Result<Self> {
        unimplemented!("HostKey::load_or_create")
    }

    /// Persist the secret half to `path` (Phase D: plain file; production: keychain).
    pub fn save(&self, _path: &Path) -> io::Result<()> {
        unimplemented!("HostKey::save")
    }

    /// Public key (Ed25519, 32 bytes).
    pub fn pubkey(&self) -> [u8; 32] {
        unimplemented!("HostKey::pubkey")
    }

    /// Sign `msg` with the host secret, returning the 64-byte Ed25519 signature.
    pub fn sign(&self, _msg: &[u8]) -> [u8; 64] {
        unimplemented!("HostKey::sign")
    }
}
