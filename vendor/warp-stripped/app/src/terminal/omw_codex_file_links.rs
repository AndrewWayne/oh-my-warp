// SPDX-License-Identifier: AGPL-3.0-only
//
// omw-authored file in the in-tree upstream fork, part of the AGPL-3.0
// derivative work. See specs/fork-strategy.md section 3.
// Copyright (C) 2026 omw contributors
// Copyright (C) 2020-2026 Denver Technologies, Inc.

//! Local Codex file-reference context. Queries run in the file-link worker, never during paint.

use diesel::{connection::SimpleConnection, Connection, RunQueryDsl, SqliteConnection};
use std::{
    ffi::OsString,
    fs::File,
    io::{Read, Seek, SeekFrom},
    os::unix::ffi::OsStringExt,
    path::{Path, PathBuf},
};
use sysinfo::{Pid, ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

const MAX_PROCESSES: usize = 64;
const MAX_FDS: usize = 512;
const MAX_CONTEXT_BYTES: u64 = 1024 * 1024;

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
    let rows: Vec<SessionDirectory> = diesel::sql_query(
        "SELECT cwd FROM threads WHERE rollout_path = ? AND updated_at_ms >= ? LIMIT 2",
    )
    .bind::<diesel::sql_types::Text, _>(rollout.to_str()?)
    .bind::<diesel::sql_types::BigInt, _>(i64::try_from(process_started.saturating_mul(1000)).ok()?)
    .load(&mut connection)
    .ok()?;
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

fn open_context(pid: u32) -> Option<OpenContext> {
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
    let mut context = OpenContext::default();
    for fd in &fds[..bytes as usize / std::mem::size_of::<libc::proc_fdinfo>()] {
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
        if path.extension().is_some_and(|s| s == "jsonl")
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
    let options = launch_options(process.cmd());
    let context = open_context(process.pid().as_u32())?;
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
