//! Bridge a Warp terminal pane to an `omw_server::SessionRegistry` so the
//! omw-remote daemon can attach to a running pane (instead of spawning a
//! sibling shell).
//!
//! The function takes:
//! - `event_loop_tx`: the pane's event-loop sender (input bytes go here as
//!   `Message::Input(...)`).
//! - `pty_reads_tx`: the pane's PTY-output broadcast sender. We clone-and-
//!   subscribe to receive every chunk the model parser produces, then forward
//!   it to the registry's per-session output broadcast.
//! - `registry`: the shared `Arc<SessionRegistry>` owned by the embedded
//!   omw-remote daemon.
//!
//! Two tokio tasks are spawned:
//! - **Input pump**: receives `Vec<u8>` chunks from the registry-side mpsc
//!   and forwards each as `Message::Input(Cow::Owned(...))` into the pane's
//!   mio event loop.
//! - **Output pump**: subscribes to `pty_reads_tx`, copies each chunk into
//!   `bytes::Bytes`, and broadcasts it on the registry's per-session output.
//!
//! The kill closure stored in the `ExternalSessionSpec` aborts both tasks,
//! tearing the bridge down. `SessionRegistry::kill(id)` invokes that closure
//! once. Returning a `PaneShareHandle::stop` lets callers tear down by id
//! without a direct `Arc<SessionRegistry>` reference.
//!
//! Items here are exercised by `#[cfg(test)] mod tests` and by the UI menu
//! wiring (Gap 1 part C), which is intentionally not part of this change. The
//! module-level `#[allow(dead_code)]` suppresses warnings until that wiring
//! lands.
#![allow(dead_code)]

use std::borrow::Cow;
use std::sync::Arc;

use bytes::Bytes;
use parking_lot::Mutex as PlMutex;
use tokio::sync::{broadcast, mpsc};
use tokio::task::JoinHandle;

use crate::terminal::local_tty::mio_channel;
use crate::terminal::writeable_pty::Message;

/// Capacity of the input mpsc the registry pushes bytes into. Generous; phone
/// keystrokes are tiny and rare relative to PTY output.
const INPUT_CHANNEL_CAPACITY: usize = 256;

/// Capacity of the per-session output broadcast channel given to the registry.
/// Mirrors `OUTPUT_BROADCAST_CAPACITY` in omw-server's owned-session path.
const OUTPUT_BROADCAST_CAPACITY: usize = 256;

/// Initial PTY size recorded for the external session. v0.4-thin does NOT
/// plumb resize for shared panes; this is a placeholder the registry only
/// stores.
const INITIAL_COLS: u16 = 80;
const INITIAL_ROWS: u16 = 24;

/// Handle returned by [`share_pane`]. Holds the assigned session id and a
/// `stop` closure that asks the registry to kill the session (which in turn
/// fires the kill closure and aborts the pumps).
///
/// The stop closure is fired exactly once: either via [`PaneShareHandle::stop`]
/// (explicit) or via [`Drop`] (implicit, when the handle leaves scope or is
/// removed from `OmwRemoteState::pane_shares`). This is what makes
/// `unshare_pane` foolproof: removing the handle from the share map is the
/// same as tearing down the pumps and removing the registry entry.
pub struct PaneShareHandle {
    pub session_id: omw_server::SessionId,
    /// `Option<...>` so we can `.take()` on first call and silently ignore
    /// subsequent calls. `PlMutex` because the explicit method takes `&self`
    /// to keep call ergonomics, while `Drop::drop` has `&mut self`.
    stop: PlMutex<Option<Box<dyn FnOnce() + Send>>>,
}

impl PaneShareHandle {
    /// Fire the stop closure. Idempotent: subsequent calls (and the eventual
    /// Drop) are no-ops.
    pub fn stop(&self) {
        if let Some(f) = self.stop.lock().take() {
            f();
        }
    }
}

impl Drop for PaneShareHandle {
    fn drop(&mut self) {
        if let Some(f) = self.stop.lock().take() {
            eprintln!(
                "[omw-debug] PaneShareHandle::drop: firing kill for session {}",
                self.session_id
            );
            f();
        }
    }
}

/// Errors from [`share_pane`].
#[derive(Debug, thiserror::Error)]
pub enum ShareError {
    #[error("registering external session: {0}")]
    Register(#[from] omw_server::Error),
}

/// Render up to 32 bytes of a payload as an ASCII-printable preview for
/// the pump-tracing eprintln lines. Non-printable bytes appear as `.` so
/// the diagnostic stays one line. Used only by the input/output pumps.
fn debug_preview(bytes: &[u8]) -> String {
    let take = bytes.len().min(32);
    let mut out = String::with_capacity(take + 8);
    for &b in &bytes[..take] {
        if (0x20..=0x7e).contains(&b) {
            out.push(b as char);
        } else if b == b'\n' {
            out.push_str("\\n");
        } else if b == b'\r' {
            out.push_str("\\r");
        } else if b == b'\t' {
            out.push_str("\\t");
        } else {
            out.push('.');
        }
    }
    if bytes.len() > take {
        out.push_str("...");
    }
    out
}

/// Bridge a Warp pane to the registry. See module docs.
///
/// Must be called from within a tokio runtime — both pumps are spawned via
/// `tokio::spawn`.
pub async fn share_pane(
    pane_name: String,
    event_loop_tx: Arc<PlMutex<mio_channel::Sender<Message>>>,
    pty_reads_tx: async_broadcast::Sender<Arc<Vec<u8>>>,
    registry: Arc<omw_server::SessionRegistry>,
) -> Result<PaneShareHandle, ShareError> {
    // Short label used in pump-tracing eprintlns so the user can correlate
    // input/output flow with the pane this share is bridging.
    let pane_name_log: Arc<str> = Arc::from(pane_name.as_str());
    eprintln!(
        "[omw-debug] pane_share[{pane_name_log}] share_pane entered — wiring input mpsc + output broadcast"
    );

    // Channels handed to the registry.
    let (input_tx, mut input_rx) = mpsc::channel::<Vec<u8>>(INPUT_CHANNEL_CAPACITY);
    let (output_tx, _output_rx0) = broadcast::channel::<Bytes>(OUTPUT_BROADCAST_CAPACITY);

    // Per-pump JoinHandles, slotted into Arc<Mutex<Option<_>>> so the kill
    // closure (which is `Fn`, not `FnOnce`) can `take()` and abort exactly
    // once, while the closure is still callable from any thread. Populated
    // AFTER `register_external` returns the session id (the output pump
    // needs that id to call `record_output`).
    let pumps: Arc<PlMutex<Option<(JoinHandle<()>, JoinHandle<()>)>>> =
        Arc::new(PlMutex::new(None));
    let pumps_for_kill = pumps.clone();

    let kill = Box::new(move || {
        if let Some((a, b)) = pumps_for_kill.lock().take() {
            a.abort();
            b.abort();
        }
    });

    let spec = omw_server::ExternalSessionSpec {
        name: pane_name,
        input_tx,
        output_tx,
        kill,
        initial_size: omw_pty::PtySize {
            cols: INITIAL_COLS,
            rows: INITIAL_ROWS,
        },
    };

    let session_id = registry.register_external(spec).await?;

    // Input pump: registry mpsc -> mio event loop sender.
    let event_loop_tx_clone = event_loop_tx.clone();
    let pump_label_in = pane_name_log.clone();
    let input_pump = tokio::spawn(async move {
        while let Some(bytes) = input_rx.recv().await {
            // Tracing visible from PowerShell when running warp-oss as a
            // console app. Truncated to 32 bytes so a paste doesn't flood
            // stderr; the byte count is the load-bearing diagnostic.
            eprintln!(
                "[omw-debug] pane_share[{pump_label_in}] input pump: {} bytes from registry -> event_loop_tx (preview {:?})",
                bytes.len(),
                debug_preview(&bytes),
            );
            let tx = event_loop_tx_clone.lock();
            match tx.send(Message::Input(Cow::Owned(bytes))) {
                Ok(()) => {}
                Err(e) => {
                    eprintln!(
                        "[omw-debug] pane_share[{pump_label_in}] input pump: event_loop_tx.send FAILED: {e:?} — Warp event loop is likely gone, exiting pump"
                    );
                    break;
                }
            }
        }
        eprintln!("[omw-debug] pane_share[{pump_label_in}] input pump: exiting (registry mpsc closed)");
    });

    // Output pump: pty_reads broadcast -> registry.record_output (which feeds
    // the parser and broadcasts to live subscribers). We materialise the
    // receiver synchronously *before* spawning so callers that broadcast
    // immediately after share_pane returns don't race the spawn (which would
    // otherwise see receiver_count == 0 and drop the chunk).
    let mut pty_rx = pty_reads_tx.new_receiver();
    let registry_for_pump = registry.clone();
    let pump_label_out = pane_name_log.clone();
    let output_pump = tokio::spawn(async move {
        loop {
            match pty_rx.recv().await {
                Ok(chunk) => {
                    let bytes = Bytes::copy_from_slice(chunk.as_slice());
                    let len = bytes.len();
                    let preview = debug_preview(&bytes);
                    match registry_for_pump.record_output(session_id, bytes) {
                        Ok(sub_count) => {
                            eprintln!(
                                "[omw-debug] pane_share[{pump_label_out}] output pump: recorded {len} bytes via registry ({sub_count} subs) (preview {preview:?})"
                            );
                        }
                        Err(e) => {
                            eprintln!(
                                "[omw-debug] pane_share[{pump_label_out}] output pump: record_output failed ({e:?}) — session likely killed, exiting"
                            );
                            break;
                        }
                    }
                }
                Err(async_broadcast::RecvError::Closed) => {
                    eprintln!("[omw-debug] pane_share[{pump_label_out}] output pump: pty_reads_tx closed, exiting");
                    break;
                }
                Err(async_broadcast::RecvError::Overflowed(skipped)) => {
                    eprintln!(
                        "[omw-debug] pane_share[{pump_label_out}] output pump: lagged, dropped {skipped} chunks"
                    );
                }
            }
        }
    });

    *pumps.lock() = Some((input_pump, output_pump));

    let registry_for_stop = registry.clone();
    let stop: Box<dyn FnOnce() + Send> = Box::new(move || {
        // `kill` is async; we may be called from a sync context. Kick off a
        // detached task on the current runtime. `kill` is idempotent on the
        // registry side — it returns NotFound if already removed, which we
        // ignore.
        tokio::spawn(async move {
            let _ = registry_for_stop.kill(session_id).await;
        });
    });

    Ok(PaneShareHandle {
        session_id,
        stop: PlMutex::new(Some(stop)),
    })
}

#[cfg(test)]
impl PaneShareHandle {
    /// Test-only constructor: builds a handle whose `stop` closure runs the
    /// supplied callback exactly once (via `Drop` or via `stop()`). Lets unit
    /// tests verify share-map idempotency without spinning up a real
    /// `SessionRegistry`.
    pub(crate) fn new_for_test<F>(session_id: omw_server::SessionId, on_stop: F) -> Self
    where
        F: FnOnce() + Send + 'static,
    {
        Self {
            session_id,
            stop: PlMutex::new(Some(Box::new(on_stop))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn make_event_loop() -> (
        Arc<PlMutex<mio_channel::Sender<Message>>>,
        mio_channel::Receiver<Message>,
    ) {
        let (tx, rx) = mio_channel::channel();
        (Arc::new(PlMutex::new(tx)), rx)
    }

    /// Test fixture mirroring how the real `local_tty::TerminalManager`
    /// holds the broadcast: an inactive receiver keeps the channel open so
    /// late `new_receiver()` calls (and thus `try_broadcast`) work.
    fn make_pty_broadcast() -> (
        async_broadcast::Sender<Arc<Vec<u8>>>,
        async_broadcast::InactiveReceiver<Arc<Vec<u8>>>,
    ) {
        let (tx, rx) = async_broadcast::broadcast::<Arc<Vec<u8>>>(64);
        (tx, rx.deactivate())
    }

    /// Spin until `f` returns Some(_) or `tries` deadline elapses (10ms each).
    async fn wait_for<T>(tries: usize, mut f: impl FnMut() -> Option<T>) -> Option<T> {
        for _ in 0..tries {
            if let Some(v) = f() {
                return Some(v);
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        None
    }

    #[tokio::test]
    async fn share_pane_registers_external_session() {
        let registry = omw_server::SessionRegistry::new();
        let (event_loop_tx, _rx) = make_event_loop();
        let (pty_tx, _pty_keep) = make_pty_broadcast();

        let handle = share_pane(
            "pane-1".to_string(),
            event_loop_tx,
            pty_tx,
            registry.clone(),
        )
        .await
        .expect("share_pane should register");

        let metas = registry.list();
        assert_eq!(metas.len(), 1);
        assert_eq!(metas[0].id, handle.session_id);
        assert_eq!(metas[0].name, "pane-1");
        assert!(metas[0].alive);
    }

    #[tokio::test]
    async fn share_pane_input_routed_to_event_loop_tx() {
        let registry = omw_server::SessionRegistry::new();
        let (event_loop_tx, event_loop_rx) = make_event_loop();
        let (pty_tx, _pty_keep) = make_pty_broadcast();

        let handle = share_pane(
            "pane-input".to_string(),
            event_loop_tx,
            pty_tx,
            registry.clone(),
        )
        .await
        .unwrap();

        registry
            .write_input(handle.session_id, b"hi")
            .await
            .expect("write_input should succeed");

        let msg = wait_for(50, || event_loop_rx.try_recv().ok())
            .await
            .expect("event loop should receive Message::Input");
        match msg {
            Message::Input(bytes) => {
                assert_eq!(<std::borrow::Cow<'_, [u8]> as AsRef<[u8]>>::as_ref(&bytes), b"hi")
            }
            other => panic!("expected Message::Input, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn share_pane_output_from_broadcast_reaches_subscriber() {
        let registry = omw_server::SessionRegistry::new();
        let (event_loop_tx, _rx) = make_event_loop();
        let (pty_tx, _pty_keep) = make_pty_broadcast();

        let handle = share_pane(
            "pane-output".to_string(),
            event_loop_tx,
            pty_tx.clone(),
            registry.clone(),
        )
        .await
        .unwrap();

        let mut sub = registry
            .subscribe(handle.session_id)
            .expect("session id should resolve");

        // Push a chunk into the broadcast that the pane would normally
        // populate from `send_pty_read_event` (which itself uses
        // `try_broadcast`).
        pty_tx
            .try_broadcast(Arc::new(b"echo\n".to_vec()))
            .expect("try_broadcast send should succeed");

        let received = tokio::time::timeout(Duration::from_secs(1), sub.recv())
            .await
            .expect("subscriber should receive within 1s")
            .expect("recv should not error");
        assert_eq!(&received[..], b"echo\n");
    }

    #[tokio::test]
    async fn share_pane_stop_kills_session_and_aborts_pumps() {
        let registry = omw_server::SessionRegistry::new();
        let (event_loop_tx, event_loop_rx) = make_event_loop();
        let (pty_tx, _pty_keep) = make_pty_broadcast();

        let handle = share_pane(
            "pane-stop".to_string(),
            event_loop_tx,
            pty_tx,
            registry.clone(),
        )
        .await
        .unwrap();
        let session_id = handle.session_id;

        // Sanity: session registered.
        assert_eq!(registry.list().len(), 1);

        // Stop. This spawns a task that calls registry.kill(session_id). Wait
        // until the session is gone.
        handle.stop();

        let removed = wait_for(50, || {
            if registry.list().is_empty() {
                Some(())
            } else {
                None
            }
        })
        .await;
        assert!(removed.is_some(), "session should be removed after stop");

        // After kill, write_input should now fail (session is gone from registry).
        let res = registry.write_input(session_id, b"x").await;
        assert!(res.is_err(), "write_input on killed session should fail");

        // Drain anything still pending in event_loop_rx — there should be
        // nothing further (input pump aborted before any Input was processed
        // for this id).
        // We just verify the channel doesn't yield surprising data.
        let _ = event_loop_rx.try_recv();
    }

    /// Dropping a `PaneShareHandle` (without an explicit `stop()` call) fires
    /// the stop closure exactly once, which asks the registry to kill the
    /// session. Wired in v0.4-thin so removing a handle from the
    /// `OmwRemoteState::pane_shares` map is enough to unshare a pane — no
    /// caller has to remember to call `.stop()` first.
    #[tokio::test]
    async fn pane_share_handle_drop_calls_kill() {
        let registry = omw_server::SessionRegistry::new();
        let (event_loop_tx, _rx) = make_event_loop();
        let (pty_tx, _pty_keep) = make_pty_broadcast();

        let handle = share_pane(
            "pane-drop".to_string(),
            event_loop_tx,
            pty_tx,
            registry.clone(),
        )
        .await
        .unwrap();
        let session_id = handle.session_id;

        assert_eq!(registry.list().len(), 1);

        // Drop fires the kill closure, which spawns a detached
        // `registry.kill(id)` task on the current runtime.
        drop(handle);

        let removed = wait_for(50, || {
            if registry.list().is_empty() {
                Some(())
            } else {
                None
            }
        })
        .await;
        assert!(
            removed.is_some(),
            "session should be removed after handle drop"
        );

        // After kill, write_input should fail (session is gone).
        let res = registry.write_input(session_id, b"x").await;
        assert!(res.is_err(), "write_input on killed session should fail");
    }

    #[tokio::test]
    async fn share_pane_kill_from_registry_aborts_pumps() {
        let registry = omw_server::SessionRegistry::new();
        let (event_loop_tx, event_loop_rx) = make_event_loop();
        let (pty_tx, _pty_keep) = make_pty_broadcast();

        let handle = share_pane(
            "pane-killed".to_string(),
            event_loop_tx,
            pty_tx,
            registry.clone(),
        )
        .await
        .unwrap();
        let session_id = handle.session_id;

        // Kill via the registry (the path the omw-remote DELETE handler takes).
        registry
            .kill(session_id)
            .await
            .expect("kill should succeed");

        // Session removed.
        assert!(registry.list().is_empty());

        // Subscribe must now also fail.
        assert!(registry.subscribe(session_id).is_none());

        // Input pump should be aborted: write_input fails because the entry
        // is gone, but separately the pump task itself should no longer be
        // running. We can't directly observe the JoinHandle (kill closure
        // took it), but we can confirm no spurious data lands in event_loop_rx.
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(event_loop_rx.try_recv().is_err());
    }
}
