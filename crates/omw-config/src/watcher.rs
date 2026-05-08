//! File-watching loader.
//!
//! Wraps `notify` with a small debounce loop so editors that write via
//! temp-file-and-rename don't cause spurious reloads. Re-reads + re-validates
//! the config on each settled change and pushes the result onto a
//! `tokio::sync::watch` channel. Consumers can poll synchronously
//! ([`WatchHandle::current`], [`WatchHandle::has_changed`]) or `await` updates
//! once a tokio runtime is available later.

use std::path::{Path, PathBuf};
use std::sync::mpsc::RecvTimeoutError;
use std::sync::Arc;
use std::time::{Duration, Instant};

use notify::Watcher;

use crate::error::ConfigError;
use crate::schema::Config;

/// One snapshot delivered by the watcher. Wrapped in `Arc` so receivers can
/// clone cheaply; wrapping in `Result` lets a transient parse error surface
/// without tearing down the watcher.
pub type ConfigUpdate = Arc<Result<Config, ConfigError>>;

const DEFAULT_DEBOUNCE: Duration = Duration::from_millis(250);

/// Handle returned from [`watch`]. Drop to stop watching.
pub struct WatchHandle {
    receiver: tokio::sync::watch::Receiver<ConfigUpdate>,
    // Held to keep the OS watcher (and thus the worker thread) alive for as
    // long as the handle exists. Order in this struct controls drop order.
    _watcher: notify::RecommendedWatcher,
}

impl WatchHandle {
    /// Borrow the most recent config snapshot.
    pub fn current(&self) -> ConfigUpdate {
        self.receiver.borrow().clone()
    }

    /// Sync poll: has a new snapshot arrived since the last `borrow_and_update`?
    pub fn has_changed(&self) -> bool {
        self.receiver.has_changed().unwrap_or(false)
    }

    /// Borrow the latest snapshot and mark it as seen.
    pub fn borrow_and_update(&mut self) -> ConfigUpdate {
        self.receiver.borrow_and_update().clone()
    }

    /// Borrow the underlying receiver for callers that want to integrate
    /// directly with tokio.
    pub fn receiver(&self) -> tokio::sync::watch::Receiver<ConfigUpdate> {
        self.receiver.clone()
    }
}

/// Watch `path` for changes. Equivalent to [`watch_with_debounce`] with the
/// default 250 ms debounce window.
pub fn watch(path: PathBuf) -> Result<WatchHandle, ConfigError> {
    watch_with_debounce(path, DEFAULT_DEBOUNCE)
}

/// Watch `path` with an explicit debounce window. Useful in tests, or for
/// consumers that need a different cadence.
pub fn watch_with_debounce(path: PathBuf, debounce: Duration) -> Result<WatchHandle, ConfigError> {
    let initial: ConfigUpdate = Arc::new(Config::load_from(&path));
    let (tx, rx) = tokio::sync::watch::channel(initial);

    let (events_tx, events_rx) = std::sync::mpsc::channel::<notify::Result<notify::Event>>();
    let mut watcher = notify::recommended_watcher(move |res| {
        let _ = events_tx.send(res);
    })?;

    // Watch the parent directory so we catch atomic-rename writes (vim, sed -i,
    // editors that use a tempfile-and-replace pattern).
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    watcher.watch(&parent, notify::RecursiveMode::NonRecursive)?;

    // Canonicalize the target path so `is_relevant` compares apples to
    // apples. macOS reports FSEvents paths under their canonical form
    // (e.g. /private/var/folders/... rather than /var/folders/...), and
    // `tempfile::tempdir()` returns the unresolved form. A literal `==`
    // on Path drops every event without a match. Falling back to the
    // unresolved path when canonicalize fails (e.g. the target file
    // doesn't exist yet) preserves the previous behavior for the
    // create-on-write case — once the file appears, parent canonicalize
    // below covers it.
    let canonical_target = match std::fs::canonicalize(&path) {
        Ok(p) => p,
        Err(_) => match std::fs::canonicalize(&parent) {
            Ok(parent_canon) => path
                .file_name()
                .map(|name| parent_canon.join(name))
                .unwrap_or_else(|| path.clone()),
            Err(_) => path.clone(),
        },
    };
    std::thread::Builder::new()
        .name("omw-config-watcher".into())
        .spawn(move || run_watcher_thread(events_rx, tx, canonical_target, debounce))
        .map_err(|e| ConfigError::PathResolution(format!("failed to spawn watcher thread: {e}")))?;

    Ok(WatchHandle {
        receiver: rx,
        _watcher: watcher,
    })
}

fn run_watcher_thread(
    events_rx: std::sync::mpsc::Receiver<notify::Result<notify::Event>>,
    tx: tokio::sync::watch::Sender<ConfigUpdate>,
    path: PathBuf,
    debounce: Duration,
) {
    loop {
        // Block until at least one event arrives. Disconnect = the notify
        // watcher (and thus the WatchHandle) was dropped — we exit cleanly.
        let first = match events_rx.recv() {
            Ok(ev) => ev,
            Err(_) => return,
        };
        if !is_relevant(&first, &path) {
            continue;
        }

        // Debounce: drain any further events that arrive within the window.
        let deadline = Instant::now() + debounce;
        loop {
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .unwrap_or_default();
            match events_rx.recv_timeout(remaining) {
                Ok(_) => continue,
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }

        let snapshot = Arc::new(Config::load_from(&path));
        // send_replace would unconditionally overwrite; send fails if all
        // receivers were dropped, in which case we have no readers — exit.
        if tx.send(snapshot).is_err() {
            return;
        }
    }
}

/// Filter notify events down to ones that touched our target file. Watching
/// the parent directory means we see siblings' events too; this drops them.
fn is_relevant(event: &notify::Result<notify::Event>, target: &Path) -> bool {
    match event {
        Ok(ev) => ev.paths.iter().any(|p| p == target),
        // Pass errors through so the worker can re-read and surface them.
        Err(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, body: &str) {
        std::fs::write(path, body).unwrap();
    }

    /// Wait up to `timeout` for the watcher to surface a snapshot satisfying
    /// `pred`. Polling — sync, no runtime needed.
    fn wait_for<F>(handle: &mut WatchHandle, timeout: Duration, mut pred: F)
    where
        F: FnMut(&ConfigUpdate) -> bool,
    {
        let deadline = Instant::now() + timeout;
        loop {
            if handle.has_changed() {
                let snap = handle.borrow_and_update();
                if pred(&snap) {
                    return;
                }
            }
            if Instant::now() > deadline {
                panic!("watcher timed out after {timeout:?}");
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn picks_up_in_place_edits() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        write(
            &path,
            r#"[providers.ollama-local]
kind = "ollama"
"#,
        );

        let mut handle = watch_with_debounce(path.clone(), Duration::from_millis(50)).unwrap();
        // Initial snapshot is loaded eagerly.
        let initial = handle.current();
        let cfg = initial.as_ref().as_ref().expect("initial load ok");
        assert!(cfg.default_provider.is_none());

        // Edit and assert we eventually see the new state.
        write(
            &path,
            r#"default_provider = "ollama-local"

[providers.ollama-local]
kind = "ollama"
default_model = "llama3.1:8b"
"#,
        );

        wait_for(&mut handle, Duration::from_secs(3), |snap| {
            snap.as_ref()
                .as_ref()
                .map(|c| c.default_provider.is_some())
                .unwrap_or(false)
        });
    }

    #[test]
    fn surfaces_parse_error_without_tearing_down_watcher() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        write(
            &path,
            r#"[providers.ollama-local]
kind = "ollama"
"#,
        );

        let mut handle = watch_with_debounce(path.clone(), Duration::from_millis(50)).unwrap();
        assert!(handle.current().as_ref().is_ok());

        // Break it.
        write(&path, "this is = =not valid toml");
        wait_for(&mut handle, Duration::from_secs(3), |snap| {
            matches!(snap.as_ref(), Err(ConfigError::Parse { .. }))
        });

        // Recover.
        write(
            &path,
            r#"[providers.fixed]
kind = "ollama"
"#,
        );
        wait_for(&mut handle, Duration::from_secs(3), |snap| {
            snap.as_ref()
                .as_ref()
                .map(|c| !c.providers.is_empty())
                .unwrap_or(false)
        });
    }
}
