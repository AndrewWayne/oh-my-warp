//! `PtyCommand` — describes the child process to launch in a PTY.
//!
//! This is a small builder over the inputs `portable_pty::CommandBuilder`
//! ultimately needs (program, argv, env, cwd) plus the initial PTY size.

use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

/// Initial PTY window size, in character cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PtySize {
    pub cols: u16,
    pub rows: u16,
}

impl Default for PtySize {
    fn default() -> Self {
        Self { cols: 80, rows: 24 }
    }
}

/// Description of a child process to spawn under a PTY.
///
/// Use [`PtyCommand::new`] to start, then chain `.arg`, `.env`, `.cwd`,
/// `.size` as needed.
#[derive(Debug, Clone)]
pub struct PtyCommand {
    pub program: OsString,
    pub args: Vec<OsString>,
    /// Environment overrides. Treated as ADDITIONS to the parent env at spawn
    /// time (the Executor decides whether to inherit or scrub the parent env;
    /// for the v1 wrapper, additions are merged on top of the parent env).
    pub envs: BTreeMap<OsString, OsString>,
    pub cwd: Option<PathBuf>,
    pub size: PtySize,
}

impl PtyCommand {
    /// Construct a new command for `program`, with no args and default size 80x24.
    pub fn new(program: impl Into<OsString>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            envs: BTreeMap::new(),
            cwd: None,
            size: PtySize::default(),
        }
    }

    pub fn arg(mut self, arg: impl Into<OsString>) -> Self {
        self.args.push(arg.into());
        self
    }

    pub fn args<I, S>(mut self, args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<OsString>,
    {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    pub fn env(mut self, key: impl AsRef<OsStr>, value: impl AsRef<OsStr>) -> Self {
        self.envs
            .insert(key.as_ref().to_os_string(), value.as_ref().to_os_string());
        self
    }

    pub fn cwd(mut self, dir: impl Into<PathBuf>) -> Self {
        self.cwd = Some(dir.into());
        self
    }

    pub fn size(mut self, cols: u16, rows: u16) -> Self {
        self.size = PtySize { cols, rows };
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_size_is_80x24() {
        let s = PtySize::default();
        assert_eq!(s.cols, 80);
        assert_eq!(s.rows, 24);
    }

    #[test]
    fn new_starts_with_no_args_no_env_no_cwd_default_size() {
        let c = PtyCommand::new("bash");
        assert_eq!(c.program, OsString::from("bash"));
        assert!(c.args.is_empty());
        assert!(c.envs.is_empty());
        assert!(c.cwd.is_none());
        assert_eq!(c.size, PtySize::default());
    }

    #[test]
    fn arg_is_chainable_and_appends() {
        let c = PtyCommand::new("sh").arg("-c").arg("echo hi");
        assert_eq!(
            c.args,
            vec![OsString::from("-c"), OsString::from("echo hi")]
        );
    }

    #[test]
    fn args_extends_in_order() {
        let c = PtyCommand::new("sh").args(["-c", "true"]);
        assert_eq!(c.args, vec![OsString::from("-c"), OsString::from("true")]);
    }

    #[test]
    fn env_inserts_pairs() {
        let c = PtyCommand::new("sh").env("FOO", "1").env("BAR", "two");
        assert_eq!(
            c.envs.get(OsStr::new("FOO")),
            Some(&OsString::from("1"))
        );
        assert_eq!(
            c.envs.get(OsStr::new("BAR")),
            Some(&OsString::from("two"))
        );
    }

    #[test]
    fn env_last_write_wins() {
        let c = PtyCommand::new("sh").env("FOO", "1").env("FOO", "2");
        assert_eq!(
            c.envs.get(OsStr::new("FOO")),
            Some(&OsString::from("2"))
        );
        assert_eq!(c.envs.len(), 1);
    }

    #[test]
    fn cwd_sets_dir() {
        let c = PtyCommand::new("sh").cwd("/tmp");
        assert_eq!(c.cwd.as_deref(), Some(std::path::Path::new("/tmp")));
    }

    #[test]
    fn size_overrides_default() {
        let c = PtyCommand::new("sh").size(132, 50);
        assert_eq!(c.size, PtySize { cols: 132, rows: 50 });
    }

    #[test]
    fn builder_is_fully_chainable() {
        let c = PtyCommand::new("bash")
            .arg("-lc")
            .arg("echo hi")
            .env("LANG", "C")
            .cwd("/")
            .size(100, 30);
        assert_eq!(c.program, OsString::from("bash"));
        assert_eq!(c.args.len(), 2);
        assert_eq!(c.envs.len(), 1);
        assert!(c.cwd.is_some());
        assert_eq!(c.size, PtySize { cols: 100, rows: 30 });
    }
}
