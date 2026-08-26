# Windows installation

Windows releases provide two x86_64 artifacts:

- `*-setup.exe` is the recommended per-user installer.
- `*.zip` is the unchanged self-contained portable package.

Preview artifacts are not code-signed. Windows SmartScreen may therefore show
an unknown-publisher warning even when the downloaded SHA-256 matches the
release sidecar.

## Installer behavior

The installer does not request elevation. By default it installs to:

```text
%LOCALAPPDATA%\Programs\omw
```

It registers `omw` in the current user's Apps & Features list and creates an
`omw` Start Menu folder. A desktop shortcut is optional in the interactive
installer. The installed application payload lives in the `app` subdirectory;
`Uninstall.exe` lives at the installation root.

Running a newer installer replaces the complete application payload so files
removed by a release do not linger. The swap is rollback-safe: if the new
payload cannot be activated, the previous payload is restored. Close omw
before upgrading. A silent upgrade exits with code `12` when the main
executable is locked.

## Uninstall

Use **Settings → Apps → Installed apps → omw → Uninstall**, the Start Menu
uninstall shortcut, or:

```powershell
& "$env:LOCALAPPDATA\Programs\omw\Uninstall.exe"
```

Uninstall removes installed binaries, shortcuts, and the Apps & Features
registration. It intentionally preserves settings, terminal state, session
data, audit data, and provider secrets in Windows Credential Manager. This
makes reinstall and upgrade safe and prevents an accidental uninstall from
destroying user data.

## Silent deployment

Install or upgrade for the current user:

```powershell
& .\omw-warp-oss-v0.0.12-x86_64-pc-windows-msvc-setup.exe /S
```

Choose another installation directory by placing `/D=` last:

```powershell
& .\omw-warp-oss-v0.0.12-x86_64-pc-windows-msvc-setup.exe /S /D=D:\Apps\omw
```

Silent uninstall:

```powershell
& "$env:LOCALAPPDATA\Programs\omw\Uninstall.exe" /S
```

Exit code `0` means success. Codes `10` and `11` report an unsupported
architecture or Windows version. Codes `12` and `14` mean the app is still
running during install/upgrade or uninstall. Code `13` means payload extraction
or activation failed and the previous installed version was preserved.

## Integrity

Each installer and portable ZIP has its own LF-terminated `.sha256` sidecar.
For example:

```powershell
(Get-FileHash -Algorithm SHA256 .\omw-warp-oss-*-setup.exe).Hash
```

The release workflow builds the installer from the same staged payload as the
portable ZIP, verifies every file against `SHA256SUMS`, and runs a silent
install, locked-upgrade, replacement-upgrade, and uninstall smoke test before
uploading either Windows artifact.
