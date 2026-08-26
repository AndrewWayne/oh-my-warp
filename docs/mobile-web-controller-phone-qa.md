# Mobile Web Controller Phone QA

This runbook verifies the current local branch before push. The fast lanes use
the production Web Controller build served by a local mock omw host, so the browser
exercises the real pairing, session, terminal, WebSocket, resize, and
shortcut-strip code paths without needing a deployed build. The fullest local
lane uses Simulator Safari against a real `omw-remote` server, a real shell,
Claude Code, and Codex CLI in a disposable QA workspace.

## Quick Start

- `npm run qa:mobile` — run this on every phone terminal PR.
- `npm run qa:mobile:full` — add this before pushing terminal UX changes that
  must work through the real omw remote-control path.
- `npm run qa:mobile:manual` — use this for hands-on Simulator or
  physical-phone QA against a real shell, Claude Code, and Codex CLI.

For one-time native iOS setup:

```bash
npm install
npm run qa:mobile:setup
npm run qa:mobile:doctor
```

## When To Use

Use this before pushing changes that touch:

- `apps/web-controller/src/pages/Pair.tsx`
- `apps/web-controller/src/pages/Sessions.tsx`
- `apps/web-controller/src/pages/Terminal.tsx`
- `apps/web-controller/src/components/TerminalShortcutStrip.tsx`
- Web Controller terminal sizing, keyboard behavior, pairing, sessions, or PTY
  WebSocket code.

## QA Ladder

Use the fastest lane that covers the risk, then move down the ladder when the
change touches behavior the browser automation cannot model.

1. **Automated mobile web lane**: run on every mobile terminal change before
   push.

   ```bash
   npm run qa:mobile
   ```

   This builds the Web Controller, starts the local mock omw host on a free
   loopback port, launches Chrome with iPhone viewport/touch emulation, opens
   the real pair URL, drives the terminal journey, captures screenshots, and
   writes a JSON report under `.gstack/qa-reports/mobile-web-mock-*`.

   It verifies:

   - Pair URL auto-redeem.
   - Single alive session auto-open.
   - Terminal WebSocket connection.
   - Normal text input into xterm.
   - Primary and overflow shortcut byte sequences.
   - Simulated visual viewport shrink for keyboard-mode layout.
   - No sub-8-row resize frames.
   - Terminal touch scrollback.
   - Back to Sessions and reopen.

   This lane is deterministic and good for regressions, but it is not native
   iOS Safari. It does not prove the real iOS keyboard, the browser-owned
   autofill accessory row, or Safari's exact scroll physics.

2. **Native iOS remote-control lane**: use before pushing terminal UX changes
   that should hold up with the real remote-control server, shell, Claude Code,
   and Codex CLI, not only the byte-asserting mock shell.

   ```bash
   npm run qa:mobile:full
   ```

   This starts a QA-only `omw-remote` harness, opens Mobile Safari in the
   simulator, creates a shell from Sessions, reconnects it, launches Claude Code
   from the phone-started shell, verifies `/help`, returns to the shell, smokes
   Codex CLI, stops the session, starts a fresh shell, and records PTY
   input/output through `OMW_INPUT_DUMP` and `OMW_BYTE_DUMP`. It writes
   screenshots plus byte-dump evidence under
   `.gstack/qa-reports/mobile-ios-remote-control-*`.

3. **Manual real remote-control lane**: use when the change affects native
   keyboard feel, thumb ergonomics, browser chrome, or physical-phone behavior.
   Start the host with `npm run qa:mobile:manual`, then open the printed URL on
   the iPhone or Simulator.

   Local setup:

   ```bash
   npm install
   npm run qa:mobile:setup
   npm run qa:mobile:doctor
   ```

   `qa:mobile:setup` installs the XCUITest driver into repo-local
   `.tmp/appium`, and `qa:mobile:doctor` verifies the installed Appium
   driver plus available Xcode simulator runtimes/devices. On this Mac, the
   native QA simulator is named `omw QA iPhone`.

   For a one-off mock-host phone pass, run
   `node scripts/qa/mobile-web-controller-host.mjs` and open the printed URL.

## Lessons Baked Into QA

The issue #20 trajectory exposed several things that browser-only automation
missed. Keep these explicit in future QA changes:

- Native keyboard and browser-owned accessory behavior must be checked in
  `qa:mobile:full` or on a real phone; Chrome mobile emulation cannot prove it.
- Terminal resize assertions must inspect host-bound control frames and reject
  tiny rows/cols, because a visual pass can still hide a SIGWINCH storm.
- Shortcut taps must be verified by exact byte sequences in the host log, not
  by button visibility alone.
- Native runner failures should keep host logs and screenshots before teardown;
  losing evidence makes coordinate and keyboard issues much slower to diagnose.
- Do not serve a stale Web Controller `dist` for final QA. The full lane must
  build first so screenshots and byte assertions reflect the current source.
- Real-TUI coverage needs a disposable workspace plus input and output byte
  dumps. Screenshots alone cannot prove Claude actually received input and
  rendered new output.
- Native scroll gestures must stay inside the terminal pane and reject literal
  keyboard bytes; otherwise a too-low drag can accidentally type on the iOS
  keyboard while pretending to test scroll. Alternate-screen TUIs may translate
  scroll into arrow escape sequences, which should be recorded rather than
  treated as keyboard-tap leakage.
- Back-to-sessions needs a journey check; a single alive session can otherwise
  bounce the user right back into Terminal.
- QA runners should preflight dependencies, use long WDA/webview timeouts, write
  screenshots and JSON summaries, and clean up Appium/simulator sessions.

## Start The Local QA Host

Build the current branch first:

```bash
npm run build --workspace @oh-my-warp/web-controller
```

Pick a phone-reachable base URL. Prefer the Mac's Tailscale IP:

```bash
tailscale ip -4
```

If `tailscale` is not on `PATH`, the app-bundled CLI is usually available:

```bash
/Applications/Tailscale.app/Contents/MacOS/Tailscale ip -4
```

Same-Wi-Fi also works when local network policy allows it:

```bash
ipconfig getifaddr en0
```

Start the host, replacing the IP with the one the phone can reach:

```bash
OMW_QA_PUBLIC_BASE_URL=http://100.95.88.74:8787 node scripts/qa/mobile-web-controller-host.mjs
```

The script binds to `0.0.0.0:8787` by default and prints:

- `phone URL`, normally `http://<reachable-ip>:8787/pair?t=ABCD1234`
- `logs`, normally `http://<reachable-ip>:8787/qa/logs`

Use `OMW_QA_MOCK_PORT`, `OMW_QA_MOCK_BIND`, or `OMW_QA_WEB_DIST` only when you
need a non-default port, bind address, or built asset directory.

## Phone Pass

Open the printed phone URL on the iPhone. Either path is useful:

- iPhone Mirroring: good for repeatable screenshots and agent-assisted tapping.
- Physical phone in hand: best for true touch, software keyboard, and thumb feel.

Verify the journey:

- Pair URL loads and auto-redeems.
- The only alive session auto-opens.
- Terminal reaches `CONNECTED`.
- The terminal shows `QA mock shell ready`.
- Tapping in the terminal allows normal text input.
- Pressing the native keyboard Return key sends Enter.
- Primary shortcut strip sends Shift-Tab, Esc, Tab, Ctrl-C, Up, Down, Left, and
  Right.
- More drawer opens and sends Ctrl-D, Ctrl-L, `/`, `|`, and `?`.
- Long host/session metadata wraps and the page cannot be panned sideways.
- The terminal remains visible when the keyboard or iOS accessory bar is present.
- Sessions button returns to the session list instead of bouncing back into the
  terminal.

Watch evidence from the Mac:

```bash
curl -sS http://127.0.0.1:8787/qa/logs
```

Reset logs between attempts:

```bash
curl -sS -X POST http://127.0.0.1:8787/qa/reset
```

Expected control bytes:

- Shift-Tab: `[27,91,90]`
- Esc: `[27]`
- Tab: `[9]`
- Ctrl-C: `[3]`
- Up: `[27,91,65]`
- Down: `[27,91,66]`
- Enter: `[13]`
- Ctrl-D: `[4]`
- Ctrl-L: `[12]`
- Slash: `[47]`
- Pipe: `[124]`
- Question: `[63]`
- Left: `[27,91,68]`
- Right: `[27,91,67]`

## Windows Packaged GUI Physical-Phone Lane

Use this lane to verify the real embedded host and a newly opened ordinary
Windows PowerShell pane, rather than the QA mock host. Start from the extracted
package under test. Before launching it, record the executable path, SHA-256,
file version, and matching source commit/build evidence so a stale binary is
ruled out. Put `tailscale.exe` on the launch-time `PATH`, resolve this PC's
Tailscale IPv4 address, and launch the packaged GUI from the same PowerShell
process:

```powershell
$tailscale = 'D:\Program Files\Tailscale\tailscale.exe'
$tailIp = (& $tailscale ip -4 | Select-Object -First 1).Trim()
$env:Path = "D:\Program Files\Tailscale;$env:Path"
$env:OMW_REMOTE_BIND = "${tailIp}:8787"
$env:OMW_REMOTE_ALLOW_DEFAULT_WRITE = "1"
$exe = 'D:\src\oh-my-warp\dist\staging-v0.0.12-dogfood.pane-phone.20260826-windows\omw-warp-oss.exe'
$expectedExeHash = 'CBBF648558EE71555712D913F9622D282E3047513C560ABF913DBE65251650CA'
$actualExeHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $exe).Hash
if ($actualExeHash -cne $expectedExeHash) {
    throw "Unexpected 0.0.12 executable hash: $actualExeHash"
}
(Get-Item -LiteralPath $exe).VersionInfo | Format-List FileVersion,ProductVersion
& $exe
```

`OMW_REMOTE_ALLOW_DEFAULT_WRITE=1` is an explicit preview-only host opt-in:
newly paired devices receive `pty:write` and can type into a shared pane. The
embedded daemon is read-only by default. An unset variable and every value
other than the exact string `1` (including `0`, `true`, and `yes`) omit
`pty:write`. The GUI samples the variable when the embedded daemon starts, so
fully stop the daemon or exit and relaunch the GUI after changing it. Do not put
pair tokens or capability tokens in retained logs.

For the interactive opt-in run:

1. Open a fresh ordinary local PowerShell pane. Before starting Codex, Claude,
   SSH, another coding agent, or a long-running command, require the persistent
   pane-header phone action to be visible with the label `Share with phone`.
   Disable the coding-agent toolbar and remove its File Explorer item if they
   are enabled; neither change may remove the pane-header action. If this
   checkpoint fails, stop and record the lane as failed.
2. In that pane, record `$PID`, print a unique GUID marker, and capture the
   scoped `pwsh.exe`, `powershell.exe`, and `cmd.exe` process inventory:

   ```powershell
   $marker = [guid]::NewGuid()
   Write-Output "OMW12_PC_PANE pid=$PID marker=$marker"
   ```

3. Click the pane-header `Share with phone` action. Confirm it changes to
   `Starting...` without accepting another activation, then to `Stop sharing`
   for this pane only. Require the pair URL to begin with
   `http://${tailIp}:8787/pair?t=`, open it in physical iPhone Safari, redeem
   it, and select the already-shared pane.
4. Confirm the phone displays the existing GUID marker. From the phone, enter
   `Write-Output "OMW_SAME_PANE_OK pid=$PID"` and verify the original desktop
   pane displays the same PowerShell PID. Capture another process inventory and
   require that no sibling shell was created.
5. Start normal `codex` without `--no-alt-screen`. While its alternate-screen
   interface is active, require the persistent pane-header `Stop sharing`
   action to remain reachable and the phone to display and control that same
   Codex session.
6. Verify desktop-to-phone output and phone-to-desktop input, disconnect Safari,
   reconnect through the tokenless base URL, and require the same pane, PID,
   Codex conversation, and per-pane sharing label.
7. Activate `Stop sharing` from the pane header. Require the local pane, its
   original PowerShell process, and the Codex process to remain alive. Confirm a
   different ordinary local pane still shows `Share with phone` and was never
   shared implicitly.

Then prove the secure default separately. Exit the GUI, remove the opt-in, start
the package again, and pair a fresh browser identity so a capability token from
the interactive run cannot be reused:

```powershell
Remove-Item Env:OMW_REMOTE_ALLOW_DEFAULT_WRITE -ErrorAction SilentlyContinue
& 'D:\src\oh-my-warp\dist\staging-v0.0.12-dogfood.pane-phone.20260826-windows\omw-warp-oss.exe'
```

From a fresh ordinary PowerShell pane, confirm the persistent pane-header action
is present, share it, and record that the fresh pair response omits `pty:write`,
that the pane is still readable, and that phone input is rejected and never
reaches the PTY. Report explicit PASS/FAIL results for the ordinary-pane action,
alternate-screen action, exact-value write opt-in, read-only default, same-pane
identity, bidirectional I/O, reconnect, unshare behavior, and absence of a
sibling shell. A source test, package build, or desktop-only run is not
sufficient to mark the physical-phone lane complete.

## Known Limits

The default Phone Pass uses real iPhone Safari but a local mock host. It does
not prove production hosting, production TLS/CDN headers, installed PWA
behavior, or the real desktop Phone button/QR cold path; use the Windows
packaged-GUI lane for that host path.

iPhone Mirroring can also route text through the Mac keyboard and active input
method, so always do at least one physical-phone typing pass when keyboard
behavior is the thing under test.
