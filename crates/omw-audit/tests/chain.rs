//! Hash-chain integrity tests for `omw-audit`.

use chrono::NaiveDate;
use omw_audit::{verify_chain, AuditWriter, GENESIS_PREV_HASH};
use serde_json::json;
use std::io::Write;
use std::path::PathBuf;
use tempfile::TempDir;
use uuid::Uuid;

fn fixed_session() -> Uuid {
    Uuid::nil()
}

fn fixed_date() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 5, 6).unwrap()
}

fn open_for_today(dir: &TempDir) -> AuditWriter {
    AuditWriter::open_for_day(dir.path().to_path_buf(), fixed_date()).expect("open writer")
}

#[test]
fn empty_directory_seeds_with_genesis_hash() {
    let dir = TempDir::new().unwrap();
    let writer = open_for_today(&dir);
    assert_eq!(writer.prev_hash(), GENESIS_PREV_HASH);
}

#[test]
fn append_100_entries_then_verify() {
    let dir = TempDir::new().unwrap();
    let mut writer = open_for_today(&dir);
    let session = fixed_session();
    for i in 0..100 {
        writer
            .append("test_event", session, json!({ "i": i, "label": format!("entry {i}") }))
            .expect("append");
    }
    let path: PathBuf = writer.current_path();
    drop(writer);

    let head = verify_chain(&path, GENESIS_PREV_HASH).expect("verify clean chain");
    assert_eq!(head.len(), 64);
}

#[test]
fn tamper_byte_detected() {
    let dir = TempDir::new().unwrap();
    let mut writer = open_for_today(&dir);
    let session = fixed_session();
    for i in 0..3 {
        writer
            .append("test_event", session, json!({ "i": i }))
            .expect("append");
    }
    let path = writer.current_path();
    drop(writer);

    // Mutate the second line: flip a byte in the middle.
    let original = std::fs::read_to_string(&path).unwrap();
    let mut lines: Vec<String> = original.lines().map(|l| l.to_string()).collect();
    assert!(lines.len() >= 2);
    let target = &mut lines[1];
    let byte_idx = target.len() / 2;
    let mut bytes = target.as_bytes().to_vec();
    bytes[byte_idx] ^= 0x01;
    *target = String::from_utf8(bytes).expect("flip should keep utf-8");
    let mutated = format!("{}\n", lines.join("\n"));
    std::fs::write(&path, mutated).unwrap();

    let err = verify_chain(&path, GENESIS_PREV_HASH).unwrap_err();
    let msg = format!("{err}");
    assert!(
        msg.contains("hash mismatch") || msg.contains("malformed") || msg.contains("parse"),
        "expected verification failure, got: {msg}"
    );
}

#[test]
fn cross_day_chain_continues_from_yesterday_tail() {
    let dir = TempDir::new().unwrap();
    let yesterday = NaiveDate::from_ymd_opt(2026, 5, 5).unwrap();
    let today = NaiveDate::from_ymd_opt(2026, 5, 6).unwrap();
    let session = fixed_session();

    // Day 1: append two entries.
    let mut writer =
        AuditWriter::open_for_day(dir.path().to_path_buf(), yesterday).expect("open day 1");
    writer.append("yesterday_a", session, json!({"i": 1})).unwrap();
    writer.append("yesterday_b", session, json!({"i": 2})).unwrap();
    let yesterday_tail = writer.prev_hash().to_string();
    drop(writer);

    // Day 2: a fresh writer should pick up from yesterday's tail.
    let mut writer =
        AuditWriter::open_for_day(dir.path().to_path_buf(), today).expect("open day 2");
    assert_eq!(
        writer.prev_hash(),
        yesterday_tail,
        "day 2 must seed from day 1 tail"
    );
    writer.append("today_a", session, json!({"i": 3})).unwrap();
    let today_path = writer.current_path();
    drop(writer);

    // Verify day 2 against day 1's tail.
    verify_chain(&today_path, &yesterday_tail).expect("day 2 chain verifies");
}

#[test]
fn reopening_picks_up_existing_tail() {
    let dir = TempDir::new().unwrap();
    let session = fixed_session();
    let mut writer = open_for_today(&dir);
    writer.append("a", session, json!({})).unwrap();
    let after_first = writer.prev_hash().to_string();
    drop(writer);

    let writer = open_for_today(&dir);
    assert_eq!(writer.prev_hash(), after_first);
}

#[test]
fn malformed_line_fails_verification() {
    let dir = TempDir::new().unwrap();
    let mut writer = open_for_today(&dir);
    writer
        .append("a", fixed_session(), json!({}))
        .unwrap();
    let path = writer.current_path();
    drop(writer);

    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    f.write_all(b"this is not json\n").unwrap();

    let err = verify_chain(&path, GENESIS_PREV_HASH).unwrap_err();
    let msg = format!("{err}");
    // The "this is not json" line gets parsed before its prev_hash is
    // checked, so we accept any of the parse / malformed errors.
    assert!(
        msg.contains("parse")
            || msg.contains("malformed")
            || msg.contains("expected")
            || msg.contains("EOF"),
        "expected parse failure, got: {msg}"
    );
}
