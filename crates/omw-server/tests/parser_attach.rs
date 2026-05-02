//! Unit tests for the per-session vt100 parser plumbing.
//!
//! Verifies the contract documented on
//! [`SessionRegistry::record_output`] / [`SessionRegistry::subscribe_with_state`]:
//! - bytes pushed via `record_output` reach the parser AND the broadcast,
//! - `subscribe_with_state` returns an ANSI-serialized snapshot of the
//!   current screen plus a fresh broadcast receiver,
//! - the map mutex makes the snapshot/subscribe pair atomic with respect to
//!   producers, so concurrently-arriving bytes are delivered EITHER via the
//!   snapshot OR via the live receiver (never both, never neither).

use std::time::Duration;

use bytes::Bytes;
use tokio::sync::{broadcast, mpsc};

use omw_pty::PtySize;
use omw_server::{ExternalSessionSpec, SessionRegistry};

/// Build an external session entry with no real PTY behind it. Returns the
/// registry, the assigned id, and the broadcast Sender (kept so the test can
/// either push bytes through `record_output` or read `subscriber_count`).
async fn make_session(rows: u16, cols: u16) -> (std::sync::Arc<SessionRegistry>, uuid::Uuid) {
    let registry = SessionRegistry::new();
    let (input_tx, _input_rx) = mpsc::channel::<Vec<u8>>(8);
    let (output_tx, _output_rx0) = broadcast::channel::<Bytes>(64);
    let spec = ExternalSessionSpec {
        name: "test-pane".to_string(),
        input_tx,
        output_tx,
        kill: Box::new(|| {}),
        initial_size: PtySize { rows, cols },
    };
    let id = registry
        .register_external(spec)
        .await
        .expect("register_external");
    (registry, id)
}

/// Plan §5 step 5 test #1: bytes pushed via `record_output` reach the parser,
/// and the parser's serialized snapshot contains the printed text.
#[tokio::test]
async fn record_output_feeds_parser() {
    let (registry, id) = make_session(24, 80).await;

    // Push plain ASCII through record_output. The parser should consume it,
    // putting "hello" on row 0 starting at column 0.
    registry
        .record_output(id, Bytes::from_static(b"hello"))
        .expect("record_output");

    // Snapshot via subscribe_with_state. The serialized contents must contain
    // the literal substring "hello" — vt100 emits cell contents in row-major
    // order, so contiguous printable ASCII appears as-is between escape codes.
    let (snapshot, _rx) = registry
        .subscribe_with_state(id)
        .expect("session is registered");

    let snapshot_bytes: &[u8] = &snapshot;
    let needle = b"hello";
    assert!(
        snapshot_bytes
            .windows(needle.len())
            .any(|w| w == needle),
        "expected snapshot to contain {:?}, got {:?}",
        std::str::from_utf8(needle).unwrap(),
        String::from_utf8_lossy(snapshot_bytes),
    );
}

/// Plan §5 step 5 test #2: the snapshot from `subscribe_with_state` is a real
/// ANSI-serialized grid. We don't pin a specific byte sequence (vt100's exact
/// output format is an implementation detail), but we DO require:
/// - a non-empty payload,
/// - it contains a CSI cursor-position introducer (`\x1b[`),
/// - it contains the printable text we wrote,
/// - and it works as a "first frame" — the rebuilt cursor lands at the post-
///   write position by serializing and re-feeding into a fresh parser.
#[tokio::test]
async fn subscribe_with_state_returns_serialized_grid() {
    let (registry, id) = make_session(24, 80).await;

    registry
        .record_output(id, Bytes::from_static(b"abc\r\ndef"))
        .expect("record_output");

    let (snapshot, _rx) = registry
        .subscribe_with_state(id)
        .expect("session is registered");

    assert!(!snapshot.is_empty(), "snapshot must not be empty");
    assert!(
        snapshot.windows(2).any(|w| w == b"\x1b["),
        "snapshot must contain at least one CSI sequence"
    );

    // Round-trip: replay the snapshot into a fresh parser at the same size
    // and verify the rebuilt screen has the same printable content on
    // rows 0 and 1.
    let mut replay = vt100::Parser::new(24, 80, 0);
    replay.process(&snapshot);
    let screen = replay.screen();

    let row0 = (0..3)
        .map(|c| screen.cell(0, c).map(|cell| cell.contents()).unwrap_or_default().to_string())
        .collect::<String>();
    let row1 = (0..3)
        .map(|c| screen.cell(1, c).map(|cell| cell.contents()).unwrap_or_default().to_string())
        .collect::<String>();

    assert_eq!(row0, "abc", "row 0 of replayed snapshot");
    assert_eq!(row1, "def", "row 1 of replayed snapshot");
}

/// Plan §5 step 5 test #3: the map mutex makes attach atomic with respect to
/// `record_output`. We don't go for an unbounded concurrent stress — vt100 is
/// lossy by design (bytes get scrolled off, repainted by SGR changes) — but
/// we DO assert the "boundary" invariant the tmux-style attach design needs:
///
/// - bytes pushed BEFORE `subscribe_with_state` show up in the snapshot and
///   NOT on the live receiver,
/// - bytes pushed AFTER `subscribe_with_state` show up on the live receiver
///   and NOT in the snapshot.
#[tokio::test]
async fn subscribe_with_state_atomic_with_record_output() {
    let (registry, id) = make_session(24, 80).await;

    registry
        .record_output(id, Bytes::from_static(b"BEFORE"))
        .expect("record_output before-attach");

    let (snapshot, mut rx) = registry
        .subscribe_with_state(id)
        .expect("session is registered");

    registry
        .record_output(id, Bytes::from_static(b"AFTER"))
        .expect("record_output after-attach");

    // The snapshot reflects the pre-attach state — must contain "BEFORE".
    let snapshot_bytes: &[u8] = &snapshot;
    assert!(
        snapshot_bytes
            .windows(b"BEFORE".len())
            .any(|w| w == b"BEFORE"),
        "snapshot must contain pre-attach bytes 'BEFORE'"
    );
    // ...and NOT contain the post-attach bytes.
    assert!(
        !snapshot_bytes
            .windows(b"AFTER".len())
            .any(|w| w == b"AFTER"),
        "snapshot must NOT contain post-attach bytes 'AFTER'"
    );

    // The live receiver gets the post-attach chunk verbatim and only that
    // chunk (we attached AFTER the BEFORE write, so the broadcast receiver
    // never saw it).
    let received = tokio::time::timeout(Duration::from_secs(1), rx.recv())
        .await
        .expect("receiver should yield within 1s")
        .expect("recv ok");
    assert_eq!(&received[..], b"AFTER");

    // No further bytes pending.
    let drained = tokio::time::timeout(Duration::from_millis(50), rx.recv()).await;
    assert!(
        drained.is_err(),
        "no further bytes should be on the receiver after AFTER"
    );
}

/// Resize updates the parser screen. Subsequent snapshots must reflect the
/// new column width (a CSI cursor-positioning into a previously-out-of-range
/// column should be representable).
#[tokio::test]
async fn resize_changes_parser_screen_size() {
    let (registry, id) = make_session(24, 80).await;

    // Default 80 cols — write past col 80 to confirm the wrap behaviour.
    registry.resize(id, 24, 40).expect("resize");

    // Push bytes that span the new 40-col width: at col 38, "abcd" should
    // wrap to row 1.
    registry
        .record_output(id, Bytes::from_static(b"\x1b[1;39Habcd"))
        .expect("record_output after resize");

    let (snapshot, _rx) = registry.subscribe_with_state(id).expect("session");
    let mut replay = vt100::Parser::new(24, 40, 0);
    replay.process(&snapshot);
    let screen = replay.screen();
    // Rows are 0-indexed; col 38 ('a'), col 39 ('b'), then wrap to row 1.
    assert_eq!(screen.cell(0, 38).map(|c| c.contents().to_string()), Some("a".to_string()));
    assert_eq!(screen.cell(0, 39).map(|c| c.contents().to_string()), Some("b".to_string()));
    assert_eq!(screen.cell(1, 0).map(|c| c.contents().to_string()), Some("c".to_string()));
    assert_eq!(screen.cell(1, 1).map(|c| c.contents().to_string()), Some("d".to_string()));
}

/// `record_output` on a kill-removed session returns `NotFound`.
#[tokio::test]
async fn record_output_on_unknown_session_returns_notfound() {
    let registry = SessionRegistry::new();
    let bogus = uuid::Uuid::new_v4();
    let err = registry
        .record_output(bogus, Bytes::from_static(b"x"))
        .expect_err("must error on unknown id");
    assert!(matches!(err, omw_server::Error::NotFound(_)));
}
