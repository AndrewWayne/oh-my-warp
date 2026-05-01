# Build and Run the Stripped Local Warp Fork

This document covers the current Windows build path for the stripped local Warp fork in
`vendor/warp-stripped`.

This build is intentionally local-only:

- official Warp cloud services are disabled
- account, billing, team, referral, Drive-hosted, and Oz-hosted surfaces are stripped or gated off
- the hosted global Warp workflow catalog is replaced with an empty local stub

The validated executable is:

- `warp-oss`

## Scope

These instructions are written for:

- Windows
- PowerShell
- repo root at `C:\Users\andre\oh-my-warp\oh-my-warp`

If you move the repo, update the absolute paths in the commands below.

Building on macOS? See the [macOS](#macos) section near the end of this doc for the platform-specific prerequisites and one-time setup. The Cargo build invocation itself is identical on both platforms.

## Prerequisites

You need:

- `rustup`, `cargo`, and `rustc`
- the Rust toolchain pinned by `vendor/warp-stripped/rust-toolchain.toml`
- the `x86_64-pc-windows-msvc` Rust target for that toolchain
- `protoc` available through the `PROTOC` environment variable

The fork is pinned to:

- Rust `1.92.0`

## One-Time Setup

Open PowerShell at the repo root and run:

```powershell
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
```

Install the pinned Rust target if it is missing:

```powershell
rustup target add x86_64-pc-windows-msvc --toolchain 1.92.0-x86_64-pc-windows-msvc
```

Download a local `protoc` binary into the repo if it is not already present:

```powershell
$toolsDir = Join-Path $PWD '.tmp\tools'
$protocDir = Join-Path $toolsDir 'protoc-29.3'
$protocExe = Join-Path $protocDir 'bin\protoc.exe'

if (-not (Test-Path $protocExe)) {
    New-Item -ItemType Directory -Force -Path $toolsDir | Out-Null
    $zipPath = Join-Path $toolsDir 'protoc-29.3-win64.zip'
    Invoke-WebRequest `
        -Uri 'https://github.com/protocolbuffers/protobuf/releases/download/v29.3/protoc-29.3-win64.zip' `
        -OutFile $zipPath
    if (Test-Path $protocDir) {
        Remove-Item $protocDir -Recurse -Force
    }
    Expand-Archive -Path $zipPath -DestinationPath $protocDir
}
```

Set `PROTOC` for the current shell session:

```powershell
$env:PROTOC = "C:\Users\andre\oh-my-warp\oh-my-warp\.tmp\tools\protoc-29.3\bin\protoc.exe"
```

## Build

Change into the fork:

```powershell
Set-Location "C:\Users\andre\oh-my-warp\oh-my-warp\vendor\warp-stripped"
```

Validate the stripped target:

```powershell
cargo check -p warp --bin warp-oss --no-default-features --features omw_local
```

Build the executable:

```powershell
cargo build -p warp --bin warp-oss --no-default-features --features omw_local
```

Expected output binary:

```text
vendor/warp-stripped/target/debug/warp-oss.exe
```

## Run

Run through Cargo:

```powershell
cargo run -p warp --bin warp-oss --no-default-features --features omw_local
```

This is the preferred command when you want Cargo to rebuild automatically after code changes.

## Relaunch the Existing Binary

If you already built successfully and just want to reopen the UI:

```powershell
Start-Process `
    "C:\Users\andre\oh-my-warp\oh-my-warp\vendor\warp-stripped\target\debug\warp-oss.exe" `
    -WorkingDirectory "C:\Users\andre\oh-my-warp\oh-my-warp\vendor\warp-stripped"
```

This starts faster than `cargo run` because it does not rebuild.

## Full Copy-Paste Session

If you want one sequence that sets up the environment and starts the stripped build:

```powershell
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
$env:PROTOC = "C:\Users\andre\oh-my-warp\oh-my-warp\.tmp\tools\protoc-29.3\bin\protoc.exe"
Set-Location "C:\Users\andre\oh-my-warp\oh-my-warp\vendor\warp-stripped"
cargo run -p warp --bin warp-oss --no-default-features --features omw_local
```

## Helper Script

A small helper script is available at:

- `vendor/warp-stripped/run-omw-local.ps1`

From the repo root, you can use:

```powershell
.\vendor\warp-stripped\run-omw-local.ps1
```

Useful modes:

```powershell
.\vendor\warp-stripped\run-omw-local.ps1 -BuildOnly
.\vendor\warp-stripped\run-omw-local.ps1 -BinaryOnly
```

Behavior:

- ensures Cargo shims are on `PATH`
- ensures Rust target `x86_64-pc-windows-msvc` exists for the pinned `1.92.0` toolchain
- ensures local `protoc` exists under `.tmp\tools\protoc-29.3`
- runs the stripped `warp-oss` build with `omw_local`

## Troubleshooting

### `cargo` is not recognized

Add Cargo to the current shell:

```powershell
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
```

### `can't find crate for core` or `can't find crate for std`

The pinned toolchain is missing the Windows MSVC target:

```powershell
rustup target add x86_64-pc-windows-msvc --toolchain 1.92.0-x86_64-pc-windows-msvc
```

### `Could not find protoc`

Make sure `PROTOC` points to the downloaded binary:

```powershell
$env:PROTOC = "C:\Users\andre\oh-my-warp\oh-my-warp\.tmp\tools\protoc-29.3\bin\protoc.exe"
```

### Build is slow on the first run

This is expected. The first build downloads and compiles a large Rust dependency set.

### The UI launches but some hosted Warp features are missing

That is expected in this stripped build. The local fork intentionally removes or disables:

- account and login-dependent flows
- official cloud services
- hosted Oz/agent cloud surfaces
- hosted/global Warp workflows

Only the local/core app surface is intended to remain for later `omw` integration.

## macOS

Deltas from the Windows path above. The Cargo invocation itself is identical:
`cargo build -p warp --bin warp-oss --no-default-features --features omw_local` from `vendor/warp-stripped/`.

Note: `--no-default-features` is required to keep the binary clean for `scripts/audit-no-cloud.sh`. The `omw_local` feature alone gates the forbidden URL string literals in `crates/warp_core/src/channel/config.rs` and `app/src/auth/credentials.rs` via `#[cfg]`. The `cloud` feature (in `default`) additionally embeds `warp-command-signatures/embed-signatures`, which pulls in `firebase.json` CLI completion data containing `firebaseio.com` strings — `--no-default-features` excludes that. The cloud-related crates (firebase, warp_server_client, warp_managed_secrets, onboarding, voice_input) remain linked because they have no forbidden strings of their own; their UI surfaces are gated at the dispatcher level by earlier strip commits.

After a fresh build, `scripts/audit-no-cloud.sh target/debug/warp-oss` should report all six patterns at zero hits.

### Prerequisites

- **Full Xcode** (Mac App Store), not Command Line Tools alone. The Metal shader
  compiler `metal` is invoked by `crates/warpui/build.rs` and ships only inside
  Xcode.
- Homebrew, with `protobuf` installed: `brew install protobuf`.
- `rustup`, `cargo`, `rustc`. The pinned toolchain (`1.92.0`, see
  `rust-toolchain.toml`) auto-installs on the first cargo invocation inside
  `vendor/warp-stripped/`.
- macOS native targets (`aarch64-apple-darwin` on Apple Silicon,
  `x86_64-apple-darwin` on Intel) ship with the toolchain — no
  `rustup target add` step is needed.

### One-Time Setup

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

### macOS Troubleshooting

#### `xcrun: error: unable to find utility "metal"`

`xcode-select` is pointing at Command Line Tools instead of full Xcode. Fix:

```bash
sudo xcode-select -s /Applications/Xcode.app/Contents/Developer
```

If `/Applications/Xcode.app` is missing, install Xcode from the Mac App Store first.

#### `You have not agreed to the Xcode license agreements`

```bash
sudo xcodebuild -license accept
```

#### `Could not find protoc`

Make sure `PROTOC` points at the brew-installed binary for your shell session:

```bash
export PROTOC=/opt/homebrew/bin/protoc
```
