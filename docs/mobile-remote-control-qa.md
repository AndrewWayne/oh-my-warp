# Mobile Remote-Control QA

Use the iOS Simulator as the default pre-push mobile remote-control gate:

```sh
npm run qa:mobile:remote-control
```

This lane opens Mobile Safari in the simulator against a real local
`omw-remote` host and a real PTY. It is intentionally broader than the mock
mobile Web Controller checks.

The simulator journey covers:

- pair and connect to the real host
- Sessions -> Start a new shell
- normal terminal command round trip
- Back to Sessions -> Open an existing shell
- Claude Code launch from the phone-started shell
- Claude Code `/help` response, proving it stayed interactive
- shortcut strip primary and overflow controls
- terminal scroll gestures
- return from Claude to shell
- Codex CLI smoke with `codex --version` and `codex --help`
- Sessions -> Stop the active shell
- Start a fresh shell after Stop

For hands-on exploratory testing, run:

```sh
npm run qa:mobile:remote-control:manual
```

The manual host starts a real shell by default and prints a fresh pair URL. Use
this when you want to drive the same remote-control flow yourself in Simulator
or on a physical phone.

Physical phone QA is still the final acceptance pass for browser-specific
keyboard/accessory behavior and real touch feel, but simulator remote-control
QA is the repeatable regression gate.
