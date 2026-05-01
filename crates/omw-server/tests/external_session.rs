//! C.x — `SessionRegistry::register_external` lifecycle tests.
//!
//! Drives the registry through its public surface only, registering
//! externally-owned sessions whose I/O channels are owned by the test rather
//! than spawned by the registry. Mirrors the style of
//! `registry_lifecycle.rs`: `#[tokio::test]` async tests, no extra deps,
//! independent registries per test, `tokio::time::timeout` around channel
//! recvs to avoid hangs.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use tokio::sync::{broadcast, mpsc};
use tokio::time::timeout;

use omw_server::registry::ExternalSessionSpec;
use omw_server::{Error, SessionRegistry, SessionSpec};

use omw_pty::PtySize;

/// Construct a fresh `ExternalSessionSpec` plus the test-owned ends of its
/// I/O channels and the kill-flag. The caller drops/holds these as needed.
fn make_external_spec(
    name: &str,
) -> (
    ExternalSessionSpec,
    mpsc::Receiver<Vec<u8>>,
    broadcast::Sender<Bytes>,
    Arc<AtomicBool>,
) {
    let (input_tx, input_rx) = mpsc::channel::<Vec<u8>>(16);
    let (output_tx, _output_rx0) = broadcast::channel::<Bytes>(64);
    let killed = Arc::new(AtomicBool::new(false));
    let killed_for_closure = killed.clone();
    let kill: Box<dyn Fn() + Send + Sync> = Box::new(move || {
        killed_for_closure.store(true, Ordering::SeqCst);
    });
    let spec = ExternalSessionSpec {
        name: name.to_string(),
        input_tx,
        output_tx: output_tx.clone(),
        kill,
        initial_size: PtySize {
            cols: 80,
            rows: 24,
        },
    };
    (spec, input_rx, output_tx, killed)
}

#[tokio::test]
async fn external_session_register_and_list() {
    let registry = SessionRegistry::new();
    let (spec, _input_rx, _output_tx, _killed) = make_external_spec("external-one");

    let id = registry
        .register_external(spec)
        .await
        .expect("register_external should succeed");

    let listed = registry.list();
    assert_eq!(listed.len(), 1, "list() must contain exactly one entry");
    let entry = listed
        .iter()
        .find(|m| m.id == id)
        .expect("freshly-registered external session must appear in list()");
    assert_eq!(entry.name, "external-one");
    assert!(entry.alive, "external session must be alive after register");

    let got = registry
        .get(id)
        .expect("get(known id) must return Some(meta)");
    assert_eq!(got.id, id);
    assert_eq!(got.name, "external-one");
    assert!(got.alive, "get() must report alive == true");
}

#[tokio::test]
async fn external_session_output_via_subscribe() {
    let registry = SessionRegistry::new();
    let (spec, _input_rx, output_tx, _killed) = make_external_spec("external-output");

    let id = registry
        .register_external(spec)
        .await
        .expect("register_external should succeed");

    let mut rx = registry
        .subscribe(id)
        .expect("subscribe(known id) must return Some(receiver)");

    output_tx
        .send(Bytes::from_static(b"hello\n"))
        .expect("test output_tx send should succeed");

    let chunk = timeout(Duration::from_secs(2), rx.recv())
        .await
        .expect("subscriber must receive within 2s")
        .expect("subscriber recv must yield Ok");
    assert_eq!(
        &chunk[..],
        b"hello\n",
        "subscriber must receive exactly the bytes that were broadcast"
    );
}

#[tokio::test]
async fn external_session_multiple_subscribers() {
    let registry = SessionRegistry::new();
    let (spec, _input_rx, output_tx, _killed) = make_external_spec("external-fanout");

    let id = registry
        .register_external(spec)
        .await
        .expect("register_external should succeed");

    let mut rx_a = registry
        .subscribe(id)
        .expect("first subscribe must return Some");
    let mut rx_b = registry
        .subscribe(id)
        .expect("second subscribe must return Some");

    output_tx
        .send(Bytes::from_static(b"fanout-payload"))
        .expect("test output_tx send should succeed");

    let got_a = timeout(Duration::from_secs(2), rx_a.recv())
        .await
        .expect("subscriber A must receive within 2s")
        .expect("subscriber A recv must yield Ok");
    let got_b = timeout(Duration::from_secs(2), rx_b.recv())
        .await
        .expect("subscriber B must receive within 2s")
        .expect("subscriber B recv must yield Ok");

    assert_eq!(&got_a[..], b"fanout-payload");
    assert_eq!(&got_b[..], b"fanout-payload");
}

#[tokio::test]
async fn external_session_write_input_forwards_to_mpsc() {
    let registry = SessionRegistry::new();
    let (spec, mut input_rx, _output_tx, _killed) = make_external_spec("external-input");

    let id = registry
        .register_external(spec)
        .await
        .expect("register_external should succeed");

    registry
        .write_input(id, b"abc")
        .await
        .expect("write_input on a healthy mpsc must succeed");

    let received = timeout(Duration::from_secs(2), input_rx.recv())
        .await
        .expect("input_rx must receive within 2s")
        .expect("input_rx recv must yield Some(_) (channel must still be open)");
    assert_eq!(
        received,
        b"abc".to_vec(),
        "test-side input_rx must observe exactly the bytes written"
    );
}

#[tokio::test]
async fn external_session_write_input_to_closed_mpsc_returns_io_error() {
    let registry = SessionRegistry::new();
    let (spec, input_rx, _output_tx, _killed) = make_external_spec("external-closed");

    let id = registry
        .register_external(spec)
        .await
        .expect("register_external should succeed");

    // Drop the test-owned receiver BEFORE write_input fires, so the registry's
    // mpsc::Sender::send() fails with SendError -> Error::Io.
    drop(input_rx);

    let err = registry
        .write_input(id, b"after-drop")
        .await
        .expect_err("write_input must fail when the mpsc receiver has been dropped");
    match err {
        Error::Io(_) => {}
        other => panic!(
            "expected Error::Io after mpsc receiver dropped, got: {other:?}"
        ),
    }
}

#[tokio::test]
async fn external_session_kill_invokes_closure_and_removes() {
    let registry = SessionRegistry::new();
    let (spec, _input_rx, _output_tx, killed) = make_external_spec("external-kill");

    let id = registry
        .register_external(spec)
        .await
        .expect("register_external should succeed");

    assert!(
        !killed.load(Ordering::SeqCst),
        "kill flag must be false before kill()"
    );

    registry
        .kill(id)
        .await
        .expect("kill(known id) must succeed");

    assert!(
        killed.load(Ordering::SeqCst),
        "kill closure must have flipped the AtomicBool"
    );

    assert!(
        registry.list().is_empty(),
        "list() must be empty after kill"
    );
    assert!(
        registry.get(id).is_none(),
        "get(killed id) must return None"
    );
}

/// Regression: registering an externally-owned session must not break the
/// existing owned-Pty path. After registering one of each, `list()` must
/// surface both with the right names and alive flags.
#[tokio::test]
async fn external_and_owned_pty_coexist_in_registry() {
    let registry = SessionRegistry::new();

    // 1. Register an external session first (no spawn — purely test-owned).
    let (ext_spec, _input_rx, _output_tx, _killed) = make_external_spec("external-coexist");
    let ext_id = registry
        .register_external(ext_spec)
        .await
        .expect("register_external should succeed");

    // 2. Register an owned-Pty session via the existing register() API.
    let owned_spec = if cfg!(windows) {
        SessionSpec {
            name: "owned-coexist".to_string(),
            command: "cmd".to_string(),
            args: vec!["/c".to_string(), "echo hi".to_string()],
            cwd: None,
            env: None,
            cols: Some(80),
            rows: Some(24),
        }
    } else {
        SessionSpec {
            name: "owned-coexist".to_string(),
            command: "sh".to_string(),
            args: vec!["-c".to_string(), "echo hi".to_string()],
            cwd: None,
            env: None,
            cols: Some(80),
            rows: Some(24),
        }
    };
    let owned_id = registry
        .register(owned_spec)
        .await
        .expect("register (owned-Pty) should succeed");

    let listed = registry.list();
    assert_eq!(
        listed.len(),
        2,
        "list() must contain both external + owned-Pty entries; got {}",
        listed.len()
    );

    let ext_entry = listed
        .iter()
        .find(|m| m.id == ext_id)
        .expect("external session must appear in list()");
    assert_eq!(ext_entry.name, "external-coexist");
    assert!(
        ext_entry.alive,
        "external session must be alive immediately after register"
    );

    let owned_entry = listed
        .iter()
        .find(|m| m.id == owned_id)
        .expect("owned-Pty session must appear in list()");
    assert_eq!(owned_entry.name, "owned-coexist");
    // The owned-Pty child may exit between register() and list() since it's
    // a `echo hi` quick-exit. We only assert it was registered and surfaced;
    // we do NOT assert alive==true here to avoid a race with reaping.
}

/// Per-ID routing: with two external sessions A and B registered concurrently,
/// `subscribe`, `write_input`, and `kill` must each route to the correct
/// session by id. An implementation that ignores `id` (e.g. tracks "the one
/// external session" globally) would fail this test.
#[tokio::test]
async fn external_session_per_id_routing() {
    let registry = SessionRegistry::new();

    let (spec_a, mut input_rx_a, output_tx_a, killed_a) = make_external_spec("external-A");
    let (spec_b, mut input_rx_b, output_tx_b, killed_b) = make_external_spec("external-B");

    let id_a = registry
        .register_external(spec_a)
        .await
        .expect("register_external(A) should succeed");
    let id_b = registry
        .register_external(spec_b)
        .await
        .expect("register_external(B) should succeed");
    assert_ne!(id_a, id_b, "registered ids must be unique");

    // --- subscribe routing: A's subscriber must see A's broadcasts only ---
    let mut rx_a = registry
        .subscribe(id_a)
        .expect("subscribe(id_a) must return Some");
    let mut rx_b = registry
        .subscribe(id_b)
        .expect("subscribe(id_b) must return Some");

    // Broadcast on A's tx — only rx_a must observe it.
    output_tx_a
        .send(Bytes::from_static(b"from-A"))
        .expect("test output_tx_a send should succeed");

    let chunk_a = timeout(Duration::from_secs(2), rx_a.recv())
        .await
        .expect("rx_a must receive within 2s")
        .expect("rx_a recv must yield Ok");
    assert_eq!(
        &chunk_a[..],
        b"from-A",
        "rx_a must receive A's broadcast bytes"
    );

    // rx_b must NOT have received A's payload — assert via a short timeout.
    match timeout(Duration::from_millis(200), rx_b.recv()).await {
        Err(_elapsed) => { /* expected: nothing for B yet */ }
        Ok(other) => panic!(
            "rx_b must not receive A's broadcast; got {other:?}"
        ),
    }

    // Now broadcast on B's tx — only rx_b must observe it.
    output_tx_b
        .send(Bytes::from_static(b"from-B"))
        .expect("test output_tx_b send should succeed");
    let chunk_b = timeout(Duration::from_secs(2), rx_b.recv())
        .await
        .expect("rx_b must receive within 2s")
        .expect("rx_b recv must yield Ok");
    assert_eq!(
        &chunk_b[..],
        b"from-B",
        "rx_b must receive B's broadcast bytes"
    );
    // And rx_a must not see B's payload.
    match timeout(Duration::from_millis(200), rx_a.recv()).await {
        Err(_elapsed) => { /* expected */ }
        Ok(other) => panic!(
            "rx_a must not receive B's broadcast; got {other:?}"
        ),
    }

    // --- write_input routing: write to A only; A's mpsc must see it; B's must not ---
    registry
        .write_input(id_a, b"input-for-A")
        .await
        .expect("write_input(id_a) must succeed");
    let got_a = timeout(Duration::from_secs(2), input_rx_a.recv())
        .await
        .expect("input_rx_a must receive within 2s")
        .expect("input_rx_a recv must yield Some");
    assert_eq!(
        got_a,
        b"input-for-A".to_vec(),
        "A's mpsc receiver must see exactly the bytes written to A"
    );
    match timeout(Duration::from_millis(200), input_rx_b.recv()).await {
        Err(_elapsed) => { /* expected: B got nothing */ }
        Ok(other) => panic!(
            "input_rx_b must not receive A's input; got {other:?}"
        ),
    }

    // --- kill routing: kill A only; A's closure fires, B's does not ---
    assert!(
        !killed_a.load(Ordering::SeqCst),
        "A's kill flag must be false before kill(id_a)"
    );
    assert!(
        !killed_b.load(Ordering::SeqCst),
        "B's kill flag must be false before kill(id_a)"
    );
    registry
        .kill(id_a)
        .await
        .expect("kill(id_a) must succeed");
    assert!(
        killed_a.load(Ordering::SeqCst),
        "A's kill closure must have fired"
    );
    assert!(
        !killed_b.load(Ordering::SeqCst),
        "B's kill closure must NOT have fired when only A was killed"
    );

    // After kill(id_a): list contains only B; get(id_a) is None; get(id_b) is Some.
    let listed = registry.list();
    assert_eq!(
        listed.len(),
        1,
        "after kill(id_a), list() must contain only B; got {}",
        listed.len()
    );
    assert_eq!(
        listed[0].id, id_b,
        "the sole remaining entry must be B"
    );
    assert!(
        registry.get(id_a).is_none(),
        "get(id_a) must be None after A is killed"
    );
    assert!(
        registry.get(id_b).is_some(),
        "get(id_b) must still be Some — B was untouched"
    );
}

/// Regression: an externally-registered session must not poison the owned-Pty
/// path. After registering an external session AND an owned-Pty session, the
/// owned session's `subscribe` must still yield output and `kill` must still
/// remove it from the registry.
#[tokio::test]
async fn external_session_does_not_break_owned_path() {
    let registry = SessionRegistry::new();

    // Register an external session FIRST so any "single external slot"
    // implementation bug would already be in effect when we exercise the
    // owned path below.
    let (ext_spec, _input_rx, _output_tx, _killed) = make_external_spec("external-poison");
    let _ext_id = registry
        .register_external(ext_spec)
        .await
        .expect("register_external should succeed");

    // Register an owned-Pty session whose child waits ~1s BEFORE emitting any
    // output, then prints a chunk and exits. The pre-output delay guarantees
    // our `subscribe()` call below registers a receiver before the output_pump
    // task fires its first `broadcast::send` — `tokio::sync::broadcast` does
    // not replay past messages to late subscribers, so a quick-exit command
    // (e.g. plain `echo hi`) could race the pump and yield zero chunks even
    // on a correct registry impl. The delay closes that race.
    let owned_spec = if cfg!(windows) {
        SessionSpec {
            name: "owned-after-external".to_string(),
            command: "cmd".to_string(),
            // `ping 127.0.0.1 -n 2 >nul` sends two echoes 1s apart and
            // produces no stdout, then `echo delayed` prints the chunk.
            args: vec![
                "/c".to_string(),
                "ping 127.0.0.1 -n 2 >nul & echo delayed".to_string(),
            ],
            cwd: None,
            env: None,
            cols: Some(80),
            rows: Some(24),
        }
    } else {
        SessionSpec {
            name: "owned-after-external".to_string(),
            command: "sh".to_string(),
            args: vec!["-c".to_string(), "sleep 0.7; echo delayed".to_string()],
            cwd: None,
            env: None,
            cols: Some(80),
            rows: Some(24),
        }
    };
    let owned_id = registry
        .register(owned_spec)
        .await
        .expect("register(owned-Pty) must succeed even after register_external");

    // subscribe(owned_id) must return Some, and — because the child waits
    // ~1s before emitting — our subscription is reliably installed before
    // the pump's first send. We must receive the delayed chunk within 3s
    // (1s pre-output delay + scheduling slack), proving the external path
    // did not poison the owned broadcast wiring.
    let mut owned_rx = registry
        .subscribe(owned_id)
        .expect("subscribe(owned_id) must return Some after external session is registered");
    let chunk = timeout(Duration::from_secs(3), owned_rx.recv())
        .await
        .expect("owned subscriber must receive the delayed chunk within 3s after subscribe was set up before the child emitted");
    let _ = chunk.expect("owned subscriber recv must yield Ok (broadcast must not be closed before first emit)");

    // kill(owned_id) must succeed and remove it from list().
    registry
        .kill(owned_id)
        .await
        .expect("kill(owned_id) must succeed even after register_external");
    assert!(
        registry.get(owned_id).is_none(),
        "owned session must be gone from get() after kill"
    );
    let listed = registry.list();
    assert!(
        listed.iter().all(|m| m.id != owned_id),
        "owned session must be gone from list() after kill"
    );
}
