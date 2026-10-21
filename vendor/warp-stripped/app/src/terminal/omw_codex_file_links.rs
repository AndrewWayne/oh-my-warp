// SPDX-License-Identifier: AGPL-3.0-only
//
// omw-authored file in the in-tree upstream fork, part of the AGPL-3.0
// derivative work. See specs/fork-strategy.md section 3.
// Copyright (C) 2026 omw contributors
// Copyright (C) 2020-2026 Denver Technologies, Inc.

//! Local Codex file-reference context. Queries run in the file-link worker, never during paint.

use diesel::{connection::SimpleConnection, Connection, RunQueryDsl, SqliteConnection};
use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use std::{
    ffi::OsString,
    fs::File,
    io::{Read, Seek, SeekFrom},
    os::unix::ffi::OsStringExt,
    path::{Path, PathBuf},
    time::Duration,
};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};
use tokio_tungstenite::{
    tungstenite::{protocol::WebSocketConfig, Message},
    WebSocketStream,
};

const MAX_PROCESSES: usize = 64;
const MAX_FDS: usize = 512;
const MAX_CONTEXT_BYTES: u64 = 1024 * 1024;

pub struct SessionTitle<'a> {
    pub prefix: &'a str,
    pub display: String,
}

/// Read the native thread-id / app-name prefix enabled in Codex's title configuration.
pub fn session_title(title: &str) -> Option<SessionTitle<'_>> {
    let (head, tail) = title.split_once(" | codex")?;
    if !tail.is_empty() && !tail.starts_with([' ', '|']) {
        return None;
    }
    let (status, identity) = head
        .rsplit_once(" | ")
        .map_or(("", head), |(status, identity)| (status, identity));
    let identity = identity.split_whitespace().next()?;
    let prefix = identity.strip_suffix("...").unwrap_or(identity);
    if !valid_session_prefix(prefix) {
        return None;
    }
    let display = tail.strip_prefix(" | ").unwrap_or(tail).trim_start();
    let display = match (status.is_empty(), display.is_empty()) {
        (true, _) => display.to_owned(),
        (false, true) => status.to_owned(),
        (false, false) => format!("{status} | {display}"),
    };
    Some(SessionTitle { prefix, display })
}

fn valid_session_prefix(prefix: &str) -> bool {
    matches!(prefix.len(), 29 | 36)
        && prefix.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)
            }
        })
}

extern "C" {
    fn omw_codex_peer_socket(pid: i32, fd: i32, path: *mut u8, capacity: usize) -> i32;
}

fn codex_socket(pid: u32) -> Result<Option<PathBuf>, ()> {
    let mut endpoint = None;
    for fd in process_fds(pid).ok_or(())? {
        if fd.proc_fdtype != libc::PROX_FDTYPE_SOCKET as u32 {
            continue;
        }
        let mut path = [0u8; 104];
        let length = unsafe {
            omw_codex_peer_socket(pid as i32, fd.proc_fd as i32, path.as_mut_ptr(), path.len())
        };
        if length < 0 {
            return Err(());
        }
        if length == 0 {
            continue;
        }
        let path = PathBuf::from(OsString::from_vec(path[..length as usize].to_owned()));
        // Codex's managed socket has a short physical path, or a named rendezvous.
        let named = path
            .file_name()
            .is_some_and(|name| name == "app-server-control.sock")
            && path
                .parent()
                .ok_or(())?
                .file_name()
                .is_some_and(|name| name == "app-server-control");
        let physical = path
            .parent()
            .ok_or(())?
            .file_name()
            .ok_or(())?
            .to_str()
            .ok_or(())?
            == format!("codex-daemon-{}", unsafe { libc::geteuid() })
            && path.file_name().ok_or(())?.to_str().ok_or(())?.len() == 64
            && path
                .file_name()
                .ok_or(())?
                .to_str()
                .ok_or(())?
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit());
        if !named && !physical {
            continue;
        }
        if endpoint.as_ref().is_some_and(|previous| previous != &path) {
            return Err(());
        }
        endpoint = Some(path);
    }
    Ok(endpoint)
}

async fn read_reply(
    socket: &mut WebSocketStream<tokio::net::UnixStream>,
    id: u64,
) -> Option<Value> {
    for _ in 0..32 {
        let message = socket.next().await?.ok()?;
        if let Message::Text(text) = message {
            let value: Value = serde_json::from_str(&text).ok()?;
            if value["id"].as_u64() == Some(id) {
                return value.get("result").cloned();
            }
        }
    }
    None
}

async fn live_directory(endpoint: &Path, prefix: &str) -> Option<PathBuf> {
    let stream = tokio::net::UnixStream::connect(endpoint).await.ok()?;
    let config = WebSocketConfig {
        max_message_size: Some(MAX_CONTEXT_BYTES as usize),
        max_frame_size: Some(MAX_CONTEXT_BYTES as usize),
        ..Default::default()
    };
    let (mut socket, _) =
        tokio_tungstenite::client_async_with_config("ws://localhost/", stream, Some(config))
            .await
            .ok()?;
    socket.send(Message::Text(json!({"id":1,"method":"initialize","params":{"clientInfo":{"name":"oh-my-warp-file-links","version":"0.0.14"},"capabilities":{}}}).to_string())).await.ok()?;
    let initialized = read_reply(&mut socket, 1).await?;
    let version = initialized["userAgent"]
        .as_str()?
        .split_once('/')?
        .1
        .split_whitespace()
        .next()?;
    let mut numbers = version.split('.');
    if numbers.next()? != "0" || numbers.next()?.parse::<u32>().ok()? < 162 {
        return None;
    }
    socket
        .send(Message::Text(json!({"method":"initialized"}).to_string()))
        .await
        .ok()?;
    socket
        .send(Message::Text(
            json!({"id":2,"method":"thread/loaded/list","params":{"limit":128}}).to_string(),
        ))
        .await
        .ok()?;
    let loaded = read_reply(&mut socket, 2).await?;
    if !loaded["nextCursor"].is_null() || loaded["data"].as_array()?.len() > 128 {
        return None;
    }
    let mut candidates = loaded["data"]
        .as_array()?
        .iter()
        .filter_map(Value::as_str)
        .filter(|id| id.starts_with(prefix));
    let id = candidates.next()?;
    if candidates.next().is_some() || id.len() != 36 || !valid_session_prefix(id) {
        return None;
    }
    socket
        .send(Message::Text(
            json!({"id":3,"method":"thread/read","params":{"threadId":id,"includeTurns":false}})
                .to_string(),
        ))
        .await
        .ok()?;
    let read = read_reply(&mut socket, 3).await?;
    let thread = &read["thread"];
    if thread["id"].as_str()? != id || thread["status"]["type"] == "notLoaded" {
        return None;
    }
    let directory = PathBuf::from(thread["cwd"].as_str()?);
    (directory.is_absolute() && directory.is_dir()).then_some(directory)
}

fn directory_from_socket(endpoint: &Path, prefix: &str) -> Option<PathBuf> {
    if !valid_session_prefix(prefix) {
        return None;
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .ok()?;
    runtime.block_on(async {
        tokio::time::timeout(Duration::from_millis(250), live_directory(endpoint, prefix))
            .await
            .ok()
            .flatten()
    })
}

#[derive(Default)]
struct LaunchOptions {
    directory: Option<PathBuf>,
    worktree: bool,
    resume: bool,
}

fn launch_options(command: &[OsString]) -> LaunchOptions {
    let mut options = LaunchOptions::default();
    let mut args = command.iter().skip(1);
    let mut session_argument = false;
    while let Some(arg) = args.next().and_then(|arg| arg.to_str()) {
        match arg {
            "--" => break,
            "resume" | "fork" if !options.resume => {
                options.resume = true;
                session_argument = true;
            }
            "--last" => session_argument = false,
            "-C" | "--cd" => options.directory = args.next().map(PathBuf::from),
            "--worktree" => options.worktree = true,
            "-c" | "--config" | "-m" | "--model" | "-p" | "--profile" | "-s" | "--sandbox"
            | "-a" | "--ask-for-approval" | "-i" | "--image" | "--add-dir" | "--enable"
            | "--disable" | "--remote" | "--remote-git-url" => {
                args.next();
            }
            text if text.starts_with("--cd=") => {
                options.directory = Some(PathBuf::from(&text[5..]));
            }
            text if text.starts_with("-C") && text.len() > 2 => {
                options.directory = Some(PathBuf::from(&text[2..]));
            }
            text if text.starts_with("--worktree=") => options.worktree = true,
            text if text.starts_with('-') => (),
            _ if session_argument => session_argument = false,
            _ => break,
        }
    }
    options
}

#[derive(diesel::QueryableByName)]
struct SessionDirectory {
    #[diesel(sql_type = diesel::sql_types::Text)]
    cwd: String,
    #[diesel(sql_type = diesel::sql_types::Text)]
    rollout_path: String,
}

fn rollout_session_id(path: &Path) -> Option<&str> {
    let name = path.file_stem()?.to_str()?.strip_prefix("rollout-")?;
    let id = name.get(name.len().checked_sub(36)?..)?;
    valid_session_prefix(id).then_some(id)
}

/// A resumed TUI can choose a new directory before another turn is written to its rollout.
/// Read only the row tied to the process's open rollout, never a "most recent" session.
pub fn directory_from_state(
    database: &Path,
    rollout: &Path,
    process_started: u64,
) -> Option<PathBuf> {
    let mut uri = url::Url::from_file_path(database).ok()?;
    uri.query_pairs_mut().append_pair("mode", "ro");
    let mut connection = SqliteConnection::establish(uri.as_str()).ok()?;
    connection
        .batch_execute("PRAGMA query_only=ON; PRAGMA busy_timeout=10;")
        .ok()?;
    let started_ms = i64::try_from(process_started.saturating_mul(1000)).ok()?;
    let mut rows: Vec<SessionDirectory> = diesel::sql_query(
        "SELECT cwd, rollout_path FROM threads WHERE rollout_path = ? AND updated_at_ms >= ? LIMIT 2",
    )
    .bind::<diesel::sql_types::Text, _>(rollout.to_str()?)
    .bind::<diesel::sql_types::BigInt, _>(started_ms)
    .load(&mut connection)
    .ok()?;
    if rows.is_empty() {
        rows = diesel::sql_query(
            "SELECT cwd, rollout_path FROM threads WHERE id = ? AND updated_at_ms >= ? LIMIT 2",
        )
        .bind::<diesel::sql_types::Text, _>(rollout_session_id(rollout)?)
        .bind::<diesel::sql_types::BigInt, _>(started_ms)
        .load(&mut connection)
        .ok()?;
        if rows.len() != 1
            || Path::new(&rows[0].rollout_path).canonicalize().ok()?
                != rollout.canonicalize().ok()?
        {
            return None;
        }
    }
    if rows.len() != 1 {
        return None;
    }
    let directory = PathBuf::from(&rows[0].cwd);
    (directory.is_absolute() && directory.is_dir()).then_some(directory)
}

pub fn directory_from_rollout(path: &Path) -> Option<PathBuf> {
    let mut file = File::open(path).ok()?;
    let length = file.metadata().ok()?.len();
    let start = length.saturating_sub(MAX_CONTEXT_BYTES);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut bytes = Vec::new();
    file.take(MAX_CONTEXT_BYTES).read_to_end(&mut bytes).ok()?;
    let first = if start > 0 {
        bytes.iter().position(|b| *b == b'\n')? + 1
    } else {
        0
    };
    let last = bytes.iter().rposition(|b| *b == b'\n')?;
    for line in bytes.get(first..last)?.split(|b| *b == b'\n').rev() {
        let Ok(record) = serde_json::from_slice::<serde_json::Value>(line) else {
            continue;
        };
        let kind = record["type"].as_str();
        if kind == Some("turn_context") || (start == 0 && kind == Some("session_meta")) {
            let directory = PathBuf::from(record["payload"]["cwd"].as_str()?);
            return (directory.is_absolute() && directory.is_dir()).then_some(directory);
        }
    }
    None
}

pub fn directory_for_process(
    cwd: Option<&Path>,
    command: &[OsString],
    rollout: Option<&Path>,
) -> Option<PathBuf> {
    let options = launch_options(command);
    if let Some(directory) = options.directory.filter(|_| !options.worktree) {
        let directory = if directory.is_absolute() {
            directory
        } else {
            cwd?.join(directory)
        };
        return directory.is_dir().then_some(directory);
    }
    directory_from_rollout(rollout?)
}

fn children(pid: u32) -> Option<Vec<Pid>> {
    let mut buffer = [0u32; MAX_PROCESSES];
    // PROC_PPID_ONLY from sys/proc_info.h; libc omits this selector.
    let bytes = unsafe {
        libc::proc_listpids(
            6,
            pid,
            buffer.as_mut_ptr().cast(),
            std::mem::size_of_val(&buffer) as i32,
        )
    };
    if bytes < 0
        || bytes as usize >= std::mem::size_of_val(&buffer)
        || bytes as usize % std::mem::size_of::<u32>() != 0
    {
        return None;
    }
    Some(
        buffer[..bytes as usize / std::mem::size_of::<u32>()]
            .iter()
            .copied()
            .filter(|pid| *pid != 0)
            .map(Pid::from_u32)
            .collect(),
    )
}

// These SDK structures are missing from libc. Keep the C layout from sys/proc_info.h.
#[repr(C)]
struct ProcFileInfo {
    open_flags: u32,
    status: u32,
    offset: i64,
    kind: i32,
    guard_flags: u32,
}
#[repr(C)]
struct VnodeFdInfoWithPath {
    file: ProcFileInfo,
    vnode: libc::vnode_info_path,
}

#[derive(Default)]
struct OpenContext {
    rollout: Option<PathBuf>,
    state: Option<PathBuf>,
}

impl OpenContext {
    fn owns_session(&self, prefix: &str) -> bool {
        self.rollout
            .as_deref()
            .and_then(rollout_session_id)
            .is_some_and(|id| id.starts_with(prefix))
    }
}

fn process_fds(pid: u32) -> Option<Vec<libc::proc_fdinfo>> {
    let mut fds: Vec<libc::proc_fdinfo> = (0..MAX_FDS)
        .map(|_| unsafe { std::mem::zeroed() })
        .collect();
    let size = fds.len() * std::mem::size_of::<libc::proc_fdinfo>();
    let bytes = unsafe {
        libc::proc_pidinfo(
            pid as i32,
            libc::PROC_PIDLISTFDS,
            0,
            fds.as_mut_ptr().cast(),
            size as i32,
        )
    };
    if bytes <= 0
        || bytes as usize >= size
        || bytes as usize % std::mem::size_of::<libc::proc_fdinfo>() != 0
    {
        return None;
    }
    fds.truncate(bytes as usize / std::mem::size_of::<libc::proc_fdinfo>());
    Some(fds)
}

fn open_context(pid: u32) -> Option<OpenContext> {
    let mut context = OpenContext::default();
    for fd in process_fds(pid)? {
        if fd.proc_fdtype != libc::PROX_FDTYPE_VNODE as u32 {
            continue;
        }
        let mut info: VnodeFdInfoWithPath = unsafe { std::mem::zeroed() };
        let size = std::mem::size_of_val(&info);
        // PROC_PIDFDVNODEPATHINFO = 2, from the same SDK header.
        if unsafe {
            libc::proc_pidfdinfo(
                pid as i32,
                fd.proc_fd as i32,
                2,
                (&mut info as *mut VnodeFdInfoWithPath).cast(),
                size as i32,
            )
        } != size as i32
        {
            continue;
        }
        let bytes: Vec<u8> = info
            .vnode
            .vip_path
            .iter()
            .flatten()
            .copied()
            .take_while(|c| *c != 0)
            .map(|c| c as u8)
            .collect();
        let path = PathBuf::from(OsString::from_vec(bytes));
        // proc_fileinfo uses the SDK's FWRITE bit, not open(2)'s O_* access mode.
        if info.file.open_flags & 0x2 != 0
            && path.extension().is_some_and(|s| s == "jsonl")
            && path
                .file_name()
                .and_then(|s| s.to_str())
                .is_some_and(|s| s.starts_with("rollout-"))
        {
            if context
                .rollout
                .as_ref()
                .is_some_and(|existing| existing != &path)
            {
                return None;
            }
            context.rollout = Some(path);
        } else if path.file_name().and_then(|s| s.to_str()).is_some_and(|s| {
            s.strip_prefix("state_")
                .and_then(|s| s.strip_suffix(".sqlite"))
                .is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
        }) {
            if context
                .state
                .as_ref()
                .is_some_and(|existing| existing != &path)
            {
                return None;
            }
            context.state = Some(path);
        }
    }
    Some(context)
}

pub fn working_directory_for_shell_pid(shell_pid: u32) -> Option<PathBuf> {
    working_directory_for_shell_pid_and_title(shell_pid, None)
}

pub fn working_directory_for_shell_pid_and_title(
    shell_pid: u32,
    session_prefix: Option<&str>,
) -> Option<PathBuf> {
    let mut system = System::new();
    let mut pending = vec![Pid::from_u32(shell_pid)];
    let mut visited = Vec::new();
    let mut codex = None;
    for depth in 0..8 {
        system.refresh_processes_specifics(
            ProcessesToUpdate::Some(&pending),
            true,
            ProcessRefreshKind::nothing()
                .with_cmd(UpdateKind::Always)
                .with_cwd(UpdateKind::Always),
        );
        let mut next = Vec::new();
        for pid in pending {
            if visited.contains(&pid) {
                continue;
            }
            visited.push(pid);
            if visited.len() > MAX_PROCESSES {
                return None;
            }
            let process = system.process(pid)?;
            if process.name() == "codex" {
                if codex.replace(pid).is_some() {
                    return None;
                }
            } else {
                next.extend(children(pid.as_u32())?);
            }
        }
        if next.is_empty() {
            break;
        }
        if depth == 7 || next.len() + visited.len() > MAX_PROCESSES {
            return None;
        }
        pending = next;
    }
    let process = system.process(codex?)?;
    let context = open_context(process.pid().as_u32())?;
    if let Some(prefix) = session_prefix {
        if process
            .cmd()
            .iter()
            .filter_map(|arg| arg.to_str())
            .any(|arg| arg == "--remote" || arg.starts_with("--remote="))
        {
            return None;
        }
        if let Some(endpoint) = codex_socket(process.pid().as_u32()).ok()? {
            let directory = directory_from_socket(&endpoint, prefix)?;
            // Do not deliver context after this client exited or reconnected elsewhere.
            return (codex_socket(process.pid().as_u32()).ok()?.as_ref() == Some(&endpoint))
                .then_some(directory);
        }
        // Embedded clients retain the previous process-owned context resolver.
        // A shared client with unavailable metadata must not fall back to launch -C.
        if !context.owns_session(prefix) {
            return None;
        }
        let rollout = context.rollout.as_deref()?;
        return match context.state.as_deref() {
            Some(database) => directory_from_state(database, rollout, process.start_time()),
            None => directory_from_rollout(rollout),
        };
    }
    let options = launch_options(process.cmd());
    if options.resume && options.directory.is_none() {
        // The saved turn context can be stale after the interactive directory choice.
        return directory_from_state(
            context.state.as_deref()?,
            context.rollout.as_deref()?,
            process.start_time(),
        );
    }
    directory_for_process(process.cwd(), process.cmd(), context.rollout.as_deref())
}

#[cfg(test)]
mod title_tests {
    use super::*;

    const ID: &str = "01a1174d-c460-7463-9783-8942cce541aa";

    #[test]
    fn owned_rollout_state_accepts_a_symlink_alias_but_not_a_different_file() {
        let directory = tempfile::tempdir().unwrap();
        let actual = directory
            .path()
            .join(format!("rollout-2026-10-16T01-33-46-{ID}.jsonl"));
        std::fs::write(&actual, "").unwrap();
        let alias = directory.path().join("alias.jsonl");
        std::os::unix::fs::symlink(&actual, &alias).unwrap();
        let database = directory.path().join("state_5.sqlite");
        let mut connection = SqliteConnection::establish(database.to_str().unwrap()).unwrap();
        connection.batch_execute("CREATE TABLE threads (id TEXT, rollout_path TEXT, cwd TEXT, updated_at_ms INTEGER);").unwrap();
        diesel::sql_query("INSERT INTO threads VALUES (?, ?, ?, 2000)")
            .bind::<diesel::sql_types::Text, _>(ID)
            .bind::<diesel::sql_types::Text, _>(alias.to_str().unwrap())
            .bind::<diesel::sql_types::Text, _>(directory.path().to_str().unwrap())
            .execute(&mut connection)
            .unwrap();
        assert_eq!(
            directory_from_state(&database, &actual, 1),
            Some(directory.path().to_owned())
        );
        assert!(directory_from_state(&database, &actual, 3).is_none());
        std::fs::remove_file(&alias).unwrap();
        std::fs::write(&alias, "different file").unwrap();
        assert!(directory_from_state(&database, &actual, 1).is_none());
    }

    #[test]
    fn embedded_fallback_requires_the_current_threads_owned_rollout() {
        let mut context = OpenContext {
            rollout: None,
            state: Some(PathBuf::from("/tmp/state_5.sqlite")),
        };
        assert!(!context.owns_session(&ID[..29]));
        context.rollout = Some(PathBuf::from(
            "/tmp/rollout-2026-10-16T01-33-46-01a1174d-c460-7463-9783-000000000000.jsonl",
        ));
        assert!(!context.owns_session(&ID[..29]));
        context.rollout = Some(PathBuf::from(format!(
            "/tmp/rollout-2026-10-16T01-33-46-{ID}.jsonl"
        )));
        assert!(context.owns_session(&ID[..29]));
        assert!(context.owns_session(ID));
    }

    #[test]
    fn read_only_rollout_previews_do_not_establish_session_ownership() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory
            .path()
            .join(format!("rollout-2026-10-16T01-33-46-{ID}.jsonl"));
        std::fs::write(&path, "").unwrap();
        let reader = File::open(&path).unwrap();
        assert!(open_context(std::process::id()).unwrap().rollout.is_none());
        drop(reader);
        let _writer = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
        assert_eq!(
            open_context(std::process::id()).unwrap().rollout,
            Some(path.canonicalize().unwrap())
        );
    }

    #[test]
    fn current_thread_metadata_is_hidden_without_changing_the_original_title() {
        for identity in [ID.to_owned(), format!("{}...", &ID[..29])] {
            let title = format!("{identity} | codex | original title");
            let parsed = session_title(&title).unwrap();
            assert!(ID.starts_with(parsed.prefix));
            assert_eq!(parsed.display, "original title");
        }
        let title = format!("Action required | {}... [1/3] | codex spinner", &ID[..29]);
        assert_eq!(
            session_title(&title).unwrap().display,
            "Action required | spinner"
        );
        for title in [
            "codex | workspace",
            "not-a-thread | codex | title",
            "01a1174d | codex",
            "01a1174d-c460-7463-9783-8942c... | codex-fake",
        ] {
            assert!(session_title(title).is_none(), "{title}");
        }
    }

    #[test]
    fn live_directory_is_read_from_the_exact_loaded_thread_without_subscribing() {
        use std::os::unix::net::UnixListener;
        use tokio_tungstenite::tungstenite::accept;

        for collision in [false, true] {
            let dir = tempfile::tempdir_in("/tmp").unwrap();
            let endpoint = dir.path().join("server.sock");
            let listener = UnixListener::bind(&endpoint).unwrap();
            let expected = dir.path().to_owned();
            let server_directory = expected.clone();
            let server = std::thread::spawn(move || {
                let (stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(1)))
                    .unwrap();
                let mut socket = accept(stream).unwrap();
                let read_request = |socket: &mut tokio_tungstenite::tungstenite::WebSocket<
                    std::os::unix::net::UnixStream,
                >| {
                    serde_json::from_str::<Value>(&socket.read().unwrap().into_text().unwrap())
                        .unwrap()
                };
                assert_eq!(read_request(&mut socket)["method"], "initialize");
                socket.send(Message::Text(json!({"id":1,"result":{"userAgent":"oh-my-warp-file-links/0.162.1 (Mac OS)"}}).to_string())).unwrap();
                assert_eq!(read_request(&mut socket)["method"], "initialized");
                assert_eq!(read_request(&mut socket)["method"], "thread/loaded/list");
                let ids = if collision {
                    vec![ID.to_owned(), format!("{}0000000", &ID[..29])]
                } else {
                    vec!["01a1174d-c460-7463-9783-000000000000".into(), ID.into()]
                };
                socket
                    .send(Message::Text(
                        json!({"id":2,"result":{"data":ids,"nextCursor":null}}).to_string(),
                    ))
                    .unwrap();
                if !collision {
                    let request = read_request(&mut socket);
                    assert_eq!(request["method"], "thread/read");
                    assert_eq!(
                        request["params"],
                        json!({"threadId":ID,"includeTurns":false})
                    );
                    socket.send(Message::Text(json!({"id":3,"result":{"thread":{"id":ID,"cwd":server_directory,"status":{"type":"idle"}}}}).to_string())).unwrap();
                }
            });
            assert_eq!(
                directory_from_socket(&endpoint, &ID[..29]),
                (!collision).then_some(expected)
            );
            server.join().unwrap();
        }
    }

    #[test]
    fn socket_discovery_uses_a_connected_peer_and_refuses_multiple_servers() {
        use std::os::unix::net::{UnixListener, UnixStream};
        let first = tempfile::tempdir_in("/tmp").unwrap();
        let second = tempfile::tempdir_in("/tmp").unwrap();
        let path = |directory: &Path| {
            let parent = directory.join("app-server-control");
            std::fs::create_dir(&parent).unwrap();
            parent.join("app-server-control.sock")
        };
        let first_path = path(first.path());
        let first_listener = UnixListener::bind(&first_path).unwrap();
        let first_client = UnixStream::connect(&first_path).unwrap();
        assert_eq!(codex_socket(std::process::id()).unwrap(), Some(first_path));
        let second_path = path(second.path());
        let second_listener = UnixListener::bind(&second_path).unwrap();
        let second_client = UnixStream::connect(&second_path).unwrap();
        assert!(codex_socket(std::process::id()).is_err());
        drop((first_listener, first_client, second_listener, second_client));
    }

    #[test]
    fn unavailable_or_silent_servers_do_not_resolve_a_relative_directory() {
        assert!(directory_from_socket(Path::new("/tmp/omw-absent-socket"), ID).is_none());
        assert!(directory_from_socket(Path::new("/tmp/omw-absent-socket"), "invalid").is_none());
        let dir = tempfile::tempdir_in("/tmp").unwrap();
        let endpoint = dir.path().join("silent.sock");
        let listener = std::os::unix::net::UnixListener::bind(&endpoint).unwrap();
        let started = std::time::Instant::now();
        assert!(directory_from_socket(&endpoint, ID).is_none());
        assert!(started.elapsed() < Duration::from_secs(1));
        drop(listener);
    }
}
