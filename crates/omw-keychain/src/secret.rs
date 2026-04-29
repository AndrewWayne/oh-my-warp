//! Opaque secret wrapper.
//!
//! `Secret` exists to make accidental leakage hard: it has no `Display` impl
//! (a `static_assertions::assert_not_impl_any!` in the test suite enforces
//! this at compile time), its `Debug` writes a fixed redaction marker, and
//! the value is zeroed on drop.
//!
//! ## Why `zeroize` and not a hand-rolled loop
//!
//! A naive `for byte in self.inner.as_bytes_mut() { *byte = 0; }` is
//! dead-store-eliminable: the compiler can prove the writes are never read
//! and remove them. `zeroize::Zeroize` performs volatile writes that the
//! optimizer is required to preserve, which is the only way to make the
//! cleanup observable in release builds.

use zeroize::Zeroize;

/// An opaque, zero-on-drop secret. Construct with [`Secret::new`] and read
/// with [`Secret::expose`]. Has no `Display` impl by design.
pub struct Secret {
    inner: String,
}

impl Secret {
    /// Wrap a `String` as a `Secret`. Takes ownership so the caller cannot
    /// retain a non-zeroizing copy.
    pub fn new(value: String) -> Self {
        Self { inner: value }
    }

    /// Borrow the underlying value. Callers should not clone or persist the
    /// returned `&str` longer than necessary.
    pub fn expose(&self) -> &str {
        &self.inner
    }
}

// Hand-rolled to emit a fixed marker. The literal "Secret" is intentionally
// omitted: the test suite sweeps any 4-char window of the wrapped value
// against rendered output, and a 4-char window like "ecre" inside the type
// name could collide with a sentinel ending in `-secret`.
impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("<redacted>")
    }
}

impl Drop for Secret {
    fn drop(&mut self) {
        self.inner.zeroize();
    }
}
