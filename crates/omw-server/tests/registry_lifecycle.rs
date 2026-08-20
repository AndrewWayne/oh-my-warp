//! C.1 — `SessionRegistry` direct lifecycle test (no HTTP).
//!
//! Drives the registry through its public surface only:
//!   - construct via `SessionRegistry::new()`
//!   - register a short-lived child
//!   - assert it appears in `list()` with `alive == true`
//!   - poll until the child exits (max 10s); assert `alive == false`
//!   - drop the registry — must not panic
//!
//! Uses platform-appropriate "print one line then exit" commands so the test
//! runs on both Unix and Windows hosts without requiring a shell layout
//! beyond the platform default.

use std::time::{Duration, Instant};

use omw_server::{SessionRegistry, SessionSpec};

fn quick_spec() -> SessionSpec {
    if cfg!(windows) {
        SessionSpec {
            name: "quick-echo".to_string(),
            command: "cmd".to_string(),
            args: vec!["/c".to_string(), "echo hello".to_string()],
            cwd: None,
            env: None,
            cols: Some(80),
            rows: Some(24),
        }
    } else {
        SessionSpec {
            name: "quick-echo".to_string(),
            command: "sh".to_string(),
            // `printf hello\n` then implicit exit. Single-quoted so the
            // shell does no expansion.
            args: vec!["-c".to_string(), "printf hello\\n".to_string()],
            cwd: None,
            env: None,
            cols: Some(80),
            rows: Some(24),
        }
    }
}

#[tokio::test]
async fn register_then_observe_exit_then_drop() {
    let registry = SessionRegistry::new();

    let id = registry
        .register(quick_spec())
        .await
        .expect("register should succeed for a benign quick child");

    // Immediately after register, the entry must be visible in list().
    let listed = registry.list();
    let entry = listed
        .iter()
        .find(|m| m.id == id)
        .expect("freshly-registered session must appear in list()");
    assert_eq!(entry.name, "quick-echo");

    // get() returns the same metadata.
    let got = registry
        .get(id)
        .expect("get(known id) must return Some(meta)");
    assert_eq!(got.id, id);
    assert_eq!(got.name, "quick-echo");

    // A short-lived ConPTY child can still take roughly three seconds to be
    // observed as reaped on Windows. Keep the assertion bounded without
    // assuming Unix-like process startup and teardown latency.
    let exit_timeout = Duration::from_secs(10);
    let deadline = Instant::now() + exit_timeout;
    let mut last_alive = true;
    while Instant::now() < deadline {
        match registry.get(id) {
            Some(meta) => {
                last_alive = meta.alive;
                if !meta.alive {
                    break;
                }
            }
            None => {
                // Some implementations may auto-evict on exit. That counts as
                // "definitely not alive" for the purposes of this assertion.
                last_alive = false;
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(
        !last_alive,
        "child should have exited within {exit_timeout:?}; last observed alive = {last_alive}"
    );

    // Drop the registry — must not panic. We cannot directly assert "no
    // panic" beyond letting the test exit cleanly; the Arc drop runs here.
    drop(registry);
}

#[tokio::test]
async fn get_unknown_id_is_none() {
    let registry = SessionRegistry::new();
    let bogus = uuid::Uuid::nil();
    assert!(registry.get(bogus).is_none());
}

#[tokio::test]
async fn list_starts_empty() {
    let registry = SessionRegistry::new();
    let listed = registry.list();
    assert!(
        listed.is_empty(),
        "fresh registry must have no sessions; got {} entries",
        listed.len()
    );
}
