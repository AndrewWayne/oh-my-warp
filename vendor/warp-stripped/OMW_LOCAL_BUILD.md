# Build and Run the Stripped Warp Fork

This document covers building and running the stripped Warp fork in `vendor/warp-stripped`. The validated executable is **`warp-oss`**, built with Cargo features `--no-default-features --features omw_local`.

## What this build is

`vendor/warp-stripped/` is an in-tree fork of `warpdotdev/warp` with the omw "local mode" patch series applied. Building with `--no-default-features --features omw_local` produces a binary that:

- Has **no Warp-cloud user surfaces.** No signup wall on first launch, no AI panel sign-in, no Drive / Account / Billing / Team / Referral settings tabs, no Oz cloud-agents UI, no voice-input UI.
- Has **no callable Warp-cloud endpoints.** All hostnames in PRD §3.1's audit contract (`app.warp.dev`, `api.warp.dev`, `cloud.warp.dev`, `oz.warp.dev`, `firebase.googleapis.com`, `firebaseio.com`, `identitytoolkit.googleapis.com`, `securetoken.googleapis.com`) are absent from the binary's `.rodata`. Verified by `scripts/audit-no-cloud.sh`.
- **Still runs as a working terminal.** Local terminal, command palette, settings (the local-only tabs), agent panel placeholder, completer, file-tree, code editor — all functional.

The default cloud build (`cargo build -p warp --bin warp-oss` without `--no-default-features`) also still works as a regression guardrail.

## What's tested

This document is validated for **macOS aarch64-apple-darwin (Apple Silicon)**. The Cargo invocation is identical on Windows and Intel Macs; only the prerequisites differ. Linux is not validated.

---

## macOS

### Prerequisites

- **Full Xcode** (Mac App Store), not just Command Line Tools. The Metal shader compiler `metal` is invoked by `crates/warpui/build.rs` and only ships inside Xcode.
- **Homebrew** with `protobuf`: `brew install protobuf`.
- **`rustup`, `cargo`, `rustc`**. The pinned toolchain (Rust `1.92.0`, see `rust-toolchain.toml`) auto-installs on first `cargo` invocation inside `vendor/warp-stripped/`.
- macOS native targets (`aarch64-apple-darwin` / `x86_64-apple-darwin`) ship with the toolchain — no `rustup target add` step needed.

### One-time setup

```bash
# Make sure cargo is on PATH for this shell.
. "$HOME/.cargo/env"

# Point xcrun at the full Xcode toolchain, not /Library/Developer/CommandLineTools.
sudo xcode-select -s /Applications/Xcode.app/Contents/Developer

# Accept the Xcode license. Required before xcrun will run any tool.
sudo xcodebuild -license accept

# Sanity check: the metal compiler must resolve.
xcrun --find metal
```

### Build

```bash
export PROTOC=/opt/homebrew/bin/protoc        # /usr/local/bin/protoc on Intel Macs
cd vendor/warp-stripped
cargo build -p warp --bin warp-oss --no-default-features --features omw_local
```

Output binary:

```
vendor/warp-stripped/target/debug/warp-oss
```

First build downloads + compiles a large Rust dependency tree (15+ minutes on Apple Silicon). Incremental builds after that are ~30 seconds–2 minutes.

### Run

```bash
cd vendor/warp-stripped
./target/debug/warp-oss
```

Or via Cargo (slower, rebuilds if anything changed):

```bash
cd vendor/warp-stripped
cargo run -p warp --bin warp-oss --no-default-features --features omw_local
```

### Verify the strip

After a fresh build, run the binary audit:

```bash
cd vendor/warp-stripped
bash scripts/audit-no-cloud.sh target/debug/warp-oss
```

Expected output:

```
app.warp.dev                             0
api.warp.dev                             0
cloud.warp.dev                           0
oz.warp.dev                              0
firebase.googleapis.com                  0
firebaseio.com                           0
identitytoolkit.googleapis.com           0
securetoken.googleapis.com               0
audit-no-cloud: OK
```

Any non-zero count means a forbidden hostname leaked back into the binary.

### Default cloud build (regression check)

To confirm the default cloud build still works (without `--no-default-features`, i.e. with the `cloud` feature on):

```bash
cd vendor/warp-stripped
cargo check -p warp --bin warp-oss
```

This is the upstream-equivalent build. The audit script will fail on this binary (cloud crates intentionally linked), which is expected.

---

## Windows

The Cargo invocation is identical to macOS; the deltas are toolchain setup.

### Prerequisites

- `rustup`, `cargo`, `rustc`
- The pinned toolchain (`1.92.0`)
- Rust target `x86_64-pc-windows-msvc` for that toolchain
- `protoc` available through the `PROTOC` environment variable

### One-time setup (PowerShell)

```powershell
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"

rustup target add x86_64-pc-windows-msvc --toolchain 1.92.0-x86_64-pc-windows-msvc

# Download protoc into a local tools dir if not already there
$toolsDir = Join-Path $PWD '.tmp\tools'
$protocDir = Join-Path $toolsDir 'protoc-29.3'
$protocExe = Join-Path $protocDir 'bin\protoc.exe'
if (-not (Test-Path $protocExe)) {
    New-Item -ItemType Directory -Force -Path $toolsDir | Out-Null
    $zipPath = Join-Path $toolsDir 'protoc-29.3-win64.zip'
    Invoke-WebRequest `
        -Uri 'https://github.com/protocolbuffers/protobuf/releases/download/v29.3/protoc-29.3-win64.zip' `
        -OutFile $zipPath
    if (Test-Path $protocDir) { Remove-Item $protocDir -Recurse -Force }
    Expand-Archive -Path $zipPath -DestinationPath $protocDir
}

$env:PROTOC = "$PWD\.tmp\tools\protoc-29.3\bin\protoc.exe"
```

### Build & run

```powershell
Set-Location vendor\warp-stripped
cargo build -p warp --bin warp-oss --no-default-features --features omw_local
cargo run -p warp --bin warp-oss --no-default-features --features omw_local
```

Output: `vendor\warp-stripped\target\debug\warp-oss.exe`.

A helper script lives at `vendor/warp-stripped/run-omw-local.ps1` (modes: `-BuildOnly`, `-BinaryOnly`).

---

## Cargo feature design

`vendor/warp-stripped/app/Cargo.toml` defines:

```
default     = ["omw_default", "cloud"]
omw_default = [...all UI/agent/settings flags shipped by upstream...]
omw_local   = ["omw_default", "warp_core/omw_local"]
cloud       = ["warp-command-signatures/embed-signatures"]
```

So:

- `cargo build` (default) → `omw_default` + `cloud` → cloud feature enables embedded signature data and is required by upstream-equivalent behavior.
- `cargo build --no-default-features --features omw_local` → `omw_default` (same UI/agent flags) + `omw_local` (URL-string `#[cfg]`-strips) − `cloud`.

**Why both share `omw_default`:** without it, an `--no-default-features --features omw_local` build is missing ~165 feature flags. Menu items in `app/src/app_menus.rs` reference custom actions (e.g. `ToggleGlobalSearch`) whose descriptions are only registered when their feature flag is enabled. A missing description fires `debug_assert!(false, "action should have a name: ...")` at startup — a silent panic visible only in the Warp log file (see Troubleshooting below).

---

## Troubleshooting

### `cargo` is not recognized (Windows) / not on PATH

```powershell
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
```

```bash
. "$HOME/.cargo/env"
```

### `xcrun: error: unable to find utility "metal"` (macOS)

`xcode-select` is pointing at Command Line Tools instead of full Xcode.

```bash
sudo xcode-select -s /Applications/Xcode.app/Contents/Developer
```

Install Xcode from the Mac App Store if `/Applications/Xcode.app` is missing.

### `You have not agreed to the Xcode license agreements` (macOS)

```bash
sudo xcodebuild -license accept
```

### `Could not find protoc`

Set `PROTOC` to point at the binary:

```bash
export PROTOC=/opt/homebrew/bin/protoc                       # macOS Homebrew
export PROTOC=/usr/local/bin/protoc                          # Intel Mac
$env:PROTOC = "$PWD\.tmp\tools\protoc-29.3\bin\protoc.exe"   # Windows
```

### `can't find crate for core` or `can't find crate for std` (Windows)

The pinned toolchain is missing the MSVC target:

```powershell
rustup target add x86_64-pc-windows-msvc --toolchain 1.92.0-x86_64-pc-windows-msvc
```

### Build is slow on the first run

Expected. First build downloads + compiles the full Rust dependency set. Subsequent builds are incremental.

### The UI is missing hosted Warp features (Drive, Sign-in, Cloud Agents, …)

Expected. The omw_local build intentionally removes or disables every Warp-cloud user surface. Only the local/core app remains. Future omw releases re-introduce an agent panel routed through `omw-server` → `omw-agent` (see PRD §13 v0.3).

### `audit-no-cloud.sh` fails with non-zero counts

A change reintroduced a forbidden hostname literal somewhere. To find it:

```bash
strings target/debug/warp-oss | grep -F "<failing-pattern>"
grep -rn "<failing-pattern>" app/src/ crates/ --include="*.rs"
```

URL literals in production code paths must be `#[cfg(not(feature = "omw_local"))]`-gated.

### The binary launches and **exits silently** with status 101 (no stderr, no panic message)

This is the most painful failure mode. Status 101 = a Rust panic + abort, but you'll see *nothing* on stderr because:

- `crates/warp_logging/src/native.rs` calls `log_panics::init()` inside `warp_logging::init()`.
- `log_panics::init()` **replaces** any panic hook installed earlier (e.g. in `app/src/bin/oss.rs::main`) with one that writes panics through `log::error!`.
- `log::error!` is wired to the Warp log file, not stderr.

So: **for any silent panic, look at the log file before chasing other suspects.**

```bash
# macOS
tail -200 ~/Library/Logs/warp-oss.log | grep -B2 -A30 "thread '.*' panicked at"

# Linux
tail -200 ~/.local/share/dev.warp.WarpOss/logs/warp-oss.log

# Windows
Get-Content "$env:LOCALAPPDATA\dev.warp.WarpOss\logs\warp-oss.log" -Tail 200 |
    Select-String -Pattern "panicked at" -Context 2,30
```

The Warp log file path on macOS is `~/Library/Logs/warp-oss.log`.

### Why `--no-default-features` is required for the audit-clean build

`--no-default-features` excludes the `cloud` feature. The `cloud` feature pulls in `warp-command-signatures/embed-signatures`, which embeds `firebase.json` CLI completion data containing `firebaseio.com` strings (third-party CLI completion, not Warp's hosted services — but still in the audit's contract). Without that feature, the embedded data is excluded and the binary stays clean.

URL literals in production code (e.g. `WarpServerConfig::production()` in `crates/warp_core/src/channel/config.rs`) are gated separately via `#[cfg(not(feature = "omw_local"))]`. So the strip needs **both** flags: `--no-default-features` to drop `cloud`/`embed-signatures`, and `--features omw_local` to fire the source-level URL gates.

---

## Helper script (Windows only, currently)

`vendor/warp-stripped/run-omw-local.ps1` automates the PowerShell setup + build. Modes:

```powershell
.\vendor\warp-stripped\run-omw-local.ps1            # build and run
.\vendor\warp-stripped\run-omw-local.ps1 -BuildOnly
.\vendor\warp-stripped\run-omw-local.ps1 -BinaryOnly
```

Behavior: ensures Cargo shims are on `PATH`, ensures the MSVC target is installed, ensures local `protoc` exists, runs the stripped build with `omw_local`. There is no equivalent shell script for macOS yet — the macOS commands above are short enough to run directly.
