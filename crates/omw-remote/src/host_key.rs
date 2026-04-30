//! Host pairing key (Ed25519). See `specs/byorc-protocol.md` §3.1.
//!
//! In production the secret half lives in the OS keychain via `omw-keychain`.
//! For Phase D tests we use a temp-file-backed loader; the keychain wiring lands later.

use std::fs;
use std::io;
use std::path::Path;

use ed25519_dalek::{Signer as _, SigningKey};
use rand::rngs::OsRng;

const MAGIC: &[u8] = b"OMW-HOSTKEY\0\x01";
const FILE_LEN: usize = MAGIC.len() + 32;

/// Long-lived Ed25519 keypair used to sign capability tokens.
pub struct HostKey {
    signing: SigningKey,
}

impl HostKey {
    /// Generate a fresh host key from the OS RNG.
    pub fn generate() -> Self {
        let signing = SigningKey::generate(&mut OsRng);
        Self { signing }
    }

    /// Load a host key from `path`, generating + saving one if `path` does not exist.
    pub fn load_or_create(path: &Path) -> io::Result<Self> {
        if path.exists() {
            let bytes = fs::read(path)?;
            if bytes.len() != FILE_LEN || &bytes[..MAGIC.len()] != MAGIC {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "host key file: bad magic or length",
                ));
            }
            let mut seed = [0u8; 32];
            seed.copy_from_slice(&bytes[MAGIC.len()..]);
            let signing = SigningKey::from_bytes(&seed);
            Ok(Self { signing })
        } else {
            let key = Self::generate();
            key.save(path)?;
            Ok(key)
        }
    }

    /// Persist the secret half to `path` (Phase D: plain file; production: keychain).
    pub fn save(&self, path: &Path) -> io::Result<()> {
        let mut buf = Vec::with_capacity(FILE_LEN);
        buf.extend_from_slice(MAGIC);
        buf.extend_from_slice(&self.signing.to_bytes());
        fs::write(path, &buf)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let perms = fs::Permissions::from_mode(0o600);
            fs::set_permissions(path, perms)?;
        }
        Ok(())
    }

    /// Public key (Ed25519, 32 bytes).
    pub fn pubkey(&self) -> [u8; 32] {
        self.signing.verifying_key().to_bytes()
    }

    /// Sign `msg` with the host secret, returning the 64-byte Ed25519 signature.
    pub fn sign(&self, msg: &[u8]) -> [u8; 64] {
        self.signing.sign(msg).to_bytes()
    }
}
