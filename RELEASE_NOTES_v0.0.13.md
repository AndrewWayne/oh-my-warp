# omw-local-preview v0.0.13 (Windows)

Windows x64 preview rebuilt from the complete 0.0.12 application source
(`299b55e1c53b685fc2ee665b6beb3b8f28e21eb8`) under version 0.0.13.
Application features and behavior remain unchanged. Release workflow guards,
installer failure diagnostics and package provenance support this trial.

This retains the native terminal/ConPTY and Credential Manager integration,
provider connection tests, keyless Ollama, persistent per-pane phone sharing,
same-pane attachment and secure read-only pairing defaults from 0.0.12.

The portable ZIP includes the desktop program, Node, Agent, keychain helper,
OpenConsole and required runtime DLLs. It does not require Rust, npm or a
separate Node installation. The per-user installer wraps the same verified
payload and preserves user settings, sessions and credentials on uninstall.
Each artifact has a SHA-256 sidecar; the payload contains `SHA256SUMS` and
`UPSTREAM_PROVENANCE.json`.

Windows 10 or newer, x64 only. Production signing is deferred. External model
providers require their normal credentials and network access; local Ollama
and optional Tailscale are separate installations.

The companion Mac 0.0.13 is built from the 0.0.11 baseline with maintenance
repairs and selected 0.0.12 backports. Platform source commits and final
checksums are recorded with the combined GitHub trial release.

Keep the release as a draft while package and installer acceptance is in
progress; no successful installer acceptance is claimed by this source note.
