//! Windows Credential Manager persistence contract.

#![cfg(windows)]

use std::time::{SystemTime, UNIX_EPOCH};

use assert_cmd::Command as AssertCommand;
use omw_config::KeyRef;

struct Cleanup(KeyRef);

impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = omw_keychain::delete(&self.0);
    }
}

fn helper() -> AssertCommand {
    let mut cmd = AssertCommand::cargo_bin("omw-keychain-helper")
        .expect("omw-keychain-helper binary should be built by cargo test");
    cmd.env_clear().env("OMW_KEYCHAIN_BACKEND", "os");
    if let Ok(path) = std::env::var("PATH") {
        cmd.env("PATH", path);
    }
    cmd
}

#[test]
fn credential_persists_across_helper_processes_and_deletes_cleanly() {
    // TODO(rust-2024): wrap in unsafe once the workspace changes edition.
    std::env::set_var("OMW_KEYCHAIN_BACKEND", "os");
    assert_eq!(
        omw_keychain::current_backend_kind(),
        omw_keychain::BackendKind::Os
    );

    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before Unix epoch")
        .as_nanos();
    let key_ref: KeyRef = format!("keychain:omw/windows-test/{}-{nonce}", std::process::id())
        .parse()
        .expect("valid test key reference");
    let key_ref_arg = key_ref.to_string();
    let sentinel = format!("OMW_WINDOWS_SENTINEL_{}_{nonce}_雪", std::process::id());
    let _cleanup = Cleanup(key_ref.clone());

    omw_keychain::set(&key_ref, &sentinel).expect("store sentinel in Windows Credential Manager");

    // Each invocation is a fresh process, so two successful reads prove the
    // credential is not process-local memory. The secret never appears in
    // argv, stderr, or assertion messages.
    for _ in 0..2 {
        let output = helper()
            .args(["get", &key_ref_arg])
            .output()
            .expect("run helper");
        assert!(output.status.success(), "helper get should succeed");
        let mut expected = sentinel.as_bytes().to_vec();
        expected.push(b'\n');
        assert!(
            output.stdout == expected,
            "helper returned unexpected secret bytes"
        );
        assert!(
            output.stderr.is_empty(),
            "helper success must not write stderr"
        );
    }

    omw_keychain::delete(&key_ref).expect("delete sentinel from Windows Credential Manager");

    let output = helper()
        .args(["get", &key_ref_arg])
        .output()
        .expect("run helper after delete");
    assert_eq!(output.status.code(), Some(1));
    assert!(
        output.stdout.is_empty(),
        "deleted secret must not reach stdout"
    );
    assert!(
        output.stderr == b"not found\n",
        "helper get after delete returned unexpected stderr"
    );
}
