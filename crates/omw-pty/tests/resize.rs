//! B.3 — Resize behavior + drop-with-live-child does not panic.
//!
//! For the (0, 0) case we pin whatever portable-pty does on the host. The
//! current portable-pty behavior (as of 0.8) accepts zero dimensions without
//! erroring on Unix; we accept either Ok or Err but require it to NOT panic.
//!
//! CROSS_PLATFORM_NOTE: Each platform has its own observable-resize test:
//!
//! - Unix: `resize_changes_observed_terminal_size` uses `stty size` to read
//!   back the kernel's view of the PTY winsize after each resize call.
//! - Windows: `resize_changes_observed_terminal_size_windows` uses a
//!   PowerShell loop that prints `[Console]::WindowWidth`/`WindowHeight`
//!   after each line of input. ConPTY forwards resize to the child's
//!   console state, so these properties should reflect the new size.
//!
//! Both tests prove that `resize()` actually forwarded the call (not just
//! returned `Ok(())` from a stub). The Windows test allows a ±1 tolerance
//! because some ConPTY configurations differ on whether the trailing column
//! is included; the contract under test is "observed size changed in the
//! right direction," not "exact equality." See `specs/fork-strategy.md` §9
//! if this proves unreliable in CI.
//!
//! `resize_returns_ok_for_normal_dimensions` remains as a cross-platform
//! smoke test that at least catches the case where `resize()` panics or
//! returns Err on a normal dimension.

use omw_pty::{Pty, PtyCommand};
use std::time::Duration as StdDuration;
use tokio::time::{timeout, Duration};

fn long_running_cmd() -> PtyCommand {
    if cfg!(windows) {
        // `timeout` is built-in on modern Windows and waits N seconds.
        PtyCommand::new("cmd")
            .arg("/c")
            .arg("timeout /t 60 /nobreak")
    } else {
        PtyCommand::new("/bin/sh").arg("-c").arg("sleep 60")
    }
}

#[tokio::test]
async fn resize_returns_ok_for_normal_dimensions() {
    let mut pty = Pty::spawn(long_running_cmd())
        .await
        .expect("spawn should succeed");

    pty.resize(120, 40)
        .expect("resize to 120x40 should succeed");

    pty.resize(200, 60).expect("a second resize should succeed");

    // Clean up: kill and reap so the test exits promptly.
    pty.kill().expect("kill should succeed");
    let _ = timeout(Duration::from_secs(5), pty.wait())
        .await
        .expect("wait did not return within 5s after kill");
}

#[tokio::test]
async fn resize_zero_dimensions_does_not_panic() {
    let mut pty = Pty::spawn(long_running_cmd())
        .await
        .expect("spawn should succeed");

    // Pin: zero dimensions must not panic. We accept either Ok or Err — the
    // assertion is the absence of a panic, plus an explicit pin that the
    // call returned a Result we can discard.
    let _ = pty.resize(0, 0);

    pty.kill().expect("kill should succeed");
    let _ = timeout(Duration::from_secs(5), pty.wait())
        .await
        .expect("wait did not return within 5s after kill");
}

/// CROSS_PLATFORM_NOTE: Unix-only. See module-level note above for the
/// Windows companion (`resize_changes_observed_terminal_size_windows`).
///
/// Strategy: spawn an `sh` loop that, for every line of input it reads, runs
/// `stty size` and prints the result. This gives us a probe to read back the
/// PTY winsize as the kernel sees it. We:
///
///   1. Trigger one probe at the spawn-time default size (80x24).
///   2. Call `resize(120, 40)`.
///   3. Trigger a second probe.
///   4. Call `resize(200, 60)`.
///   5. Trigger a third probe.
///
/// We then assert the three reported sizes match the expected sequence —
/// proving the wrapper actually forwarded the resizes (not just returned
/// `Ok(())` from a stub).
#[cfg(unix)]
#[tokio::test]
async fn resize_changes_observed_terminal_size() {
    // `stty size` prints "rows cols". The loop runs `stty size` for every
    // line we send on stdin, so we control when each probe fires. Script
    // body is POSIX — use `sh` to widen compatibility (busybox/Alpine).
    let cmd = PtyCommand::new("sh")
        .arg("-c")
        .arg("while IFS= read -r line; do stty size; done");

    let mut pty = Pty::spawn(cmd).await.expect("spawn sh should succeed");

    let mut reader = pty.reader().expect("reader once");
    let mut writer = pty.writer().expect("writer once");

    // Helper: fire one probe (write a newline) and read until we see one
    // "rows cols" pair we have not already consumed. We accumulate output
    // into a Vec<u8>, parse out lines that match `^\d+ \d+$`, and return
    // the most recent one we hadn't seen yet.
    async fn probe(
        writer: &mut omw_pty::PtyWriter,
        reader: &mut omw_pty::PtyReader,
        already_seen: usize,
        accumulator: &mut Vec<u8>,
    ) -> (u16, u16, usize) {
        // Trigger the child's `stty size`.
        writer
            .write_all(b"\n")
            .await
            .expect("write newline to bash loop");

        // Read until we have at least `already_seen + 1` "rows cols" pairs in
        // the accumulated output, with a hard timeout.
        let collected = timeout(Duration::from_secs(5), async {
            let mut buf = [0u8; 1024];
            loop {
                let n = reader.read(&mut buf).await.expect("read from PTY");
                if n == 0 {
                    panic!("PTY hit EOF before stty size probe responded");
                }
                accumulator.extend_from_slice(&buf[..n]);

                let text = String::from_utf8_lossy(accumulator);
                let pairs: Vec<(u16, u16)> = text
                    .lines()
                    .filter_map(|line| {
                        let trimmed = line.trim();
                        let mut parts = trimmed.split_ascii_whitespace();
                        let rows: u16 = parts.next()?.parse().ok()?;
                        let cols: u16 = parts.next()?.parse().ok()?;
                        if parts.next().is_some() {
                            return None;
                        }
                        Some((rows, cols))
                    })
                    .collect();

                if pairs.len() > already_seen {
                    let (rows, cols) = *pairs.last().unwrap();
                    return (rows, cols, pairs.len());
                }
            }
        })
        .await;

        collected.expect("probe did not yield a stty size line within 5s")
    }

    let mut acc = Vec::<u8>::new();

    // Probe 1: default size (80x24 per PtySize::default()).
    let (rows1, cols1, count1) = probe(&mut writer, &mut reader, 0, &mut acc).await;
    assert_eq!(
        (rows1, cols1),
        (24, 80),
        "default PTY size at spawn should be 80x24, got rows={rows1} cols={cols1}"
    );

    // Resize and probe again.
    pty.resize(120, 40).expect("resize 120x40 should succeed");
    let (rows2, cols2, count2) = probe(&mut writer, &mut reader, count1, &mut acc).await;
    assert_eq!(
        (rows2, cols2),
        (40, 120),
        "after resize(120, 40) stty size should report 40 rows 120 cols, got rows={rows2} cols={cols2}"
    );

    // Resize again and probe a third time, to make sure it isn't a fluke.
    pty.resize(200, 60).expect("resize 200x60 should succeed");
    let (rows3, cols3, _count3) = probe(&mut writer, &mut reader, count2, &mut acc).await;
    assert_eq!(
        (rows3, cols3),
        (60, 200),
        "after resize(200, 60) stty size should report 60 rows 200 cols, got rows={rows3} cols={cols3}"
    );

    // Drop the writer to let bash exit cleanly on EOF.
    drop(writer);
    pty.kill().expect("kill should succeed");
    let _ = timeout(Duration::from_secs(5), pty.wait())
        .await
        .expect("wait did not return within 5s after kill");
}

/// CROSS_PLATFORM_NOTE: Windows companion to
/// `resize_changes_observed_terminal_size`. Spawn a PowerShell loop that
/// reads a line from stdin, then prints `WIDTHxHEIGHT` from
/// `[System.Console]::WindowWidth`/`WindowHeight`. ConPTY forwards resize
/// to the child's console state, so these properties reflect the size we
/// last asked for via `pty.resize()`.
///
/// Tolerance: we allow ±1 on each axis because some ConPTY configurations
/// disagree on whether the trailing column/row is reported. The contract
/// under test is "observed size changed in the right direction after each
/// resize call" — a stub `Ok(())` impl that doesn't actually forward
/// resize would leave the observed size pinned at the spawn-time default.
///
/// If [Console]::WindowWidth turns out to NOT track ConPTY resize on the
/// CI host, switch this test to `#[ignore = "..."]` and add an entry to
/// `specs/fork-strategy.md` §9 describing the gap.
/// Strip ANSI CSI escape sequences (e.g. `\x1b[K`, `\x1b[?25l`, `\x1b[8;40;120t`)
/// from a string. ConPTY emits these during post-resize screen repaints, and
/// they break naive `parse::<u16>()` on otherwise-clean digit lines like
/// `120x40\x1b[K`. Test-file private; only the Windows parser uses it.
#[cfg(windows)]
fn strip_ansi(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'[' {
            // CSI: ESC [ ... final-byte-in-range-0x40..=0x7e
            let mut j = i + 2;
            while j < bytes.len() {
                let b = bytes[j];
                if (0x40..=0x7e).contains(&b) {
                    j += 1;
                    break;
                }
                j += 1;
            }
            i = j;
        } else if bytes[i] == 0x1b {
            // Bare ESC or other escape (e.g. ESC ] OSC ... BEL). Skip the ESC
            // plus the next byte if it looks like the start of an OSC, then
            // walk to BEL or ST. Conservative: just drop the ESC and continue.
            i += 1;
        } else {
            // Safe because we only land here on a non-ESC byte; UTF-8 multi-
            // byte continuation bytes are >= 0x80, which is fine to push as
            // part of a char sequence — but we're walking by byte, so we need
            // to be careful. Use the original char iterator for non-ESC runs.
            // Find next ESC and copy the slice as-is via from_utf8_lossy.
            let start = i;
            while i < bytes.len() && bytes[i] != 0x1b {
                i += 1;
            }
            out.push_str(&String::from_utf8_lossy(&bytes[start..i]));
        }
    }
    out
}

#[cfg(windows)]
#[tokio::test]
async fn resize_changes_observed_terminal_size_windows() {
    // Loop: read one line, print "WIDTHxHEIGHT". Exit on null input (EOF).
    let script = "while ($true) { $line = [Console]::In.ReadLine(); \
                  if ($null -eq $line) { break }; \
                  Write-Host (\"{0}x{1}\" -f [Console]::WindowWidth, [Console]::WindowHeight) }";
    let cmd = PtyCommand::new("powershell")
        .arg("-NoProfile")
        .arg("-NonInteractive")
        .arg("-Command")
        .arg(script);

    let mut pty = Pty::spawn(cmd)
        .await
        .expect("spawn powershell should succeed");

    let mut reader = pty.reader().expect("reader once");
    let mut writer = pty.writer().expect("writer once");

    // Helper: parse all `<digits>x<digits>` pairs from the accumulated output,
    // stripping ANSI escapes first (ConPTY emits `\x1b[K`, cursor-hide, etc.
    // during the post-resize repaint). Returns the full list in arrival order.
    fn parse_pairs(accumulator: &[u8]) -> Vec<(u16, u16)> {
        let text = String::from_utf8_lossy(accumulator);
        text.lines()
            .filter_map(|line| {
                let stripped = strip_ansi(line);
                let trimmed = stripped.trim();
                let (w, h) = trimmed.split_once('x')?;
                let w: u16 = w.parse().ok()?;
                let h: u16 = h.parse().ok()?;
                Some((w, h))
            })
            .collect()
    }

    // Helper: send "\r\n" to trigger one probe, then read until we see one
    // new "WIDTHxHEIGHT" pair we have not yet consumed.
    async fn probe(
        writer: &mut omw_pty::PtyWriter,
        reader: &mut omw_pty::PtyReader,
        already_seen: usize,
        accumulator: &mut Vec<u8>,
    ) -> (u16, u16, usize) {
        writer
            .write_all(b"\r\n")
            .await
            .expect("write CRLF to powershell loop");

        let collected = timeout(Duration::from_secs(10), async {
            let mut buf = [0u8; 1024];
            loop {
                let n = reader.read(&mut buf).await.expect("read from PTY");
                if n == 0 {
                    panic!("PTY hit EOF before WIDTHxHEIGHT probe responded");
                }
                accumulator.extend_from_slice(&buf[..n]);

                let pairs = parse_pairs(accumulator);
                if pairs.len() > already_seen {
                    let (w, h) = *pairs.last().unwrap();
                    return (w, h, pairs.len());
                }
            }
        })
        .await;

        collected.expect("probe did not yield a WIDTHxHEIGHT line within 10s")
    }

    let mut acc = Vec::<u8>::new();

    // Probe 1: pin whatever PowerShell reports at spawn-time default size
    // (PtySize::default() is 80x24). We don't assert exact equality here —
    // some ConPTY versions report 79 instead of 80, etc. We only need a
    // baseline to compare subsequent probes against.
    let (w1, h1, count1) = probe(&mut writer, &mut reader, 0, &mut acc).await;

    // Resize to 120x40 and probe.
    pty.resize(120, 40).expect("resize 120x40 should succeed");
    let (w2, h2, count2) = probe(&mut writer, &mut reader, count1, &mut acc).await;

    // Resize to 200x60 and probe again — proves it isn't a one-shot fluke.
    pty.resize(200, 60).expect("resize 200x60 should succeed");
    let (w3, h3, _count3) = probe(&mut writer, &mut reader, count2, &mut acc).await;

    // Drop the writer so PowerShell sees EOF on its ReadLine and exits.
    drop(writer);
    pty.kill()
        .expect("kill should succeed (or be a no-op if already exited)");
    let _ = timeout(Duration::from_secs(5), pty.wait())
        .await
        .expect("wait did not return within 5s after kill");

    // Now do the assertions on the deduplicated full-sequence view of the
    // accumulator. ConPTY's post-resize repaint emits the same `WxH` line
    // multiple times (once per cleared row), so we collapse consecutive
    // duplicates. The contract is "the sequence of distinct sizes contains
    // ~80x24, then ~120x40, then ~200x60, in that order, with ±1 tolerance."
    let all_pairs = parse_pairs(&acc);
    let mut deduped: Vec<(u16, u16)> = Vec::new();
    for p in &all_pairs {
        if deduped.last() != Some(p) {
            deduped.push(*p);
        }
    }

    fn near(a: u16, b: u16) -> bool {
        a.abs_diff(b) <= 1
    }

    // Find the three expected sizes in order in the deduped sequence.
    let expected: [(u16, u16); 3] = [(80, 24), (120, 40), (200, 60)];
    let mut idx = 0usize;
    for &(w, h) in &deduped {
        if idx < expected.len() {
            let (ew, eh) = expected[idx];
            if near(w, ew) && near(h, eh) {
                idx += 1;
            }
        }
    }

    assert_eq!(
        idx,
        expected.len(),
        "expected the deduped size sequence to contain ~80x24, then ~120x40, \
         then ~200x60 (±1 each), in order. \
         baseline=({w1},{h1}) after-120x40=({w2},{h2}) after-200x60=({w3},{h3}). \
         all_pairs={all_pairs:?} deduped={deduped:?}"
    );
}

/// Finding 3 — drop must actually kill the child, not just return quickly.
///
/// We give the child an observable side effect (heartbeat lines appended to
/// a tempfile every 1s — POSIX `sleep` is integer-second only). After
/// `drop(pty)`, the file MUST stop growing. A leaky impl that drops fast but
/// leaves the child running will keep appending and fail the post-drop
/// equality assertion.
#[tokio::test]
async fn drop_with_live_child_actually_kills_child() {
    let tmp = tempfile::NamedTempFile::new().expect("create tempfile");
    let path = tmp.path().to_path_buf();
    // Close the handle but keep the path alive (tmp is dropped explicitly
    // below at end of test). On Windows we need the file to be closeable
    // by other processes, so re-open path-only.
    drop(tmp);

    let path_str = path
        .to_str()
        .expect("tempfile path should be valid UTF-8")
        .to_owned();

    let cmd = if cfg!(windows) {
        // PowerShell heartbeat: mirror the Unix 1s cadence so both branches
        // share the same warmup/post-drop windows downstream.
        let script = format!(
            "while ($true) {{ Add-Content -LiteralPath '{}' -Value 'h'; Start-Sleep -Seconds 1 }}",
            path_str.replace('\'', "''")
        );
        PtyCommand::new("powershell")
            .arg("-NoProfile")
            .arg("-Command")
            .arg(script)
    } else {
        // POSIX `sleep` only takes integer seconds (busybox/Alpine), so we
        // use `sleep 1` rather than `sleep 0.1`. Warmup and post-drop
        // windows below are widened to match this 1s cadence.
        let script = format!(
            "while true; do echo h >> '{}'; sleep 1; done",
            path_str.replace('\'', "'\\''")
        );
        PtyCommand::new("/bin/sh").arg("-c").arg(script)
    };

    let pty = Pty::spawn(cmd).await.expect("spawn heartbeat child");

    // Heartbeat warmup — child sleeps 1s between writes, so 2s gives at
    // least one write reliably.
    tokio::time::sleep(Duration::from_secs(2)).await;

    let size_before_drop = std::fs::metadata(&path)
        .expect("heartbeat file must exist before drop")
        .len();
    assert!(
        size_before_drop > 0,
        "heartbeat child should have written something before drop, but file is empty"
    );

    // Drop in a guarded task so a hang surfaces as a timeout, not a hang.
    let drop_task = tokio::task::spawn_blocking(move || {
        drop(pty);
    });
    timeout(Duration::from_secs(3), drop_task)
        .await
        .expect("drop with live child must not hang for >3s")
        .expect("drop task should not panic");

    // Brief settle window so the OS can reap the child after drop, then
    // sample the file size as our "post-drop baseline."
    tokio::time::sleep(StdDuration::from_millis(200)).await;
    let size_after_drop = std::fs::metadata(&path)
        .expect("heartbeat file must still exist after drop")
        .len();

    // Wait one full heartbeat cycle (1s) plus margin. A leaky impl that left
    // the child alive will append at least one more 'h\n' in this window.
    tokio::time::sleep(Duration::from_secs(2)).await;
    let size_final = std::fs::metadata(&path)
        .expect("heartbeat file must still exist at end of test")
        .len();

    assert_eq!(
        size_after_drop, size_final,
        "heartbeat file kept growing after drop ({} -> {}) — child was not killed when Pty was dropped",
        size_after_drop, size_final
    );

    // Best-effort cleanup; ignore errors.
    let _ = std::fs::remove_file(&path);
}
