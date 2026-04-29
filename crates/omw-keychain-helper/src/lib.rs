//! `omw-keychain-helper` — library entrypoint.
//!
//! `pub fn run` implements the CLI's behavior over generic `Write` sinks so
//! that in-process tests can capture stdout/stderr into buffers. The binary
//! in `src/main.rs` is a thin shim around this function.
//!
//! ## Threat-model invariant I-1
//!
//! Secret material is written to `stdout` only on the success path — a
//! single `Secret::expose()` followed by `\n`. `stderr` MUST NOT receive
//! any value-derived data; in particular, error formatting never echoes
//! the parsed `KeyRef` content back. All `KeychainError` variants render
//! through their (redacted) `Display` impl, which omits the wrapped
//! `reason` / `source` fields.

use std::collections::HashMap;
use std::io::Write;

use omw_config::KeyRef;
use omw_keychain::KeychainError;

const USAGE: &str = "usage: omw-keychain-helper get <key_ref>";

/// Execute the helper. `args` is argv WITHOUT argv[0].
///
/// Exit codes:
/// - `0` — success; secret + `\n` written to `stdout`.
/// - `1` — `KeychainError::NotFound`.
/// - `2` — bad input (no subcommand, unknown subcommand, missing operand,
///   malformed `key_ref`).
/// - `3` — `KeychainError::BackendUnavailable` / `KeychainError::Os`.
pub fn run(
    args: &[String],
    envs: &HashMap<String, String>,
    stdout: &mut dyn Write,
    stderr: &mut dyn Write,
) -> i32 {
    // Mirror `OMW_KEYCHAIN_BACKEND` from the supplied envs map onto the
    // process env. `omw-keychain`'s backend resolver reads `std::env`
    // directly via `OnceLock`, so the in-process tests need this bridge to
    // honor an `envs` map that differs from the live process env.
    if let Some(v) = envs.get("OMW_KEYCHAIN_BACKEND") {
        std::env::set_var("OMW_KEYCHAIN_BACKEND", v);
    }

    if args.is_empty() {
        let _ = writeln!(stderr, "{USAGE}");
        return 2;
    }

    match args[0].as_str() {
        "--help" | "-h" => {
            let _ = writeln!(stdout, "{USAGE}");
            0
        }
        "get" => run_get(&args[1..], stdout, stderr),
        _ => {
            let _ = writeln!(stderr, "unknown command; {USAGE}");
            2
        }
    }
}

fn run_get(rest: &[String], stdout: &mut dyn Write, stderr: &mut dyn Write) -> i32 {
    if rest.is_empty() {
        let _ = writeln!(stderr, "{USAGE}");
        return 2;
    }

    // We deliberately do NOT echo the input back in the error message:
    // a misuse where a real secret was passed as the key_ref must not be
    // amplified by reflecting it into stderr (cli.rs t7).
    let key_ref: KeyRef = match rest[0].parse() {
        Ok(k) => k,
        Err(_) => {
            let _ = writeln!(stderr, "invalid key_ref");
            return 2;
        }
    };

    match omw_keychain::get(&key_ref) {
        Ok(secret) => {
            let _ = stdout.write_all(secret.expose().as_bytes());
            let _ = stdout.write_all(b"\n");
            0
        }
        Err(KeychainError::NotFound) => {
            let _ = writeln!(stderr, "not found");
            1
        }
        Err(KeychainError::BackendUnavailable { .. }) => {
            let _ = writeln!(stderr, "keychain backend unavailable");
            3
        }
        Err(KeychainError::Os { .. }) => {
            let _ = writeln!(stderr, "OS keychain error");
            3
        }
    }
}
