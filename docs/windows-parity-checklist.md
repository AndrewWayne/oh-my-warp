# omw Windows Parity Checklist

Status: 0.0.12 CBBF/A47B package ready; gutter and persistent per-pane phone action automated gates pass; desktop visual and physical-phone validation pending

Last updated: 2026-08-26

This checklist turns the macOS reference and the omw product contracts into
measurable Windows acceptance criteria. A feature is not considered equivalent
because its code exists or because the application compiles. It must pass the
test in its row against a named executable and environment.

## Baseline under test

| Field | Value |
|---|---|
| Upstream Warp reference | Commit `2faa7a449c89735c9befc6c9d0318c36ab6198b7`; Phase 2 Windows GUI build passed; executable SHA-256 `4A047895DB9BC60041660CC840793590873FB199AD1951170E90957C38BB9C13` |
| omw reference | Commit `56fb4e3e0930a2eece818b791cd0a12b32c1a160` |
| omw Windows executable | `vendor/warp-stripped/target/debug/warp-oss.exe` from the disposable Phase 5C tree; 336,679,424 bytes (321.08 MiB); SHA-256 `53CD111A4231A1A73D536CFD9B74D997B0BBC87977A8F596C5FFF89DC2D68092` |
| Current 0.0.12 release candidate | Release EXE `CBBF648558EE71555712D913F9622D282E3047513C560ABF913DBE65251650CA`, 295,974,912 bytes, staged at `dist/staging-v0.0.12-dogfood.pane-phone.20260826-windows/omw-warp-oss.exe` and embedded byte-identically in ZIP `A47BEDA48E7F9E5E54CA2C7EEA503BDE8D82C8C0F2B989CA880A5C4228C7372A`, 190,243,587 bytes. Per-user installer `46FE84D41D162CF69D54EB597425E3FB73C06C517E66EECC52E4B45129F1BA8F`, 131,694,785 bytes, embeds that same 16,769-file payload and generates an installed `Uninstall.exe`. The locked/offline package build, clean-PATH helper/kernel probes, required payload checks, manifest hashes, archive path/duplicate checks, PDB exclusion, package-lock preservation, LF checksum sidecars, and independent source/staged forbidden-hostname audits pass. Automated gutter, persistent per-pane phone-action, install, locked-upgrade, replacement-upgrade, and uninstall regressions pass. Desktop visual and physical-iPhone acceptance are `NOT RUN`. |
| Previous Phase 5M release candidate | Refreshed release EXE `E5BD87696CCC771583B7B76B780280588FB73E864DC42DBA6172B7DBE264303C`, 295,988,224 bytes, embedded in ZIP `072066AB21C32132243A448816EF68E271AE80903EC2C3498E46A7B1BE7D1C33`, 190,258,727 bytes. Source/staging/ZIP-entry EXE hashes and byte counts match. The EXE omits the unintended omw_local install-detection listener on 9277. A payload-only agent refresh then repaired persistent keyless Ollama without invoking Cargo or relinking the EXE. All 16,759 manifest payload hashes, required entries, archive path/duplicate checks, package-lock preservation, PDB exclusion, packaged Node syntax, and forbidden-hostname checks passed. Exact E5BD desktop Settings, terminal, and local-agent network capture passed with normal close and cleanup. The subsequent local Windows integration gate passed without changing the package. Phone/Tailscale, elevated packet attribution, and fresh-account/VM validation remain deferred. Exact-binary OMW-04 and OMW-05 direct GUI evidence remains on predecessor B37; the E5BD changes are scoped to the retired listener and keyless Ollama session path. |
| omw feature lock | Disposable `vendor/warp-stripped/Cargo.lock`; SHA-256 `166E911114F2614BCDF522C978449B9637A18A2E59DFD04A9E53AB00884E735C` |
| Windows host | Windows build `22631.6199` (23H2), x64 |
| Toolchain | Rust/Cargo `1.92.0`, MSVC `14.44.35207`, Windows SDK `10.0.26100.0`, host Node `v24.19.0`/npm `11.17.0`; CI-parity Node validation used packaged Node `v22.11.0`/npm `10.9.0` |
| GPU baseline | Direct3D 12, DXGI, DirectComposition, DirectWrite, `dxcompiler.dll`, and `dxil.dll` loaded; adapter and driver version still need to be recorded |
| Isolated runtime profile | `WARP_DATA_PROFILE=omw-phase5a-ct04-20260817-1400` |
| macOS reference | The current Mac build has been viewed, but structured observations and evidence are not yet recorded; use M1-M5 below |

The Phase 3 build command was:

```powershell
cargo --config <validated-local-patch-config> build --locked --offline `
  -p warp --bin warp-oss --no-default-features --features omw_local
```

The normal-network equivalent is documented in
[`vendor/warp-stripped/OMW_LOCAL_BUILD.md`](../vendor/warp-stripped/OMW_LOCAL_BUILD.md).

## Vocabulary and evidence rules

Priorities:

- `P0`: Windows preview/release gate. A failure blocks dependent feature work.
- `P1`: expected parity before sharing a Windows preview outside the project.
- `P2`: polish, optional environment support, or a deferred compatibility lane.

Statuses:

- `PASS`: the complete row was observed against the executable identified above.
- `FAIL`: the test ran and the observed behavior did not meet the target.
- `BLOCKED`: the test cannot run because a named prerequisite is unavailable.
- `NOT TESTED`: implementation may exist, but there is no Windows result yet.
- `NOT IMPLEMENTED`: source or package inspection proves the required behavior is absent.
- `N/A`: intentionally outside the Windows target, with the reason recorded.

Rules:

1. Do not use `PASS` for source inspection alone.
2. If only part of a row passes, split the row or retain `NOT TESTED` and record
   the partial evidence.
3. Use a fresh `WARP_DATA_PROFILE` for first-run and persistence tests.
4. Store screenshots, command output, and logs under
   `.tmp/windows-parity/<run-id>/<check-id>/`; never store real API keys.
5. Record the executable SHA-256, Windows build, display scaling, monitor, GPU,
   and driver with every visual run.
6. Do not infer Mac behavior. Record it through M1-M5 or use the product
   requirement as the provisional target.

## macOS reference capture

These observations are required to turn provisional product targets into exact
parity targets. All are currently `PENDING`.

| Ref | Observation to record on the Mac build | Evidence |
|---|---|---|
| M1 | Fresh-profile launch, first-run screens, time to first local terminal, and every account/payment/network prompt | Screen recording plus ordered screenshots |
| M2 | PowerShell-equivalent shell interaction: ANSI/Unicode output, input, selection, clipboard, tabs, splits, search, command palette, and Settings | Screen recording plus the exact fixture commands |
| M3 | Font, cursor, resize/reflow, display scaling, maximize/minimize, and movement to an external monitor | Screenshots with display resolution/scaling noted |
| M4 | Provider setup, `# <prompt>`, streaming response, command/file approval, rejection, restart, and key persistence | Redacted screen recording and redacted config/keychain evidence |
| M5 | Tailscale pairing, physical-phone attach, bidirectional terminal I/O, disconnect/reconnect, and phone/desktop resize ownership | Host and phone recordings with timestamps |

## Core terminal

Rows are ordered by priority and then by expected execution order.

| ID | Pri | Target behavior | Reproducible Windows test | Current Windows status and evidence | Mac ref |
|---|---|---|---|---|---|
| CT-01 | P0 | The `omw_local` GUI compiles without default/cloud features using the pinned toolchain. | Run the Phase 3 Cargo command from `vendor/warp-stripped`; require exit 0 and the expected executable. | `PASS` 2026-08-18: locked/offline Phase 5C rebuild completed in 6m32s and produced SHA-256 `53CD111A...D68092`. | N/A |
| CT-02 | P0 | A fresh isolated profile creates a responsive top-level window within 15 seconds, stays alive for at least 5 minutes, and accepts a normal new-window request. | Set a unique `WARP_DATA_PROFILE`, launch from the runtime directory, record PID/window handle after 15 seconds and 5 minutes, then invoke `warp://action/new_window`. | `PASS` 2026-08-17: responsive 1280x800 `~` window created; new-window action succeeded; duplicate closed cleanly; recovery watcher remained healthy. | M1 |
| CT-03 | P0 | First run opens a usable local terminal without signup, browser authentication, payment, or Warp cloud onboarding. | Launch with a never-used profile and record every screen until the prompt is usable; fail on any mandatory cloud/account flow. | `PASS` 2026-08-17: the user confirmed the isolated Phase 3 OMW window appeared and looked normal; the screenshot shows the local welcome banner, a usable PowerShell prompt, and no mandatory cloud/account screen. | M1 |
| CT-04 | P0 | PowerShell presents a prompt, uses the intended working directory, accepts a command, prints output, preserves exit status, and handles Ctrl-C. | Run `$PSVersionTable.PSVersion`, `Get-Location`, `Write-Output OMW_PHASE4_OK`, `cmd /c exit 7`, inspect `$LASTEXITCODE`, then interrupt `ping -t 127.0.0.1`. | `PASS` 2026-08-17: PowerShell 7.6.4 opened at `%USERPROFILE%`; `cmd /c exit 7` yielded `$LASTEXITCODE=7`; Ctrl-C stopped a 26-reply continuous ping and `CT04_AFTER_CTRL_C` proved the prompt remained usable. Evidence: disposable `.tmp/windows-parity/20260817-phase5a-auto/ct04/manual-result.txt`. | M2 |
| CT-05 | P0 | ANSI 16/256/truecolor, CJK, emoji, combining marks, box drawing, and long wrapped lines render without corruption or width drift. | Run a fixed PowerShell ANSI/Unicode fixture, capture at least two screen widths, and compare glyph cells/cursor alignment with M2. | `PASS` 2026-08-17: 16/256/truecolor, CJK, emoji, combining marks, box drawing, two-width wrap/reflow, and the post-test prompt passed. Legacy direct-console encoding produced pre-render `?` characters, while PowerShell pipeline and UTF-8 console paths rendered correctly; no live gray-row overlap was reproduced. Evidence: `.../ct05/final-assessment.txt`. | M2 |
| CT-06 | P0 | Typing, cursor movement, Home/End, multiline edit, mouse selection, copy, paste, and undo do not lose or duplicate text. | Edit a fixed ASCII/Unicode multiline fixture; copy it to Notepad and back; compare exact text and CRLF behavior. | `PASS` 2026-08-17: typing/navigation/Home/End/undo, three-line editing, mouse selection, and the Notepad round trip passed without loss or duplication. Controlled OMW copy replaced a decoy and exactly preserved 106 UTF-16 code units, 3 CRLF, 0 bare LF, 0 lone CR, terminal CRLF, and SHA-256 `4E2CA5AC368B9C40BAE641F563805733D2527EE7CB81541D7A097A02862F44E5`. Evidence: `.../ct06/final-assessment.txt` and `decoy-copy-proof.txt`. | M2 |
| CT-07 | P0 | Tabs and panes can be created, focused, resized, reordered where supported, and closed without terminating unrelated shells. | Create two tabs and two splits, run distinct marker commands, resize each split, close one pane/tab, and verify the remaining markers and PIDs. | `PASS` 2026-08-17: two tabs and split panes were created, focused, resized, switched, reordered, and closed independently. T1A PID 20456 retained its original start time; T1B PID 10136 and T2A PID 23504 exited; Warp PID 4856 remained responsive. The before/after scoped process-tree CSVs are byte-identical. Evidence: `.../ct07/`. | M2 |
| CT-08 | P1 | Search, command palette, and Settings open through mouse and Windows shortcuts; changes persist after a clean restart. | Search scrollback for a unique token; invoke the palette; change one reversible setting under a disposable profile; restart and verify persistence. | `NOT TESTED`. | M2 |
| CT-09 | P1 | Selected system/bundled fonts render crisply with correct fallback; cursor and layout remain aligned at 100%, 125%, and 150% scaling. | Test one monospace font plus CJK/emoji fallback at each available scale; record screenshots, cursor position, and clipped controls. | `NOT TESTED`; GPU adapter, driver, and display-scale matrix still need recording. | M3 |
| CT-10 | P1 | Resize, maximize, minimize, restore, and repeated window creation do not produce blank frames, stale surfaces, or lost terminal state. | Stream numbered colored output while exercising all window states for 5 minutes; inspect application logs for DXGI/D3D errors. | `NOT TESTED`: GPU modules and window recreation passed, but sustained visible rendering has not been signed off. | M3 |

## Windows integration

| ID | Pri | Target behavior | Reproducible Windows test | Current Windows status and evidence | Mac ref |
|---|---|---|---|---|---|
| WI-01 | P0 | ConPTY supports spawn, read, write, resize, Ctrl-C, exit, and cleanup without orphaning the shell or OpenConsole. | Run `cargo test --locked -p omw-pty --test spawn_and_read --test write_and_echo --test resize --test kill_and_wait`, then repeat resize/Ctrl-C manually in the GUI. | `PASS` 2026-08-18: vendored `portable-pty 0.8.1` now interprets `TerminateProcess` correctly (`0` is failure, nonzero is success). The direct regression passed 50/50 stress iterations; the full `omw-pty` suite passed 25 tests; heartbeat-backed Drop coverage passed 10/10 combined iterations. The prior normal-window cleanup observation remains valid. Evidence: disposable `.tmp/windows-parity/20260817-phase5c-wi01-fix/`. | N/A |
| WI-02 | P0 | A PowerShell profile is discovered and launches with the intended Warp shell hook. | Select PowerShell explicitly; inspect the spawned command line and startup log for the expected `pwsh.exe` and hook path. Exercise command I/O under CT-04 and inherited environment handling under WI-06. | `PASS` 2026-08-17: Phase 3 captured the expected `pwsh.exe` command line and shell hook. | N/A |
| WI-03 | P1 | `cmd.exe` can be selected, receives input/output correctly, and reports Windows paths without shell conversion. | Open a cmd profile; run `ver`, `echo %CD%`, `set OMW_PHASE4`, and a nonzero `exit /b` in a disposable tab. | `NOT TESTED`; `C:\Windows\System32\cmd.exe` is available. | N/A |
| WI-04 | P1 | Git Bash/MSYS can be selected without corrupting drive paths, environment variables, shortcuts, or terminal modes. | Configure an available Bash, run `pwd`, `printf`, `git --version`, a Unicode path fixture, and a full-screen TUI. | `NOT TESTED`; Git Bash and MSYS2 Bash are available on the validation host, but profile integration is unverified. | N/A |
| WI-05 | P2 | An installed WSL distribution can launch, preserve its Linux working directory, and restore sessions without path conversion errors. | If a distro exists, run `uname -a`, `pwd`, Unicode output, tab restore, and a Windows-to-WSL directory launch. | `BLOCKED`: `wsl.exe` exists, but distro enumeration was not accessible/confirmed in the current environment. | N/A |
| WI-06 | P0 | Paths with spaces, non-ASCII characters, drive letters, UNC syntax where supported, and inherited environment variables remain exact. | Work in a disposable path such as `<test-drive>:\tmp\omw phase4\unicode-<fixture>`; print and compare `Get-Location`, arguments, and task-specific environment values in PowerShell and cmd. | `PASS` 2026-08-17: the live T1A pane exactly preserved a non-system-drive path containing spaces/CJK, a UTF-8 Unicode file, launch-time profile and task environment values, ordered PowerShell/cmd arguments, cmd exit 37, and an accessible administrative-share UNC form; cwd was restored. Fixture hashes and stable process trees matched before/after, with Warp and T1A responsive. Evidence: disposable `.tmp/windows-parity/20260817-phase5b-wi06/final-assessment.txt`. | N/A |
| WI-07 | P1 | Keyboard bindings use Windows conventions and do not confuse terminal control keys with application shortcuts. | Verify copy/paste, new tab/window, split, close, search, palette, Settings, Ctrl-C, Ctrl-Shift-C/V, Alt, and Windows-key behavior against a recorded binding table. | `NOT TESTED` (partial): CT-04/06/07 passed Ctrl-C, copy/paste, tab, split, focus, reorder, and close shortcuts. Search, palette, Settings, Alt/Windows-key behavior, and final product conventions remain. | M2 |
| WI-08 | P0 | Clipboard transfer preserves ASCII, Unicode, multiline CRLF text, and trailing newlines without exposing stale clipboard data. | Round-trip fixed fixtures between omw, Notepad, PowerShell, and another tab; hash file-backed fixtures before and after. | `PASS` 2026-08-17: CT-06 covered OMW/Notepad/PowerShell; the Phase 5B T1A-to-T2A relay then passed independent decoy-controlled source and target checks. Each copy was non-stale and exactly matched the 106-code-unit fixture: 3 CRLF, 0 bare LF, 0 lone CR, terminal CRLF, SHA-256 `4E2CA5AC368B9C40BAE641F563805733D2527EE7CB81541D7A097A02862F44E5`. Closing T2A removed only PID 14940 and OpenConsole PID 11180; T1A PID 20456/start time and OpenConsole PID 24828 survived responsive, and the stable process tree returned byte-identically. Attempt 1 was procedurally invalid and demonstrated no product failure. Evidence: disposable `.tmp/windows-parity/20260817-phase5b-wi08/final-assessment.txt`. | M2 |
| WI-09 | P1 | Moving between monitors and DPI values reflows the terminal once, keeps the cursor visible, and leaves popovers/menus on-screen. | Move a live long-output session across all available monitors/scales; open menus before/after; record row/column and screenshots. | `NOT TESTED`; monitor/scaling inventory is not yet recorded. | M3 |
| WI-10 | P0 | App data and logs use a deterministic per-profile user-writable location and require no elevation. | Launch with a unique `WARP_DATA_PROFILE`; verify SQLite/log files below `%LOCALAPPDATA%\warp\WarpOss-<profile>` and no writes to the source tree. | `PASS` 2026-08-17: isolated SQLite and log files were created under the expected LocalAppData profile without elevation. | N/A |
| WI-11 | P0 | The runtime directory contains the executable, ConPTY, DirectX compiler libraries, and bundled OpenConsole required for local launch. | Verify `warp-oss.exe`, `conpty.dll`, `dxcompiler.dll`, `dxil.dll`, and `x64\OpenConsole.exe`, then launch from that runtime directory. | `PASS` 2026-08-20: current ZIP `072066AB...D1C33` contains EXE `E5BD8769...4303C`, all four native runtime components, `vcruntime140.dll`, bundled Node, agent, and keychain helper. Independent verification passed all 16,759 manifest hashes, required entries, duplicate/safe-path checks, package-lock preservation, and PDB exclusion. The exact runtime-directory E5BD executable completed two OMW-02 GUI launches with normal closes; the second completed the full bounded desktop capture. Exact B37 previously completed two OMW-05 and one OMW-04 launch, and CA8 completed the 660-second automatic-egress regression. Clean-machine behavior remains WI-12. | N/A |
| WI-12 | P0 | A Windows preview package launches on a clean standard-user machine without Rust, Cargo, npm, Node, Visual Studio, or developer PATH entries. | Extract the package in a fresh Windows account/VM, verify hashes, launch from Explorer, and execute CT-03 through CT-07. | `NOT TESTED` 2026-08-20: newest self-contained ZIP `072066AB...D1C33` with EXE `E5BD8769...4303C` passed all 16,759 manifest hashes, archive/runtime checks, no-cloud audit, packaged Node syntax, and two current-account GUI launches with normal closure. The initial E5BD/DE95 package passed the clean-PATH helper/kernel probes; 0720 retained those helper/runtime bytes and exercised the refreshed kernel through the exact-package agent flow. This does not substitute for the required fresh-account/VM Explorer launch and CT-03 through CT-07, which remain deferred. | N/A |
| WI-13 | P1 | Windows CI builds and runs the selected Rust, TypeScript, web, ConPTY, and no-cloud gates with retained logs. | Inspect a clean Windows CI run for the commands referenced by this checklist and verify artifacts/evidence are retained. | `IMPLEMENTED; CLEAN HOSTED RUN PENDING` 2026-08-20. A focused `windows-latest` job now runs locked root Rust clippy/tests/doctests, ConPTY, native Credential Manager/helper, agent typecheck/Vitest, Web build/Vitest, and BYORC typecheck with retained logs; the real compiled-GUI/no-cloud audit remains in the tagged Windows release job. The release workflow pins Node 22, retains its build/audit log, requires the evidence artifact, publishes an LF-terminated ZIP `.sha256` sidecar, and passes validated release metadata through step environments rather than interpolating tag text into shell source. Local CI-equivalent gates passed, but WI-13 is not `PASS` until a clean hosted run and artifacts are inspected. The commit must include the currently untracked `vendor/portable-pty/` payload and `windows_os.rs` test. Evidence: `.../20260820-phase5m-windows-integration-ci/final-assessment.md`. | N/A |
| WI-14 | P0 | A standard user can install, upgrade, and uninstall omw through executable Windows surfaces without developer tools or elevation; uninstall preserves user data and credentials by default. | Build `setup.exe` from a manifest-verified staging payload, install silently into a disposable path, compare all installed hashes, require Apps & Features and Start Menu entries, reject a locked upgrade without damage, replace the whole payload on an unlocked upgrade, then run the generated uninstaller and require binaries/shortcuts/registration to be absent. | `PASS` 2026-08-26 against installer `46FE84D4...F1BA8F`, 131,694,785 bytes, built from the exact 0.0.12 staging payload. The smoke installed and re-hashed all 16,769 manifest entries, checked product metadata and shortcut targets, returned exit 12 on a write-locked executable without changing it, removed a deliberately stale file during replacement upgrade, re-hashed the upgraded payload, and removed the disposable install directory, Apps & Features key, and Start Menu folder through `Uninstall.exe /S`. No registry, shortcut, process, or temporary-directory residue remained. The NSIS script contains no user-data or Credential Manager deletion path. Installer signing remains deferred. | N/A |

## omw features

| ID | Pri | Target behavior | Reproducible Windows test | Current Windows status and evidence | Mac ref |
|---|---|---|---|---|---|
| OMW-01 | P0 | The local executable contains none of the eight forbidden Warp/Firebase hostnames. | Run `bash scripts/audit-no-cloud.sh target/debug/warp-oss.exe` with MSYS2 GNU `strings`; require eight zero counts and `audit-no-cloud: OK`. | `PASS` 2026-08-20: newest release EXE `E5BD8769...4303C` passed the fail-closed eight-hostname GNU strings audit, and the exhaustive staging audit found zero occurrences. The payload-only agent refresh changed two emitted JavaScript files; both also contain zero forbidden-hostname hits. The upstream Phase 2 binary contained 20 total hits across five patterns. | N/A |
| OMW-02 | P0 | First launch, Settings, terminal use, agent use, and Phone flow make no Warp-cloud or Firebase network requests. | Capture `Get-NetTCPConnection -OwningProcess <pid>` plus a packet/firewall trace through each flow; allow only loopback and endpoints explicitly selected by the user. | `PASS` 2026-08-20 for the bounded Windows desktop Settings, terminal, and local-agent scope; the complete cross-device row remains deferred. Exact E5BD/0720 produced exactly one keyless `/v1/models` request and one keyless streaming `/v1/chat/completions` request to the named loopback fixture, with correct model/prompt and no `Authorization`. Process-local deny proxy plus process-tree TCP polling observed zero denials/client errors, public TCP, package-tree use of existing v2rayN/Xray port 10808, retired port 9277, or unnamed loopback ports. Normal close, process/listener/credential cleanup, keyless config integrity, and unchanged sanitized system-proxy/v2rayN/Xray fingerprints all passed. Earlier CA8 separately crossed the ten-minute updater boundary and three focus cycles with zero automatic egress. This does **not** claim Phone/Tailscale, UDP/DNS attribution, or elevated ETW/WFP packet coverage. Evidence: `.../20260820-phase5-omw02-e5bd-agentfix-r2/final-assessment.md` plus the earlier automatic-egress evidence. | M1, M4, M5 |
| OMW-03 | P0 | At column zero, `# <prompt>` routes to the local agent, streams a response into the correct pane, and does not execute the prompt as a shell command; `##` and malformed prefixes fall through normally. | Build/package `apps/omw-agent`, use a deterministic mock/local provider, run positive and fall-through cases, and compare pane/session routing. Also run `omw_inline_prompt_test` and pane-session tests. | `PASS` 2026-08-18: repaired extracted GUI routed one exact column-zero prompt to `/chat/completions` with the prefix stripped, streamed `OMW_P0_VISIBLE_RESPONSE_PANE_A_OK`, and persisted it as a non-shell output block on pane A UUID `A3F876AF...2D31`. Focus moved to distinct pane B during streaming, where a shell marker executed; the response remained on pane A. `##` and malformed-prefix inputs executed in PowerShell and added zero provider requests. Evidence: `.../20260818-phase5-p0-gui-omw03/final-assessment.txt`. | M4 |
| OMW-04 | P0 | Command and file-write tool calls remain blocked until explicit approval; Reject has no side effect and Approve performs exactly one audited action. | Drive deterministic mock command and file-edit calls; snapshot target files/processes before and after Reject/Approve; run GUI approval and agent policy tests. | `PASS` 2026-08-20 against exact EXE `B37E60A8...D027C`. Reject returned `rejected by user`, left the target absent, and produced zero audit rows. Approve created the exact sentinel once and produced one exit-0 audit row. The deterministic provider completed exactly four requests; normal GUI close and credential/package-process/listener cleanup passed. Manual terminal entry avoided restored-tab and Windows foreground ambiguity without changing the package, provider, approval, filesystem, audit, request-count, or cleanup assertions. Evidence: `.../20260820-phase5-p0-gui-omw04-b37e/final-assessment.txt`. | M4 |
| OMW-05 | P0 | Settings can add, validate, select, test, edit, and remove OpenAI, Anthropic, OpenAI-compatible, and Ollama providers without exposing keys. | Use a disposable `OMW_CONFIG`; exercise valid/invalid forms and provider round trips; restart; inspect redacted TOML and UI state. | `PASS` 2026-08-20 against exact EXE `B37E60A8...D027C`. Two GUI runs exercised all four provider kinds, local invalid-form rejection with no request, masked input, edited-model/default persistence, restart, configured removal, and normal close. The keyed-to-Ollama live-buffer sentinel disappeared immediately, never reached config/logs/argv, and the obsolete credential was deleted. Exact totals were six accepted direct requests (OpenAI 1/Bearer, compatible 2/Bearer, Ollama 1/no-auth, switched Ollama 2/no-auth), one generic denied Anthropic CONNECT, and zero unknown traffic. AfterRun1, AfterRun2, and cleanup snapshots passed every assertion; all synthetic credentials were removed. Visible statuses are operator-confirmed because maximizing reflowed the large Settings page outside the screenshot viewport; fixture logs independently prove route/auth/counts. Settings Test is a `/models` connection/auth probe and does not replace the separate real chat-inference gate. Evidence: `.../20260819-phase5-p0-gui-omw05-b37e/final-assessment.txt`. | M4 |
| OMW-06 | P0 | API keys persist across process restart in Windows Credential Manager/DPAPI, are retrievable only through the helper, and never appear in config, logs, process arguments, or error text. | Save a sentinel key, restart app/helper, invoke a mock provider, delete the key, scan config/logs/process command lines for the sentinel, and verify removal. | `PASS` 2026-08-18 for the storage/helper contract: Windows `auto` and explicit `os` select the native Credential Manager backend. A generated Unicode sentinel was stored, read exactly from two fresh helper processes without entering argv/stderr, deleted, then returned exit 1 with empty stdout and exactly `not found` on stderr. Both the direct extracted-kernel probe and the packaged GUI agent route resolved temporary fake credentials through the packaged helper; each entry was deleted and independently confirmed absent. GUI provider management remains OMW-05. | M4 |
| OMW-07 | P1 | Remote control binds to loopback by default; after explicit user opt-in, Tailscale Serve exposes it only to the tailnet; the pair URL/QR is single-use and expires after 10 minutes. | Start the service without overrides and require a loopback-only listener. Then explicitly enable Tailscale Serve, inspect `tailscale serve status`, redeem the URL once, retry/expire/revoke it, and capture host/phone evidence. | `NOT IMPLEMENTED`: the GUI currently never invokes Tailscale Serve and instead requires an explicit `OMW_REMOTE_BIND` for direct tailnet-IP HTTP. Tailscale is installed for the pending direct-tailnet OMW-09 physical-device lane, but that does not satisfy this row's Serve contract. | M5 |
| OMW-08 | P1 | The locked production Web Controller bundle is generated and included in a successful GUI build without modifying `package-lock.json`. | Hash `package-lock.json`, run the Web Controller production build followed by the `omw_local` GUI build, require both to succeed, confirm embedded assets were consumed, and compare the lock hash. | `PASS` 2026-08-17: Phase 3 built 296 modules/10 files, included the bundle in the successful GUI build, and preserved the package-lock hash. Runtime serving and Vitest remain OMW-09. | M5 |
| OMW-09 | P1 | The embedded Web Controller passes Vitest, is served by the Windows host, and lets a physical phone pair with the same active PowerShell pane, exchange I/O, disconnect, and reattach without creating an unrelated shell. | Run `npm.cmd test --workspace @oh-my-warp/web-controller`, launch the packaged GUI with an explicit tailnet `OMW_REMOTE_BIND` and `OMW_REMOTE_ALLOW_DEFAULT_WRITE=1`, then follow `docs/mobile-web-controller-phone-qa.md`. Begin in an ordinary PowerShell pane before starting an agent and require the persistent pane-header action; then start normal `codex` and require the action in alternate-screen mode. Repeat with the write variable unset and a fresh browser identity. Record pair capabilities, served assets, pane/PID and process-tree identity, per-pane labels, byte flow, disconnect, reconnect, unshare, and the read-only default. | `NOT TESTED` (`AUTOMATED READY / PHYSICAL PENDING`): exact-value write-opt-in tests pass, the focused phone-action suite passes 8/8, Web Controller Vitest passes 114 with 1 intentional skip, and the production bundle and 0.0.12 package gates pass. The staged candidate is EXE `CBBF6485...650CA` in ZIP `A47BEDA4...C7372A`. Tailscale/physical-phone evidence for ordinary-pane and alternate-screen reachability, same-pane identity, bidirectional I/O, reconnect, unshare, read-only default, and no sibling shell remains pending. Do not mark this row `PASS` unless every physical check succeeds. | M5 |
| OMW-10 | P1 | Phone keyboard/viewport resize and desktop-pane resize negotiate usable rows/columns without oscillation, sub-8-row frames, clipping, or stale dimensions. | Pair a phone, show/hide keyboard, rotate, resize desktop pane/window, and record WebSocket resize frames plus xterm dimensions in both directions. | `NOT IMPLEMENTED` for complete parity: phone-to-host logic exists, but host-to-phone reverse resize remains an open cross-platform gap. | M5 |
| OMW-11 | P2 | Multiple panes keep agent prompts, streaming responses, approvals, and remote viewers isolated to the correct pane/session. | Open two marked panes, submit concurrent deterministic prompts, approve one tool call, attach a viewer, and assert no event crosses session IDs. | `NOT TESTED`: pane-routing tests exist; current one-agent-session limitations must be recorded as shared product behavior, not a Windows-only regression. | M4, M5 |
| OMW-12 | P1 | With no override, the bundled `AGENTS.md` is created in a Windows-appropriate user-writable app-data directory; `OMW_AGENTS_MD_PATH` is honored exactly and an existing file is never overwritten. | Under a disposable Windows account/profile, test first run with the override set and unset, compare resolved paths and contents, edit the file, restart, and require the edit to remain intact. | `PASS` 2026-08-18: the default resolves to `%LOCALAPPDATA%\omw.local.warpOss\AGENTS.md`, with `%USERPROFILE%\AppData\Local` fallback. A temporary LocalAppData profile proved first-run baseline creation and preservation after a simulated restart; the exact override and remaining I/O cases passed in the 64/64 `omw-config` suite. Repaired package `25648A9E...C10F4` includes the change, and its extracted binary contains the expected Windows path and bundled-prompt markers. Fresh-account package sign-off remains WI-12. | M4 |

## Recommended execution order

1. Include the required untracked PTY/helper-test inputs in the eventual commit,
   run the new hosted Windows CI job, and inspect its retained artifacts.
2. Run the remaining applicable P1/P2 display, shell-profile, and multi-pane
   checks, pausing before any further GUI interaction.
3. Stage Tailscale plus a physical phone when available and execute OMW-07
   through OMW-10.
4. Run the self-contained package in a fresh account/VM to complete WI-12 when
   that environment becomes available; this is intentionally deferred.
5. Record M1-M3 on the Mac reference and the Windows GPU/monitor inventory.

Never use real provider keys in automated tests or evidence; retain the OMW-06
native Credential Manager regression as the storage baseline.

## Current gate summary

Confirmed through the refreshed Phase 5M checkpoint:

- Upstream and `omw_local` Windows GUI builds compile and start.
- The omw runtime creates a responsive window, initializes D3D12/DXGI, and
  starts bundled ConPTY/OpenConsole plus PowerShell.
- The complete local runtime payload exists in the debug target directory.
- The no-cloud binary audit passes with zero forbidden hostnames.
- The unfiltered locked/offline OMW Rust workspace passes on Windows. There
  are no filtered Windows exclusions; one explicitly ignored beyond-v1
  WebSocket broadcast test remains.
- The Web Controller production bundle builds with the locked npm graph.
- CT-04 through CT-07 pass in the live Windows GUI: PowerShell/Ctrl-C,
  rendering/reflow, exact editing/clipboard, and tab/pane process isolation.
- WI-06 exact Windows paths/arguments/environment/UNC behavior and WI-08
  decoy-controlled another-tab clipboard/lifecycle isolation pass.
- WI-01 passes its direct kill-result regression, 50-iteration stress run,
  full PTY suite, heartbeat Drop coverage, and prior normal-window cleanup.
- `omw-config` watcher paths normalize Windows verbatim and regular forms;
  the two formerly filtered watcher tests pass.
- The server registry lifecycle uses a bounded ten-second Windows observation
  window; its formerly filtered lifecycle test passes in about 3.1 seconds.
- OMW-06 uses Windows Credential Manager and passes fresh-process
  read/read/delete/not-found coverage without placing the sentinel in argv or
  error output.
- OMW-12 resolves its unset Windows default below `%LOCALAPPDATA%`, falls back
  to `%USERPROFILE%\AppData\Local`, preserves a user-edited canonical file,
  and still honors `OMW_AGENTS_MD_PATH` exactly.
- OMW-03 passes in the repaired extracted release GUI: prefix interception,
  streaming visible-output persistence, delayed multi-pane correct-target
  routing, fall-through, shell continuity, and credential cleanup all pass.
- The local assistant panel now subscribes to live agent events, routes approval
  decisions through typed GUI actions, disables resolved approval cards, and is
  no longer rejected as an official-cloud action under `omw_local`. Its header
  icon is also available without the official cloud Agent Mode feature flag.
- The user visually confirmed the local panel, connected session, pending bash
  approval, and Approve/Reject controls. The first Reject click did not dispatch;
  it exposed repaint-created mouse handles that lost state between mouse-down and
  mouse-up. The panel now keeps stable handles keyed by approval ID.
- The current 0.0.12 package is ZIP `A47BEDA4...C7372A`, containing exact EXE
  `CBBF6485...650CA`. Independent verification passed all 16,769 manifest
  hashes, archive-safety and required-entry checks, package-lock preservation,
  PDB exclusion, the LF-only checksum sidecar, and both source/staged
  eight-hostname audits. The package build also passed clean-PATH helper and
  kernel probes. Desktop visual and physical-iPhone acceptance remain pending.
- OMW-04 passes against predecessor exact package B37: Reject produced no target or audit
  row, while Approve produced one exact target and one exit-0 audit row. The
  provider completed four requests; normal close and scoped cleanup passed.
- OMW-05 passes against predecessor exact package B37: two GUI runs cover all four provider
  kinds, invalid validation, masking, edit/default/restart persistence,
  keyed-to-Ollama live-buffer clearing, configured removal, exact route/auth/count
  assertions, two normal closes, zero leaks, and full scoped cleanup.
- Background omw_local update polling is disabled while the explicit manual
  update action remains. Predecessor exact package CA8 crossed the ten-minute
  timer boundary and three focus cycles with zero denied proxy requests and zero
  public scoped TCP observations. Exact E5BD/0720 now also passes the bounded
  desktop Settings, terminal, and local-agent capture: exactly two expected
  keyless loopback requests, no public/10808/9277/unnamed-loopback process-tree
  TCP, normal close, complete scoped cleanup, and unchanged v2rayN/system-proxy
  fingerprints. Phone/Tailscale, UDP/DNS attribution, and elevated ETW/WFP
  coverage remain explicitly outside this PASS.

The previous six characterized Windows workspace filters are resolved. The
Phase 5M locked/offline all-target root-workspace run listed 364 tests and
recorded 363 passed, 1 explicitly ignored, and 0 failed; the Windows remote
start/stop target contributes two executed tests and now proves live status plus
the exact bound port, while the Windows default-shell unit proves `cmd.exe /Q`.
Doctests, formatting, and full `-D warnings` clippy passed. With the real Windows
helper path, the refreshed agent suite passes 92 tests with 1 headless-Linux-only
integration test skipped; Web Controller tests remain 114 passed with 1 skipped,
and its production build plus BYORC typecheck pass. The separate GUI integration
expectation for incomplete non-default provider drafts was refreshed in Phase 5E
and its full test binary now passes.

Phase 5E packaged-runtime result:

- `omw-warp-oss-v0.0.11-x86_64-pc-windows-msvc.zip` is self-contained and
  SHA-256 `1BBD73EC...BA4E476`; its 16,761-entry payload manifest passed.
- The extracted package passed clean-PATH helper/kernel probes, a real JSON-RPC
  agent turn against a deterministic loopback provider, and the eight-hostname
  no-cloud audit.
- The extracted release GUI then launched responsive with a clean PATH and used
  the packaged Node/kernel/OpenConsole/helper paths. A column-zero `#` prompt
  made exactly one deterministic provider request; double-hash and malformed
  prefixes made none; normal PowerShell input remained usable afterward.

Phase 5F Windows `AGENTS.md` result:

- `omw-config` now uses `%LOCALAPPDATA%\omw.local.warpOss\AGENTS.md` as the
  Windows default and a user-profile LocalAppData fallback when the environment
  variable is unavailable. The disposable-profile regression passed first-run
  bootstrap, edit preservation, restart-equivalent idempotence, exact override,
  and fallback behavior; all 64 `omw-config` tests and doctests passed.

Phase 5G refreshed-package result:

- The package was rebuilt after OMW-12. ZIP `E3D086CD...C6B0F9` contains release
  EXE `370529CD...EFF48`; all 16,761 payload hashes passed independently.
- The extracted EXE passed all eight no-cloud patterns with zero hits and
  contains the new Windows `AGENTS.md` path plus bundled-prompt markers.
- Packaged Node, agent, and helper completed a deterministic loopback turn,
  streamed `Hello packaged agent`, and removed the temporary Credential Manager
  fixture; a fresh helper lookup and `cmdkey` inventory confirmed absence.

Phase 5H repaired-package and P0 GUI result:

- The first refreshed executable exposed an upgrade-profile regression: the
  historical migration was recorded but `welcome_panes` was absent, causing
  each app-state save to fail. Forward idempotent migration
  `20260818203000_ensure_welcome_panes_table` repairs missing profiles and is a
  no-op for correct profiles; both synthetic cases passed.
- Repaired ZIP `25648A9E...C10F4` contains EXE `68FCBDF7...C533`; all 16,761
  hashes, all eight no-cloud checks, the packaged agent/helper turn, and
  Credential Manager cleanup passed independently.
- The reproduced real upgrade profile moved from table/ledger 0/0 to 1/1.
  The next runtime log contained zero missing-table errors, and terminal block
  persistence resumed.
- Packaged-GUI OMW-03 passed with two distinct pane UUIDs. Focus moved to pane B
  during a deliberately delayed pane-A stream, but the exact response remained
  on pane A's non-shell output block. Both fall-through inputs executed in
  PowerShell and caused zero additional provider requests.

Phase 5I approval-wiring and GUI checkpoint:

- A live-event pump now applies broadcast agent events to the local assistant
  transcript and schedules another receive without blocking the UI.
- Approval buttons dispatch a typed decision carrying session, approval ID, and
  decision. A successful send changes the card from Pending to its resolved
  state, and repeated queued clicks no-op once it is no longer pending.
- `ToggleAIAssistant` and the assistant icon bypass the official-cloud guard only
  in `omw_local`; non-local builds retain the existing guard.
- The user visually confirmed that the assistant icon opens the local panel and
  that the live `approval/request` renders the correct pending bash card.
- The first Reject click on candidate `7CCDA406...858C8DC` did not dispatch. The
  button state was being recreated during repaint, so mouse-up could not complete
  the click begun on the previous handle. Approval cards now retain stable
  Approve/Reject mouse handles keyed by approval ID and remove them on resolution.
- Targeted `omw_local` Cargo check passed. Repaired release candidate
  `F29E52B4...A55B89` built successfully, matches the staged package copy, and
  passed all eight forbidden-hostname checks plus `audit-no-cloud: OK`.
- A fresh human run proved that both stable buttons dispatch: Reject returned
  exactly `rejected by user` without a side effect, and Approve wrote the exact
  target once while the provider completed four turns.
- That run exposed a real command-audit gap: the approved command executed but
  appeared zero times in the audit table because the broker wrote directly to
  the PTY instead of using TerminalView's ExecuteCommand path.
- Production agent commands now return to the UI thread as
  `CommandExecutionSource::OmwAgent` and use the normal block/history/audit path.
  Source parity and diff checks passed, and locked/offline compilation passed in
  46.49 seconds. Candidate `F29E52B4...A55B89` predates this fix.
- The full-debug rebuild exceeded this PC's 16 GiB practical memory limit. A
  package-only four-codegen-unit, no-debug-symbol override retained release
  optimization and completed in 28m09s; the temporary override was then removed.
- Final tagged candidate `0129B224...435F3E` was packaged in ZIP
  `3E3D9AC3...29FAD21`; source/staging/ZIP-entry identities, all 16,761 payload
  hashes, archive safety checks, required entries, and all eight forbidden-hostname
  counts passed.
- The exact tagged EXE passed the complete human OMW-04 rerun: Reject produced
  no side effect or audit row, while Approve produced one exact sentinel write
  and one exit-0 audit row. Four provider turns and full cleanup passed.

Phase 5J/K refreshed-package, automatic-egress, and OMW-05 result:

- omw_local no longer registers automatic launch, focus, or timer update checks;
  explicit manual update checks remain available. Focused warp_features and
  warp_core regressions plus the shipped-feature production check passed.
- Pre-fix CA8 ZIP `8AF1C459...BDD8979` contains exact EXE
  `CA8F2CD6...8038AC6`.
  Source/staging/ZIP-entry identity, all 16,759 manifest payload hashes, archive
  safety, required runtime files, clean-PATH helper/kernel probes, PDB exclusion,
  and the fail-closed no-cloud audit passed independently.
- The exact CA8 EXE ran 660 seconds through three focus cycles and the prior
  ten-minute updater boundary with no proxy-observed request, no GitHub attempt,
  and no public scoped TCP connection. The GUI closed normally and left no
  package process or listener.
- Exact EXE `B37E60A8...D027C` and ZIP `4BF5DF5E...B17AD` include the keyed-to-Ollama
  live-buffer fix and independently pass all 16,759 payload hashes, archive/runtime
  checks, clean-PATH probes, PDB exclusion, package-lock preservation, and no-cloud audit.
- OMW-05 passed on that exact package: all four provider kinds, invalid local
  validation, edit/default/restart persistence, live masked-buffer clearing,
  reference-only config, credential deletion/retention, two normal closes, exact
  route/auth/count assertions, zero leaks, and scoped cleanup passed. The operator's
  exploratory compatible kind toggle made no request and was safely recovered with
  Discard before the planned Test.
- Exact-binary OMW-04 passed against then-current candidate `B37E60A8...D027C`:
  Reject produced no
  target or audit row, while Approve produced one exact target and one exit-0
  audit row. Four provider requests and complete credential/process/listener
  cleanup passed; the GUI closed normally without forced cleanup.

Phase 5L desktop network capture and implementation-gap result:

- The first exact-B37 desktop diagnostic exposed an unintended GUI-root listener
  on `127.0.0.1:9277`. Source traced it to the upstream install-detection HTTP
  server, whose only active stripped-build route was not part of omw_local. The
  registration is now excluded under `omw_local`; the production-feature check
  passed and refreshed EXE `E5BD8769...4303C` has no 9277 listener.
- The first exact-E5BD diagnostic passed Settings and terminal network checks but
  exposed a persistent-session keyless Ollama defect before HTTP. The agent now
  supplies a non-secret SDK sentinel only for Ollama without `key_ref` and removes
  the SDK-generated Authorization header before transport. The loopback
  request-boundary regression and typecheck succeeded; the agent suite recorded
  90 passed with 3 OS-keychain integration tests intentionally skipped.
- The agent-only payload refresh invoked no Cargo build or linker and preserved
  exact E5BD. Current ZIP `072066AB...D1C33` passed 16,759/16,759 manifest hashes,
  archive safety, package-lock preservation, PDB exclusion, and packaged Node
  syntax checks.
- The fresh exact-E5BD/0720 r2 capture passed the bounded desktop Settings,
  terminal, and local-agent scope. Exactly one `/v1/models` and one streaming
  `/v1/chat/completions` request reached the keyless loopback fixture without
  Authorization. There were zero proxy denials/errors, public connections,
  package-tree uses of v2rayN/Xray port 10808, retired port 9277 observations, or
  unnamed loopback ports. Normal close, full scoped cleanup, config integrity,
  and unchanged v2rayN/Xray/system-proxy fingerprints passed.

Phase 5M Windows integration and CI result:

- The exact release package remained E5BD/0720; no GUI rebuild, relink, package
  refresh, or launch occurred. Locked/offline root Rust tests, doctests,
  formatting, and full clippy passed, as did the Web production build, 114/1
  Vitest result, BYORC typecheck, and 92/1 agent result with the real helper.
- The formerly Unix-only remote start/stop target now runs two tests on Windows.
  Its diagnostics exposed over-isolated daemon and CLI subprocess environments:
  removing `SystemRoot` prevented WinSock initialization or made a live daemon
  look stale. Preserving only that required value made live status, exact-port,
  public stop, normal-exit, and cleanup assertions pass. Panic-safe child cleanup
  prevents failed assertions from orphaning a disposable daemon; the one
  diagnostic orphan was verified as the exact debug test binary, stopped by PID,
  and its listener was confirmed absent before the passing rerun.
- A direct Windows unit test now pins the default shell to `cmd.exe /Q`.
- The focused Windows CI job and retained release-audit evidence are implemented.
  CI/release YAML, all 14 embedded PowerShell blocks, and 24 Bash blocks parse;
  no `run` block directly embeds workflow expressions. Release metadata is
  environment-routed and validated, Node 22 is pinned, evidence is mandatory,
  and the checksum sidecar is LF-terminated. Both synchronized worktrees pass
  `git diff --check`, and the package lock is unchanged. WI-13 remains pending
  until the first clean hosted run and artifact inspection.
- The per-user executable installer and generated uninstaller pass the WI-14
  install, locked-upgrade, replacement-upgrade, full-payload hash, registration,
  shortcut, uninstall, and residue gates. The portable ZIP remains available.

Remaining blocking or deferred gaps after the Phase 5M checkpoint:

- `P0`: WI-12 still requires a fresh-account/VM Explorer launch and CT-03
  through CT-07 against the extracted release package. A release build ignores
  `WARP_DATA_PROFILE`, so account/VM isolation cannot be substituted by that env var.
- `P0`: OMW-02 desktop Settings, terminal, and local-agent scope passes. Phone/
  Tailscale, UDP/DNS attribution, and elevated process-filtered ETW/WFP packet
  coverage remain explicitly deferred and are not claimed by the desktop result.
- `P0`: exact-binary OMW-04 and OMW-05 direct GUI evidence is on predecessor B37.
  The current E5BD changes are scoped to omitting listener 9277 and repairing
  keyless Ollama persistent sessions; record an explicit scoped-change waiver or
  rerun those gates if release policy requires every P0 GUI row on one ZIP hash.
- `P1`: Tailscale is not installed, so remote-control parity cannot yet run.
- `P1`: the documented Tailscale Serve flow and the current direct tailnet-IP
  implementation do not match.
- `P1`: the Windows test lane and retained release-audit artifacts are now
  implemented, but the first clean hosted run is pending. The eventual commit
  must include untracked `vendor/portable-pty/` and `windows_os.rs` inputs or a
  clean checkout will fail before the intended tests run.
- Reference: detailed M1-M5 Mac observations have not yet been captured.

## Result record template

Add one row per execution. Never include secrets in the evidence path or notes.

| Run ID | Date | Build/EXE SHA-256 | Host/display/GPU | Check IDs | Result | Evidence path | Issue/reference |
|---|---|---|---|---|---|---|---|
| Example | 2026-08-17 | `B2098B...EE29` | Win 22631.6199; display/GPU to record | CT-02, WI-10 | `PASS` | `.tmp/windows-parity/example/` | Phase 3 baseline |
| phase3-manual-20260817-a | 2026-08-17 | `B2098B...EE29` | Win 22631.6199; 2538x1566 capture; GPU/scale to record | CT-02, CT-03 | `PASS` | `.tmp/windows-parity/20260817-phase3-manual/omw-window.png` | User confirmed window appeared, looked normal, and reached a usable local prompt; image SHA-256 `ED0951...14415` |
| phase3-manual-20260817-b | 2026-08-17 | `B2098B...EE29` | Win 22631.6199; 2538x1566 capture; GPU/scale to record | CT-04 | `NOT TESTED` | `.tmp/windows-parity/20260817-phase3-manual/omw-window.png` | Partial evidence: `echo OMW_PHASE3_OK` round trip passed; version/location/exit/Ctrl-C remain |
| phase5a-auto-20260817 | 2026-08-17 | `71852AA1...3E316` | Win 22631.6199 x64 | Filtered Rust workspace/doctests; GUI; agent; web | `CONDITIONAL PASS` | Disposable `.tmp/windows-parity/20260817-phase5a-auto/` | Rust: 354 passed, 0 failed, 2 ignored, 6 characterized filters; GUI: 66 passed, 2 ignored, 1 stale-test failure; agent 92/92; web 114 passed, 1 skipped |
| phase5a-manual-20260817-ct04-ct07 | 2026-08-17 | `71852AA1...3E316` | Win 22631.6199 x64; GPU/driver/scale still to record | CT-04, CT-05, CT-06, CT-07 | `PASS` | Disposable `.tmp/windows-parity/20260817-phase5a-auto/{ct04,ct05,ct06,ct07}/` | PowerShell/Ctrl-C, rendering/reflow, exact editing/clipboard, and tab/pane isolation all passed; CT-07 surviving/closed PIDs verified |
| phase5b-wi06-20260817 | 2026-08-17 | `71852AA1...3E316` | Win 22631.6199 x64 | WI-06 | `PASS` | Disposable `.tmp/windows-parity/20260817-phase5b-wi06/` | Exact D:/space/CJK path, UTF-8 file, PowerShell/cmd argv and inherited env, cmd exit 37, and live `\\localhost\D$` UNC passed; no persistent child/process-tree change |
| phase5b-wi08-20260817 | 2026-08-17 | `71852AA1...3E316` | Win 22631.6199 x64 | WI-08 | `PASS` | Disposable `.tmp/windows-parity/20260817-phase5b-wi08/` | Decoy-controlled T1A-to-T2A transfer was exact/non-stale; closing target pwsh/OpenConsole preserved the source tab and restored the baseline tree |
| 20260817-phase5b-wi01-final-close | 2026-08-17 | `71852AA1...3E316` | Win 22631.6199 x64 | WI-01 | `MIXED: runtime PASS; contract FAIL` | Disposable `.tmp/windows-parity/20260817-phase5b-wi01-final-close/` | Normal app close removed the six-process family with no delayed respawn/crash/recovery evidence; `Pty::kill()` result reporting remains defective and heartbeat Drop coverage inconclusive |
| 20260818-phase5c-resume | 2026-08-18 | `53CD111A...D68092` | Win 22631.6199 x64 | CT-01, WI-01, OMW-01, OMW-06; unfiltered workspace | `PASS` | Disposable `.tmp/windows-parity/20260818-phase5-resume/` | Locked/offline workspace and Warp checks passed; native Credential Manager survived two fresh helper reads and deleted cleanly; GUI rebuilt in 6m32s; all eight forbidden hostname counts were zero |
| 20260818-phase5-package | 2026-08-18 | release EXE `BB6BE967...ACD8F8`; ZIP `1BBD73EC...BA4E476` | Win 22631.6199 x64 | WI-11, WI-12 prerequisite, OMW-03 prerequisite, OMW-06 packaged helper | `PASS` for package construction and extracted-runtime probes | Disposable `.tmp/windows-parity/20260818-phase5-package/` | 16,761/16,761 payload hashes passed; bundled Node/kernel/helper completed a deterministic streamed turn; helper credential cleanup and eight-hostname audit passed; fresh-account GUI lane remains |
| 20260818-phase5-package-gui | 2026-08-18 | release EXE `BB6BE967...ACD8F8`; ZIP `1BBD73EC...BA4E476` | Win 22631.6199 x64 | WI-12 current-account smoke; OMW-03 partial GUI route/fall-through; OMW-06 packaged GUI helper | `PARTIAL PASS` | Disposable `.tmp/windows-parity/20260818-phase5-package-gui/` | Clean-PATH release GUI was responsive and used packaged children; one exact `#` request reached the loopback model; `##` and malformed-prefix inputs caused no request; normal shell marker and cleanup passed. D3D response capture and fresh-account/multi-pane gates remain. |
| 20260818-phase5-settings-secret-regression | 2026-08-18 | source-only test addition; packaged release unchanged | Win 22631.6199 x64 | OMW-05 Settings secret lifecycle | `PASS` | Disposable `.tmp/windows-parity/20260818-phase5-package-gui/settings-secret-regression.txt` | Apply/reference-only TOML/keychain read, rename migration, and remove/delete passed. The stale incomplete-draft expectation was updated to the intentional omit-on-Apply behavior; full binary passed 8/8. Pinned Rust 1.92 direct linking reused prior libraries to avoid another multi-GiB build. |
| 20260818-phase5-omw12 | 2026-08-18 | source-only implementation; packaged release unchanged | Win 22631 x64 | OMW-12 | `PASS` for Windows path/I/O contract | Disposable `.tmp/windows-parity/20260818-phase5-omw12/final-assessment.txt` | `%LOCALAPPDATA%` default, `%USERPROFILE%` fallback, exact override, first-run bootstrap, and edit preservation passed; full `omw-config` suite passed 64/64. Packaged fresh-account testing remains WI-12. |
| 20260818-phase5-package-refresh | 2026-08-18 | release EXE `370529CD...EFF48`; ZIP `E3D086CD...C6B0F9` | Win 22631 x64 | WI-11, WI-12 prerequisite, OMW-01, OMW-06, OMW-12 packaged inclusion | `PASS` for refreshed package and extracted-runtime probes | Disposable `.tmp/windows-parity/20260818-phase5-package-refresh/final-assessment.txt` | OMW-12 included; 16,761/16,761 hashes and eight-hostname audit passed; deterministic packaged agent/helper route and credential cleanup passed. Fresh-account GUI lane remains WI-12. |
| 20260818-phase5-package-migration-repair | 2026-08-18 | release EXE `68FCBDF7...C533`; ZIP `25648A9E...C10F4` | Win 22631.6199 x64 | WI-11, WI-12 prerequisite, OMW-01; upgrade-profile migration | `PASS` | Disposable `.tmp/windows-parity/20260818-phase5-package-refresh-migration/final-assessment.txt` | Forward idempotent repair handles absent/existing `welcome_panes`; reproduced profile moved table/ledger 0/0 to 1/1; 16,761/16,761 hashes, no-cloud audit, packaged agent turn, and credential cleanup passed. |
| 20260818-phase5-p0-gui-omw03 | 2026-08-18 | release EXE `68FCBDF7...C533`; ZIP `25648A9E...C10F4` | Win 22631.6199 x64 | OMW-03, OMW-06 packaged use | `PASS` | Disposable `.tmp/windows-parity/20260818-phase5-p0-gui-omw03/final-assessment.txt` | Exactly one stripped-prefix request; delayed stream persisted on originating pane A while focus and a shell marker moved to distinct pane B; `##` and malformed prefix executed in PowerShell with zero requests; cleanup passed. |
| 20260818-phase5-p0-gui-omw04-checkpoint | 2026-08-19 | staged fixed release EXE `E049AA6D...F0AAC6`; final ZIP pending | Win 22631.6199 x64 | OMW-04 partial GUI | `PARTIAL PASS; FIXED RELEASE RETEST READY` | Disposable `.tmp/windows-parity/20260818-phase5-p0-gui-omw04/release-rebuild-assessment.txt` | Old candidate proved human decision dispatch and exposed the missing approved audit row. The normal full-debug rebuild exceeded 16 GiB RAM; a scoped four-codegen-unit/no-warp-debug-symbol release build passed in 28m09s, the temporary override was removed, source/staged hashes match, and all eight forbidden-hostname counts are zero. Human Reject/Approve retest remains. |
| 20260819-phase5-final-release-omw04 | 2026-08-19 | tagged EXE `0129B224...435F3E`; ZIP `3E3D9AC3...29FAD21` | Win 22631.6199 x64 | WI-11, WI-12 prerequisite, OMW-01, OMW-04, OMW-06 packaged helper | `PASS`; WI-12 fresh-account lane deferred | Disposable `.tmp/windows-parity/20260819-phase5-p0-gui-omw04-tagged/final-release-assessment.txt` | Locked/offline 16 GiB-safe release completed; 16,761/16,761 manifest hashes and archive safety checks passed; fail-closed no-cloud audit passed; exact tagged EXE Reject/Approve produced 0/1 audit rows and full cleanup. |
| 20260819-phase5-p0-gui-omw05-refreshed | 2026-08-19 | intermediate EXE `2F60A82F...276DD9` | Win 22631.6199 x64 | OMW-05 | `PARTIAL PASS` | Disposable `.tmp/windows-parity/20260819-phase5-p0-gui-omw05-refreshed/gui-assessment.txt` | At that checkpoint, OpenAI and Anthropic GUI paths, Apply/removal, restart/default persistence, reference-only config, credential cleanup, and normal close were exercised; compatible/Ollama Test routes, invalid/edit/status evidence, and the then-current exact EXE remained. |
| 20260819-phase5-omw02-manual-update-fix | 2026-08-19 | source plus production-feature check; final EXE `CA8F2CD6...8038AC6` | Win 22631.6199 x64 | OMW-02 implementation; WI-11; OMW-01 | `PASS` for implementation/package gates | Disposable `.tmp/windows-parity/20260819-phase5-omw02-manual-update-fix/final-assessment.txt` | Preserved explicit manual update while disabling automatic omw_local launch/focus/timer polling; focused regressions and shipped-feature production check passed. ZIP `8AF1C459...BDD8979` passed 16,759/16,759 manifest hashes, archive/runtime checks, clean-PATH probes, and no-cloud audit. |
| 20260819-phase5-omw02-runtime-regression | 2026-08-19 | exact EXE `CA8F2CD6...8038AC6`; ZIP `8AF1C459...BDD8979` | Win 22631.6199 x64 | OMW-02 automatic-egress subset | `PASS` for scoped subset; full row `NOT TESTED` | Disposable `.tmp/windows-parity/20260819-phase5-omw02-runtime-regression/final-assessment.txt` | Exact packaged GUI ran 660 seconds across three durable focus cycles and the ten-minute boundary with zero proxy requests, zero GitHub attempts, and zero public scoped TCP observations; normal close and cleanup passed. Settings/terminal/agent/Phone plus elevated trace remain. |
| 20260819-phase5-p0-gui-omw04-ca8f | 2026-08-19 | exact EXE `CA8F2CD6...8038AC6`; ZIP `8AF1C459...BDD8979` | Win 22631.6199 x64 | OMW-04 | `PASS` | Disposable `.tmp/windows-parity/20260819-phase5-p0-gui-omw04-ca8f/final-assessment.txt` | Exact package Reject yielded no target and 0 audit rows; Approve yielded one exact target and one exit-0 audit row. Provider count was 4 and credential/process/listener cleanup passed. Four preceding synthetic-input attempts were procedurally invalid before readiness and are retained separately. |
| 20260819-phase5-omw05-secret-fix-rebuild | 2026-08-19 | exact EXE `B37E60A8...D027C`; ZIP `4BF5DF5E...B17AD` | Win 22631.6199 x64 | WI-11/WI-12 prerequisite, OMW-01, OMW-05 fix inclusion | `PASS` for build/package integrity | Disposable `.tmp/windows-parity/20260819-phase5-omw05-secret-fix-rebuild/assessment.md` | Single-job locked/offline release completed on the 16 GiB host; all 16,759 manifest hashes, archive/runtime checks, clean-PATH probes, package-lock preservation, PDB exclusion, and no-cloud audit passed independently. |
| 20260819-phase5-p0-gui-omw05-b37e | 2026-08-20 | exact EXE `B37E60A8...D027C`; ZIP `4BF5DF5E...B17AD` | Win 22631.6199 x64 | OMW-05 | `PASS` | Disposable `.tmp/windows-parity/20260819-phase5-p0-gui-omw05-b37e/final-assessment.txt` | Two exact-package GUI runs covered four provider kinds, invalid validation, masking, edit/default/restart, keyed-to-Ollama live-buffer clearing, configured removal, exact 6-request auth/path totals, one generic Anthropic denial, zero leaks, two normal closes, and complete scoped credential/process/listener cleanup. |
| 20260820-phase5-p0-gui-omw04-b37e | 2026-08-20 | exact EXE `B37E60A8...D027C`; ZIP `4BF5DF5E...B17AD` | Win 22631.6199 x64 | OMW-04 | `PASS` | Disposable `.tmp/windows-parity/20260820-phase5-p0-gui-omw04-b37e/final-assessment.txt` | Reject yielded no target and 0 audit rows; Approve yielded one exact target with the exact sentinel and one exit-0 audit row. Provider count was 4; normal GUI close and credential/process/listener cleanup passed without forced cleanup. |
| 20260820-phase5-omw02-b37e-desktop-capture | 2026-08-20 | exact EXE `B37E60A8...D027C`; ZIP `4BF5DF5E...B17AD` | Win 22631.6199 x64 | OMW-02 diagnostic; omw_local listener review | `REVIEW` | Disposable `.tmp/windows-parity/20260820-phase5-omw02-b37e-desktop-capture/final-assessment.txt` | One keyless Settings request and clean outbound observations passed, but strict named-port capture found GUI-root listener 9277. Source attributed it to the upstream install-detection HTTP server. Terminal/agent were intentionally not completed; normal close and cleanup passed. |
| 20260820-phase5-omw02-9277-fix-rebuild | 2026-08-20 | exact EXE `E5BD8769...4303C`; initial ZIP `DE95E5A1...2EDEE0` | Win 22631.6199 x64 | OMW-02 implementation; WI-11; OMW-01 | `PASS` for implementation/package gates | Disposable `.tmp/windows-parity/20260820-phase5-omw02-9277-fix-rebuild/` | omw_local no longer registers the upstream 9277 server. Single-job production check/package completed; all 16,759 manifest hashes, archive safety, required runtime files, source/staging/ZIP identity, package-lock preservation, PDB exclusion, and forbidden-hostname audits passed. |
| 20260820-phase5-omw02-e5bd-desktop-capture | 2026-08-20 | exact EXE `E5BD8769...4303C`; ZIP `DE95E5A1...2EDEE0` | Win 22631.6199 x64 | OMW-02 diagnostic; keyless Ollama session review | `REVIEW` | Disposable `.tmp/windows-parity/20260820-phase5-omw02-e5bd-desktop-capture/final-assessment.md` | Settings and terminal passed with no forbidden network observation and 9277 absent. The local-agent route correctly intercepted the marker but failed before HTTP with `No API key for provider: ollama`, exposing a persistent-session implementation defect. Normal close, full cleanup, and unchanged v2rayN/system proxy passed. |
| 20260820-phase5-omw02-keyless-ollama-agent-refresh | 2026-08-20 | exact EXE `E5BD8769...4303C`; refreshed ZIP `072066AB...D1C33` | Win 22631.6199 x64 | OMW-02 fix inclusion; package integrity | `PASS` | Disposable `.tmp/windows-parity/20260820-phase5-omw02-keyless-ollama-agent-refresh/final-assessment.md` | Persistent keyless Ollama now satisfies the locked SDK without transmitting Authorization. Request-boundary regression, typecheck, and 90-pass agent suite succeeded. Payload-only refresh changed two expected emitted JS files, invoked no Cargo/linker, and passed exhaustive manifest/archive verification. |
| 20260820-phase5-omw02-e5bd-agentfix-r2 | 2026-08-20 | exact EXE `E5BD8769...4303C`; ZIP `072066AB...D1C33` | Win 22631.6199 x64 | OMW-02 bounded desktop Settings/terminal/local-agent | `PASS` for bounded desktop scope | Disposable `.tmp/windows-parity/20260820-phase5-omw02-e5bd-agentfix-r2/final-assessment.md` | Exactly one keyless Settings request and one keyless streaming agent request used only the named loopback fixture; no Authorization, denials, public/10808/9277/unnamed-loopback process-tree TCP, or proxy mutation. Operator confirmed Settings, terminal, agent response, and normal close. Full process/listener/credential cleanup passed. Phone/Tailscale, UDP/DNS attribution, and elevated process-filtered ETW/WFP coverage remain deferred. |
| 20260820-phase5m-windows-integration-ci | 2026-08-20 | package invariant: exact EXE `E5BD8769...4303C`; ZIP `072066AB...D1C33` | Win 22631.6199 x64 | WI-01, WI-13, root Rust/ConPTY/keychain, agent, Web, BYORC | `LOCAL PASS`; hosted CI pending | Disposable `.tmp/windows-parity/20260820-phase5m-windows-integration-ci/final-assessment.md` | 363/364 root tests passed with one explicit ignore; 2/2 Windows remote start/stop tests including live status/exact port, Windows `cmd.exe /Q`, doctests, fmt, full clippy, Web build + 114/1 Vitest, BYORC typecheck, and agent 92/1 with real helper passed. Focused Windows CI plus injection-hardened retained release build/no-cloud logs and LF ZIP sidecar are implemented; first clean hosted run remains required. Package hashes, lockfile, v2rayN/Xray, process, listener, and credential cleanup invariants held. |
| 20260826-gutter-pane-phone-r1 | 2026-08-26 | exact EXE `CBBF6485...650CA`; ZIP `A47BEDA4...C7372A` | Win 22631.6199 x64 | line-number gutter; persistent per-pane phone action; WI-11/OMW-01 package gates | `AUTOMATED PASS`; desktop/iPhone pending | Disposable `.tmp/windows-parity/20260826-gutter-pane-phone-r1/` | Real Windows Hack shaping proved the prior `em_width` underestimate and passed 12 focused gutter regressions; persistent phone sharing passed 8 focused action/pane tests plus pair, pane-share, status-stream, exact write-opt-in, agent-routing, and inline-parser gates. Locked/offline release packaging passed clean-PATH helper/kernel probes, required payloads, 16,769 manifest hashes, archive safety, PDB exclusion, source/staged/ZIP EXE identity, package-lock preservation, LF sidecar, and independent source/staged no-cloud audits. Global `cargo fmt --all -- --check` remains red only on documented pre-existing stripped/upstream formatting drift; touched Rust files pass scoped formatting. Desktop visual and physical-iPhone lanes are `NOT RUN`. |
| 20260826-windows-installer-r1 | 2026-08-26 | exact EXE `CBBF6485...650CA`; installer `46FE84D4...F1BA8F` | Win 22631.6199 x64 | WI-14 installer, upgrade, uninstaller, release integration | `PASS`; unsigned | Local production installer plus disposable `%TEMP%\omw-installer-smoke-*` lifecycle, fully removed | NSIS 3.12 built a 131,694,785-byte per-user installer from the manifest-verified 16,769-file staging payload. Full installed hashes, Apps & Features metadata, Start Menu target, locked-upgrade preservation, whole-payload replacement, full upgraded hashes, silent uninstall, LF sidecar, and zero registry/shortcut/temp residue passed. The release workflow builds, tests, retains logs for, and uploads installer plus portable ZIP. |

## Repository references

- [`PRD.md`](../PRD.md): provider, agent, approval, pairing, Web Controller,
  security, and launch requirements.
- [`specs/test-plan.md`](../specs/test-plan.md): trust tiers, GUI strategy,
  pre-release gates, and manual phone requirements.
- [`vendor/warp-stripped/OMW_LOCAL_BUILD.md`](../vendor/warp-stripped/OMW_LOCAL_BUILD.md):
  local feature build and no-cloud audit.
- [`docs/mobile-web-controller-phone-qa.md`](mobile-web-controller-phone-qa.md):
  phone/Web Controller QA ladder and resize expectations.
- [`docs/mobile-remote-control-qa.md`](mobile-remote-control-qa.md): real-shell
  mobile control journey.
- [`docs/v0.4-thin-functional-gaps.md`](v0.4-thin-functional-gaps.md): shared
  remote-control limitations that must not be mislabeled Windows regressions.
