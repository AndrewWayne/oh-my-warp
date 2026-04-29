//! `KeyRef` — typed reference to a credential, never the credential itself.
//!
//! Threat model invariant **I-1** (`specs/threat-model.md`): no plaintext keys
//! on disk; configuration references keychain entries by name only. We enforce
//! this at the type level — `KeyRef` is constructable only from a string with a
//! recognised resolver scheme, so plaintext keys fail at deserialize time.

use std::str::FromStr;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyRef {
    Keychain { name: String },
}

#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum KeyRefParseError {
    #[error("missing scheme; expected `keychain:<name>`")]
    MissingScheme,

    #[error("unsupported scheme `{0}:` (only `keychain:` is supported in v0.1)")]
    UnsupportedScheme(String),

    #[error("empty name after `keychain:`")]
    EmptyName,
}

impl FromStr for KeyRef {
    type Err = KeyRefParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (scheme, rest) = s.split_once(':').ok_or(KeyRefParseError::MissingScheme)?;
        match scheme {
            "keychain" => {
                if rest.is_empty() {
                    Err(KeyRefParseError::EmptyName)
                } else {
                    Ok(KeyRef::Keychain {
                        name: rest.to_string(),
                    })
                }
            }
            other => Err(KeyRefParseError::UnsupportedScheme(other.to_string())),
        }
    }
}

impl TryFrom<String> for KeyRef {
    type Error = KeyRefParseError;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        s.parse()
    }
}

impl std::fmt::Display for KeyRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KeyRef::Keychain { name } => write!(f, "keychain:{name}"),
        }
    }
}

impl serde::Serialize for KeyRef {
    fn serialize<S: serde::Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        ser.collect_str(self)
    }
}

impl<'de> serde::Deserialize<'de> for KeyRef {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_keychain_uri() {
        let k: KeyRef = "keychain:omw/openai".parse().unwrap();
        assert_eq!(
            k,
            KeyRef::Keychain {
                name: "omw/openai".into()
            }
        );
    }

    #[test]
    fn deserializes_from_toml_string() {
        #[derive(serde::Deserialize)]
        struct Wrap {
            value: KeyRef,
        }
        let w: Wrap = toml::from_str(r#"value = "keychain:omw/anthropic""#).unwrap();
        assert!(matches!(w.value, KeyRef::Keychain { .. }));
    }

    #[test]
    fn rejects_plaintext_openai_key_i1() {
        // Threat model I-1: structural rejection of plaintext keys at deserialize
        // time. The CI grep guard is defense-in-depth, not the primary defense.
        let err = "sk-test123".parse::<KeyRef>().unwrap_err();
        assert_eq!(err, KeyRefParseError::MissingScheme);
    }

    #[test]
    fn rejects_plaintext_anthropic_key_i1() {
        let err = "sk-ant-test".parse::<KeyRef>().unwrap_err();
        assert_eq!(err, KeyRefParseError::MissingScheme);
    }

    #[test]
    fn rejects_plaintext_with_colon_in_value() {
        // A plaintext key that happens to contain ':' must still fail —
        // it would land in UnsupportedScheme, which is also a hard reject.
        let err = "abc:def".parse::<KeyRef>().unwrap_err();
        assert_eq!(err, KeyRefParseError::UnsupportedScheme("abc".into()));
    }

    #[test]
    fn rejects_empty_name() {
        let err = "keychain:".parse::<KeyRef>().unwrap_err();
        assert_eq!(err, KeyRefParseError::EmptyName);
    }

    #[test]
    fn rejects_empty_string() {
        let err = "".parse::<KeyRef>().unwrap_err();
        assert_eq!(err, KeyRefParseError::MissingScheme);
    }

    #[test]
    fn rejects_env_scheme_reserved_for_later() {
        // env: is reserved for v0.2+; explicitly rejected in v0.1 so that adding
        // it later is additive and visible.
        let err = "env:OPENAI_API_KEY".parse::<KeyRef>().unwrap_err();
        assert_eq!(err, KeyRefParseError::UnsupportedScheme("env".into()));
    }

    #[test]
    fn rejects_cmd_scheme_reserved_for_later() {
        let err = "cmd:op read op://omw/openai".parse::<KeyRef>().unwrap_err();
        assert_eq!(err, KeyRefParseError::UnsupportedScheme("cmd".into()));
    }

    #[test]
    fn round_trips_via_display() {
        let k = KeyRef::Keychain {
            name: "omw/azure".into(),
        };
        let s = k.to_string();
        assert_eq!(s, "keychain:omw/azure");
        let parsed: KeyRef = s.parse().unwrap();
        assert_eq!(parsed, k);
    }
}
