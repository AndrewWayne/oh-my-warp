# Mobile Web Controller Phone QA

This runbook verifies the current local branch before push. The fast lanes use
the production Web Controller build served by a local mock omw host, so Safari
exercises the real pairing, session, terminal, WebSocket, resize, and
shortcut-strip code paths without needing a deployed build. The fullest local
lane uses Simulator Safari against a real `omw-remote` server, a real shell,
Claude Code, and Codex CLI in a disposable QA workspace.

## Quick Start

- `npm run qa:mobile:web` — run this on every phone terminal PR.
- `npm run qa:mobile:ios` — add this when native Safari keyboard, shortcut, or
  scroll behavior matters.
- `npm run qa:mobile:remote-control` — add this before pushing terminal UX
  changes that must work through the real omw remote-control path.
- `npm run qa:mobile:remote-control:manual` — use this for hands-on Simulator
  or physical-phone QA against a real shell, Claude Code, and Codex CLI.

For one-time native iOS setup:

```bash
npm install
npm run qa:mobile:ios:install
npm run qa:mobile:ios:doctor
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
   npm run qa:mobile:web
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

2. **Manual real-phone mock-host lane**: use when the change affects native
   keyboard feel, thumb ergonomics, or browser chrome. Start the host with
   `npm run qa:mobile:web:manual`, then open the printed URL on the iPhone.

3. **Native iOS automation lane**: use for pre-push Safari coverage when the
   change touches native keyboard behavior, shortcut reachability with the
   keyboard open, or touch scrolling.

   Local setup:

   ```bash
   npm install
   npm run qa:mobile:ios:install
   npm run qa:mobile:ios:doctor
   ```

   Run:

   ```bash
   npm run qa:mobile:ios
   ```

   `qa:mobile:ios:install` installs the XCUITest driver into repo-local
   `.tmp/appium`, and `qa:mobile:ios:doctor` verifies the installed Appium
   driver plus available Xcode simulator runtimes/devices. On this Mac, the
   native QA simulator is named `omw QA iPhone`.

   The runner starts the same mock host as the browser lane, boots Simulator
   with the software keyboard forced on, launches real Mobile Safari through
   Appium/XCUITest, opens the pair URL, and uses host WebSocket logs as the
   source of truth for terminal input/control bytes. It captures screenshots at
   terminal-connected, keyboard-visible, More-drawer, and scroll milestones.
   The normal command builds the current branch before serving it; only set
   `OMW_QA_SKIP_BUILD=1` while debugging the runner itself, not for final
   pre-push evidence.

4. **Native iOS remote-control lane**: use before pushing terminal UX changes
   that should hold up with the real remote-control server, shell, Claude Code,
   and Codex CLI, not only the byte-asserting mock shell.

   ```bash
   npm run qa:mobile:remote-control
   ```

   This reuses the native iOS runner but starts a QA-only `omw-remote` harness
   instead of the mock host. The harness creates a disposable workspace under
   the report directory, starts the journey at Sessions, drives Start a new
   shell, reconnects the same shell, launches Claude Code from the phone-started
   shell, verifies `/help`, returns to the shell, smokes Codex CLI, stops the
   session, starts a fresh shell, and records PTY input/output through
   `OMW_INPUT_DUMP` and `OMW_BYTE_DUMP`. It writes screenshots plus byte-dump
   evidence under `.gstack/qa-reports/mobile-ios-remote-control-*`.

   The mock iOS lane remains the exact assertion for every shortcut byte and
   tiny-resize regression. The remote-control lane proves that the same mobile
   UI can drive a real TUI over the real remote server without a deploy or
   phone.

## Lessons Baked Into QA

The issue #20 trajectory exposed several things that browser-only automation
missed. Keep these explicit in future QA changes:

- Native keyboard and browser-owned accessory behavior must be checked in the
  native iOS lane or on a real phone; Chrome mobile emulation cannot prove it.
- Terminal resize assertions must inspect host-bound control frames and reject
  tiny rows/cols, because a visual pass can still hide a SIGWINCH storm.
- Shortcut taps must be verified by exact byte sequences in the host log, not
  by button visibility alone.
- Native runner failures should keep host logs and screenshots before teardown;
  losing evidence makes coordinate and keyboard issues much slower to diagnose.
- Do not serve a stale Web Controller `dist` for final QA. The native lane must
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
OMW_QA_PUBLIC_BASE_URL=http://100.95.88.74:8787 npm run qa:mobile:web:manual
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

## Known Limits

This is a real iPhone Safari pass, but it is still a local mock-host pass. It
does not prove production hosting, production TLS/CDN headers, installed PWA
behavior, or the real desktop Phone button/QR cold path.

iPhone Mirroring can also route text through the Mac keyboard and active input
method, so always do at least one physical-phone typing pass when keyboard
behavior is the thing under test.
