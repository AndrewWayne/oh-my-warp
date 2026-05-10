# Mobile Web Controller Phone QA

This runbook verifies the current local branch on a real iPhone before push.
It uses the production Web Controller build served by a local mock omw host, so
Safari exercises the real pairing, session, terminal, WebSocket, resize, and
shortcut-strip code paths without needing a deployed build.

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
   npm run qa:mobile-web:auto
   ```

   This builds the Web Controller, starts the local mock omw host on a free
   loopback port, launches Chrome with iPhone viewport/touch emulation, opens
   the real pair URL, drives the terminal journey, captures screenshots, and
   writes a JSON report under `.gstack/qa-reports/mobile-web-auto-*`.

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
   `npm run qa:mobile-web`, then open the printed URL on the iPhone.

3. **Future native-iOS automation lane**: use once wired for pre-push Safari
   coverage. This should drive iOS Simulator Safari for routine checks and a
   USB-connected iPhone for release/PR confidence. It is the lane that can
   cover the real iOS software keyboard and native touch/scroll behavior.

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
OMW_QA_PUBLIC_BASE_URL=http://100.95.88.74:8787 npm run qa:mobile-web
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
- Pressing Enter sends input and echoes output.
- Primary shortcut strip sends Shift-Tab, Esc, Tab, Ctrl-C, Up, Down, Enter.
- More drawer opens and sends Ctrl-D, Ctrl-L, `/`, `|`, `?`, Left, Right.
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
