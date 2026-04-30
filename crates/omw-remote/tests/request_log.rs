//! request_log table append/tail, per BYORC §6.3 + PRD §10.

use chrono::Utc;
use omw_remote::{open_db, RequestLog, RequestLogEntry};
use tempfile::tempdir;

fn fresh_log() -> RequestLog {
    let dir = tempdir().expect("tempdir");
    let path = dir.path().join("rl.sqlite");
    Box::leak(Box::new(dir));
    let conn = open_db(&path).expect("open db");
    RequestLog::new(conn)
}

#[test]
fn appended_accepted_row_is_readable_via_tail() {
    let log = fresh_log();
    let entry = RequestLogEntry {
        route: "/api/v1/sessions".into(),
        actor_device_id: Some("a1b2c3d4e5f6a7b8".into()),
        nonce: Some("nonce-abc".into()),
        ts: Utc::now(),
        signature: Some("sig-abc".into()),
        body_hash: Some("0".repeat(64)),
        accepted: true,
        reason: None,
    };
    log.append(entry.clone()).expect("append accepted row");

    let rows = log.tail(10).expect("tail");
    assert_eq!(rows.len(), 1);
    let r = &rows[0];
    assert!(r.accepted);
    assert_eq!(r.route, "/api/v1/sessions");
    assert_eq!(r.actor_device_id.as_deref(), Some("a1b2c3d4e5f6a7b8"));
    assert!(r.reason.is_none());
}

#[test]
fn appended_rejected_row_carries_reason() {
    let log = fresh_log();
    let entry = RequestLogEntry {
        route: "/api/v1/sessions".into(),
        actor_device_id: Some("a1b2c3d4e5f6a7b8".into()),
        nonce: Some("nonce-bad".into()),
        ts: Utc::now(),
        signature: Some("sig-bad".into()),
        body_hash: Some("0".repeat(64)),
        accepted: false,
        reason: Some("nonce_replayed".into()),
    };
    log.append(entry).expect("append rejected row");

    let rows = log.tail(10).expect("tail");
    assert_eq!(rows.len(), 1);
    let r = &rows[0];
    assert!(!r.accepted);
    assert_eq!(r.reason.as_deref(), Some("nonce_replayed"));
}

#[test]
fn tail_returns_at_most_n_rows_in_insertion_order() {
    let log = fresh_log();
    for i in 0..5 {
        let entry = RequestLogEntry {
            route: format!("/route/{i}"),
            actor_device_id: None,
            nonce: None,
            ts: Utc::now(),
            signature: None,
            body_hash: None,
            accepted: i % 2 == 0,
            reason: if i % 2 == 0 { None } else { Some("ts_skew".into()) },
        };
        log.append(entry).expect("append");
    }

    let last_three = log.tail(3).expect("tail");
    assert_eq!(last_three.len(), 3);
    // Oldest-of-the-three first; newest last.
    assert_eq!(last_three[0].route, "/route/2");
    assert_eq!(last_three[1].route, "/route/3");
    assert_eq!(last_three[2].route, "/route/4");
}
