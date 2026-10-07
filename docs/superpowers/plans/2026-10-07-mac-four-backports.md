# Mac Four Backports Implementation Plan

**Goal:** Ship a 0.0.13 Mac preview retaining all completed maintenance and custom
0.0.11 behavior, with provider connection tests, keyless Ollama, persistent
per-pane phone sharing, and pairing-link log privacy. Windows stays unchanged.

**Stable references:** User authorized these four items and background work;
foreground acceptance is already authorized. `CLAUDE.md`, `specs/fork-strategy.md`,
`docs/upstream/v0.0.13.json`, and `docs/macos27-maintenance-verification.md` apply.
Baseline is `24c06811dee6cb6b22d715c69657af909ade8d4d`. Upstream reference is
`299b55e1c53b685fc2ee665b6beb3b8f28e21eb8` (0.0.12); adopt selected changes only.

**Repository operations:** Continue inline in `fix/macos27-maintenance` at
`worktrees/macos27-maintenance`. Preserve its existing dirty maintenance changes,
the original unpublished DMGs in `dist/archive/2026-10-07-unpublished/`, and other
worktrees. No commit, push, install, publish,
Windows package rebuild, or full upstream merge is authorized or needed.

**Recovery state:** The previous maintenance work is complete. This plan begins
the four-item backport. Upstream source references are in `.tmp/upstream-012/`.
Read the progress below before resuming; regenerate evidence after relevant edits.
Use CLT Git, Rust 1.92.0, isolated QA identity, and the documented Metal mount.

## Compatibility Contract

- Rust behavior and UI additions use `cfg(target_os = "macos")`, combined with
  `omw_local` for terminal remote controls. Existing non-Mac branches remain.
- Agent keyless handling runs only on `process.platform === "darwin"`.
- Preserve API-key action Debug redaction, window/path/notification fixes,
  same-pane PTY sharing, read-only pairing defaults, and existing remote endpoints.
- Real Windows execution, older macOS, physical phone and Tailscale testing may
  require external environments; report these limits explicitly.

### Task 1: Keyless Ollama

**Project meaning:** Local Ollama without a key must stream model responses.
**Architecture relationship:** `apps/omw-agent/src/session.ts` supplies options to
the existing pi-agent-core / pi-ai loop. A sentinel satisfies SDK validation;
an explicit null Authorization override prevents fake credentials on the wire.
**Technical contract:** Mac-only keyless Ollama options; keyed Ollama and all
other providers retain their authentication. No dependency upgrades.
**Dependencies:** Independent of Rust changes; rebuild the packaged Agent later.
**Semantic acceptance:** Mock real HTTP SSE and observe assistant output, absent
Authorization and zero key lookups; simulate win32 and verify old options and
behavior. Obtain pre-fix failure and passing post-fix evidence.
**Recovery checkpoint:** Record focused test and packaged-agent results below.

### Task 2: Provider Connection Tests

**Project meaning:** Settings can check the configured provider before saving.
**Architecture relationship:** `settings_view/omw_agent_page.rs` owns ephemeral
request IDs/status, live input flushing, keychain resolution, async GET `/models`,
and the Test control. Existing reducer, config persistence and secret handling
remain owners of their current behavior.
**Technical contract:** Mac-only fields/actions/helpers/UI. No redirects; 15 s
timeout; generic errors without response bodies, credentials or URLs. Edits,
removal, Apply and Discard invalidate results. Preserve custom Debug redaction.
**Dependencies:** Use existing reqwest and async-compat runtime.
**Semantic acceptance:** Local HTTP tests for keyless/keyed headers, success,
401, refused connection, and redirect refusal; reducer stale-result tests;
isolated GUI Test success/failure and edited-input readback. Explain that this
checks connectivity/authentication, not actual inference/model availability.
**Recovery checkpoint:** Export test helpers via existing test-exports and adapt
state literals with cfg fields; app integration runs serial.

### Task 3: Persistent Phone Entry And Pairing Privacy

**Project meaning:** Phone share/stop stays available while a terminal program
uses the alternate screen; token-bearing pair URLs stay out of Mac process logs.
**Architecture relationship:** New Mac terminal module uses existing
`OmwRemoteState` and `pane_auto_share::share_self_pane`. Header icon, overflow
menu and command palette dispatch one action. Lifetime subscriptions refresh
state; detach cancels pending work and releases only the closing pane.
**Technical contract:** Preserve Windows footers and PTY code. Copy/open pair URL
only through intentional pairing UI. Mac status Debug and startup/modal logs
must redact it; UI retains URL for pairing. No new endpoints or shell process.
**Dependencies:** Inspect existing detach/Drop lifecycle before porting. Adopt
upstream generation/cancellation mechanics only where needed for safe startup.
**Semantic acceptance:** Compile lifecycle/presentation tests, captured logging
with a known token, GUI header/overflow during alternate screen, two-pane
isolation, local browser pairing and read-only terminal output. Closing or
stopping one pane leaves other shares intact; last unshare cleans the daemon.
**Recovery checkpoint:** Upstream module contains a token-printing modal path;
remove that in the Mac adaptation, do not copy it unchanged.

### Task 4: Package And Verify 0.0.13

**Project meaning:** Deliver an inspectable Mac artifact and exact provenance.
**Architecture relationship:** Existing Mac build script bundles optimized
terminal, rebuilt Agent, Node, web controller, CLI and helper resources.
**Technical contract:** Combined release notes, `docs/upstream/v0.0.13.json`, and a
verification report. Preserve retired candidates in the archive. No Windows files.
**Dependencies:** All three behavior deliverables accepted before final package.
**Semantic acceptance:** Agent suite/build/typecheck; current serial Rust app
integration; optimized app GUI; final package signatures/minimum OS/hash; bundled
Agent Ollama HTTP request and token-free process logs; diff review of Mac gates
and non-Mac behavior. Report unavailable physical environments honestly.
**Recovery checkpoint:** Use `--test '*'`, not `--tests`: inherited upstream lib
internal tests have known compile gaps. Keep installed app untouched; stop only
the isolated QA process before replacing it.

## Progress

- Release identity corrected at the user's request: keep 0.0.13 because the
  earlier maintenance candidate was not published. Active release notes and
  provenance combine maintenance and the four additions. Exact previous records
  are archived under `docs/upstream/archive/`; generated candidates and their
  checksum files are archived under `dist/archive/2026-10-07-unpublished/`.
  Rebuilt the embedded tag, plist and DMG as 0.0.13. Fresh mounted-package and
  bundled Agent/notification checks pass. Runtime sources and previous acceptance
  limits are unchanged. Final DMG SHA-256:
  `dbe880f4ec2b0733cc20e054b6f2edefac9f4f9c02d7c1c10c4c68cb691d633a`.

- Task 1 implemented. Pre-fix Mac keyless Ollama produced no HTTP request;
  after fix all 6 request-boundary/platform cases pass. Full Agent suite:
  95 passed, 3 existing skipped; typecheck and build pass.
- Tasks 2 and 3 implemented. Privacy test failed pre-fix on the exact token
  in Debug output and passes after redaction. Provider HTTP/stale-request tests
  and phone lifecycle tests are included in 103 passing app integration tests
  (2 existing ignored), serial. Loopback probes bypass system proxies after
  a real test showed a closed localhost port was translated into HTTP 502.
- Task 4 in progress. Settings GUI success, 401 privacy, edit invalidation,
  alternate-screen header/overflow and palette stop passed. Local paired browser
  exposed a baseline read-only scope mismatch; two tests failed with HTTP 401
  before a Mac-only repair. The complete remote suite now has 103 passing tests,
  including read-only output/heartbeat, denied input, unchanged pane size,
  existing write-only attachment and rejection of unrelated scopes.
  The optimized package is built with this repair and stable Test text. Final
  mounted-package checks, bundled Agent Ollama/auth/approval/history/cancel/error
  acceptance and notification CLI lifecycle pass. The archived intermediate
  0.0.14 DMG was 271 MiB, with SHA-256
  `38e809d3aad102b163148f6e08f276b83419b4c580d892706b7652986dcca43d`.
  Final-binary GUI checks are pending because the host locked: event activation
  and screenshots fail, while the QA process remains alive. The user has been
  asked whether to unlock for full acceptance or receive the artifact with this
  explicit gap. Installed PIDs
  13611/13612/13624 are untouched. QA port 8787 was free before startup.
- Evidence: `.tmp/backport-app-tests.log`, `.tmp/backport-agent-tests.log`,
  `.tmp/backport-red-privacy.log`. New release notes and provenance exist.
- Added evidence: `.tmp/backport-red-readonly.log`,
  `.tmp/backport-green-readonly.log`, `.tmp/backport-remote-tests.log`.
- Final evidence: `.tmp/backport-final-package-check.log`,
  `.tmp/backport-final-agent-check.log`, `.tmp/backport-final-package-build.log`,
  and `docs/macos-four-backports-verification.md`.
- The final 0.0.13 package passed its checks and is ready for delivery with the
  two final GUI gaps explicitly recorded. Current logs:
  `.tmp/renumber-0013-package-build.log`, `.tmp/renumber-0013-package-check.log`,
  and `.tmp/renumber-0013-agent-check.log`.
  After the host remained locked, owned QA/browser/provider/pair-page processes
  were stopped, the clipboard restored and the toolchain mount detached. Resume
  the final GUI scenarios after unlock using the final package and the existing
  isolated QA wrapper; recreate the provider/clipboard fixtures first.
