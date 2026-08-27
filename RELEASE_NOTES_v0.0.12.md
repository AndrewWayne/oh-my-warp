# omw-local-preview v0.0.12

**Windows desktop preview, local providers, and persistent phone sharing.**

This pre-release completes the self-contained Windows distribution path for
omw. It adds a recommended per-user installer with an uninstaller, a portable
ZIP, Windows-native terminal and credential integration, local-provider
support, and persistent per-pane sharing controls for a phone or another
browser on the same tailnet.

The Windows package does not require Rust, Cargo, npm, a separate Node.js
installation, administrator access, or any Warp-hosted service. Remote model
providers still require their normal network access; Ollama can run entirely
locally.

## What's in this release

### Windows installer and portable distribution

- A self-contained x64 portable ZIP includes the desktop app and every runtime
  component it needs.
- The recommended `*-setup.exe` installer installs for the current user under
  `%LOCALAPPDATA%\Programs\omw` by default, registers omw in Apps & Features,
  and creates a Start Menu shortcut. A desktop shortcut is optional.
- Installation creates `Uninstall.exe` in the installation directory. Normal
  uninstall removes application binaries, shortcuts, and registration while
  preserving settings, terminal/session state, audit history, and provider
  credentials.
- Upgrades replace the complete application payload so removed files cannot
  linger. If a running executable locks the installation, a silent upgrade
  exits with code `12` without damaging the existing installation.
- The portable ZIP and installer each ship with an independent SHA-256
  sidecar. The release workflow verifies every packaged path and payload hash
  before uploading either artifact.

### Providers and credential storage

- Settings supports OpenAI, Anthropic, OpenAI-compatible endpoints, and Ollama,
  including connection tests and persistent local-agent sessions.
- API keys are stored in the operating system's credential store: macOS
  Keychain on macOS and Windows Credential Manager on Windows. Configuration
  files retain credential references rather than plaintext keys.
- Keyless Ollama requests no longer receive an unnecessary `Authorization`
  header.

### Persistent pane sharing

- Each shareable local terminal pane has a persistent **Share with phone** /
  **Stop sharing** action, including while a full-screen terminal application
  is active.
- Sharing remains scoped to the selected pane, survives UI state changes, and
  can be stopped without closing the pane or terminating its process.
- The phone connects to the same pane and process rather than a newly created
  shell. Physical-iPhone and Tailscale acceptance covered bidirectional I/O,
  reconnect, unshare, same-pane identity, and the secure read-only default.
- Remote input remains disabled by default. This preview requires the exact
  opt-in `OMW_REMOTE_ALLOW_DEFAULT_WRITE=1` before newly paired devices can
  type into a shared pane.

### Windows reliability and UI polish

- Windows terminal startup, resizing, input routing, clipboard behavior, and
  child-process cleanup are more reliable.
- Line-number gutters, native caption controls, panel sizing, long status
  messages, and Agent-panel close/resize behavior have been refined for the
  Windows layout.
- Existing Agent-panel width preferences are preserved during migration while
  new windows receive the compact default.
- The token-bearing pair URL is no longer printed to desktop process output; it
  remains available through the sharing UI and is copied to the clipboard when
  sharing starts.
- Background update polling is disabled in the local build; the explicit
  **Check for updates** action remains available. Other unintended Warp-cloud
  behavior remains disabled. The compiled Windows binary is audited for all
  eight blocked Warp/Firebase hostnames before release.

## Install

### Windows x64

Download `omw-warp-oss-v0.0.12-x86_64-pc-windows-msvc-setup.exe` and its
matching `.sha256` file. Verify the checksum, then run the installer normally.
Administrator access is not required. See [Windows installation](./docs/windows-installation.md)
for upgrade, uninstall, silent-deployment, and checksum details.

The portable `omw-warp-oss-v0.0.12-x86_64-pc-windows-msvc.zip` remains
available for users who prefer not to install the application.

```powershell
(Get-FileHash -Algorithm SHA256 .\omw-warp-oss-v0.0.12-x86_64-pc-windows-msvc-setup.exe).Hash
Get-Content .\omw-warp-oss-v0.0.12-x86_64-pc-windows-msvc-setup.exe.sha256
```

### macOS Apple Silicon

Download the `aarch64-apple-darwin.dmg` and its matching `.sha256` sidecar,
then compare the checksum:

```sh
shasum -a 256 omw-warp-oss-v0.0.12-aarch64-apple-darwin.dmg
cat omw-warp-oss-v0.0.12-aarch64-apple-darwin.dmg.sha256
```

Open the DMG, drag the app into `/Applications`, then remove the quarantine
attribute before first launch:

```sh
xattr -dr com.apple.quarantine /Applications/omw-warp-oss.app
open /Applications/omw-warp-oss.app
```

## Preview limitations

- This release is unsigned. Windows SmartScreen may show an unknown-publisher
  warning, and macOS requires the quarantine-removal step above.
- Windows is x86_64-only and macOS is Apple-Silicon-only. Linux packaging is
  not available yet.
- Tailscale must be installed and running on both the host and client; it is not
  bundled. Direct-tailnet sharing requires launching the host with
  `OMW_REMOTE_BIND=<host-tailnet-IPv4>:8787`. The preview does not automatically
  configure Tailscale Serve.
- A cold iOS/Tailscale path can take 10-30 seconds to establish its first
  connection; the controller retries automatically.
- Resizing the desktop window during an active phone session does not yet send
  the new dimensions back to the phone.
- One agent session is supported per app process. The Agent panel does not yet
  show per-tool argument/result cards, and cost reporting remains CLI-only.
- On some macOS systems, the first API-key save from the unsigned app can fail.
  Settings reports the error; README documents the one-time Keychain workaround.
- Production code signing, a fresh-account/VM Windows acceptance lane, and
  Linux packages remain deferred.

Because this is marked as a pre-release, existing non-prerelease installations
will not discover it through the normal automatic-update channel.

## Architectures

- Windows: `x86_64-pc-windows-msvc`
- macOS: `aarch64-apple-darwin`
