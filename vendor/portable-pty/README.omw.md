# portable-pty 0.8.1 local patch

This directory contains the functional source from the published
`portable-pty` 0.8.1 crate. Package-only `.cargo_vcs_info.json` metadata and
the crate's standalone `Cargo.lock` are omitted because this copy is consumed
as a workspace path dependency.

- Crate checksum: `806ee80c2a03dbe1a9fb9534f8d19e4c0546b790cde8fd1fea9d6390644cb0be`
- Upstream repository: `https://github.com/wez/wezterm`
- License: MIT; see `LICENSE.md`

## Local delta

The Windows child-kill paths follow Win32 `TerminateProcess` semantics:
nonzero is success, while zero is failure and requires `GetLastError`.
