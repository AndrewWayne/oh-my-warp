//! `omw-keychain-helper` — binary entrypoint.
//!
//! Thin shim over `omw_keychain_helper::run`. All business logic lives in
//! the library so that in-process tests can capture stdout/stderr.

use std::collections::HashMap;
use std::io::{self, Write};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let envs: HashMap<String, String> = std::env::vars().collect();
    let stdout = io::stdout();
    let stderr = io::stderr();
    let mut stdout_lock = stdout.lock();
    let mut stderr_lock = stderr.lock();
    let code = omw_keychain_helper::run(&args, &envs, &mut stdout_lock, &mut stderr_lock);
    let _ = stdout_lock.flush();
    let _ = stderr_lock.flush();
    std::process::exit(code);
}
