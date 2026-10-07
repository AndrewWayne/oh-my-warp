# Mac 0.0.13 Verification

Date: 2026-10-07. Host: Apple Silicon, macOS 27.0 (26A428).

## Scope And Preservation

This release retains the omw 0.0.11 feature baseline and the completed
maintenance changes. It adapts four selected improvements from 0.0.12:
provider connection testing, keyless Ollama, persistent phone-sharing controls,
and pairing-link log privacy. Exact source references and local adaptations are
recorded in [the provenance record](upstream/v0.0.13.json).

The user retained version 0.0.13 because the earlier maintenance candidate was
not published. The intermediate 0.0.14 package is archived as an internal
candidate; both maintenance repairs and the four additions belong to this 0.0.13.
The renumbering changes release identity and documentation only. Earlier test
observations and hashes are preserved in
[the archived candidate report](upstream/archive/0.0.14-unpublished-verification.md).

The Mac adaptation was completed in the `fix/macos27-maintenance` worktree
without a whole-tree upstream merge, dependency upgrade, installed-app replacement
or Windows rebuild. The subsequent combined release preparation commits/pushes
the source and builds Windows separately; see
[the release plan](superpowers/plans/2026-10-07-v0013-draft-release.md).
Rust runtime additions are Mac-gated;
the Agent authentication adjustment checks `process.platform === "darwin"`.
Existing Windows footer/authentication paths and the shared web controller remain.

The earlier maintenance acceptance and its limits are in
[the maintenance verification](macos27-maintenance-verification.md).

## Automated Evidence

| Area | Result | Meaningful Coverage |
| --- | --- | --- |
| Agent | 95 passed, 3 existing skipped; build/typecheck pass | Real mock HTTP/SSE for keyless Mac Ollama with no Authorization or keychain lookup; keyed Ollama; unchanged win32/Linux keyless behavior; other providers still require keys |
| Application integration | 103 passed, 2 existing ignored | Provider HTTP success/401/redirect/refused connection, key headers, stale request/edit/remove/Apply/Discard; phone transitions, subscription lifetime, pane detach/drop isolation; pairing Debug privacy; inherited maintenance regressions |
| Remote protocol | 103 passed, none ignored | Read-only header/browser attachment, signed live output, heartbeat, no input or resize mutation; existing write-only attachment and unrelated-scope rejection; existing pairing/signature/replay/origin/session/PTY contracts |

Red evidence was obtained before the keyless Ollama, pairing Debug, and read-only
attachment fixes. The latter returned HTTP 401 `capability_scope` on both auth
paths because pairing granted read permission while attachment required write.
The Mac adaptation accepts read scope after write-scope rejection; input is still
rejected with code 4403 and read-only resize/control mutations are ignored.
No new remote endpoint was added, and generic signature verification is unchanged.

Application integration uses explicit `--test '*'` targets and serial execution.
The inherited upstream internal lib-test harness has known compilation gaps;
these results do not claim that the entire upstream internal harness passes.
The later stable Test-label change is compiled into the optimized package. Its
final visual check is pending because the host locked during the last acceptance
round; it does not change the previously tested reducer/HTTP behavior.

Local logs: `.tmp/backport-agent-tests.log`, `.tmp/backport-app-tests.log`,
`.tmp/backport-remote-tests.log`, `.tmp/backport-red-privacy.log`,
`.tmp/backport-red-readonly.log`, `.tmp/backport-green-readonly.log`.

## Actual UI Acceptance

All UI actions target an isolated QA bundle, `omw.local.macos27QA`, with its own
HOME/config/data/keychain and a browser-launch interceptor. The installed app and
its terminal sessions remain untouched. Pairing HTML uses an isolated TMPDIR;
the clipboard guard restores our pairing clipboard only while it is still ours.

| Scenario | Status |
| --- | --- |
| About displayed the intermediate candidate version | Passed before renumbering; final 0.0.13 identity checked in package metadata and embedded tag |
| Test uses typed, unsubmitted URL without saving | Passed against a local HTTP provider |
| Success, waiting/disabled, and HTTP 401 feedback | Passed; error response secret absent from UI |
| Editing URL clears old results; late results cannot reattach | Passed in reducer tests; UI edit clears result |
| Apply persists provider URL and clears transient test state | Passed, with on-disk TOML readback |
| Phone icon and overflow action survive alternate-screen mode | Passed using a real terminal alternate screen |
| Command palette toggles sharing; last unshare closes the daemon | Passed; local listening port closed |
| Pair URL/QR generated and browser redemption reaches existing pane | Redemption passed; initial WS scope failure repaired and covered by protocol tests |
| Actual app process/application logs omit generated pairing token | Passed with real QA token; URL/QR retained in pairing UI |
| Final optimized binary: settings restart/readback and stable Test dimensions | Pending: host screen is locked |
| Final optimized binary: browser output, two-pane isolation and close cleanup | Pending: host screen is locked |
| Final mounted DMG, bundled Agent and notification lifecycle | Passed |

## Package Evidence

Final package target:
`dist/omw-warp-oss-v0.0.13-aarch64-apple-darwin.dmg`.

The 0.0.13 rebuild and fresh package checks passed. The final combined-release
DMG changes only its outer README to clarify the separate Windows rebuild.
Size: 281,455,711 bytes (approximately 268 MiB). DMG SHA-256:
`bc6df1c2874c43ef919352761424fba96ef80455f856607e8e0e4d0b0cde6e8f`.
The preceding DMG (`dbe880f4ec2b0733cc20e054b6f2edefac9f4f9c02d7c1c10c4c68cb691d633a`)
is retained in `dist/archive/2026-10-07-unpublished/before-dual-platform-notes/`.
Fresh repack checks are in `.tmp/dual-platform-0013-package-check.log`.

Read-only mounting verified the DMG's internal integrity and companion checksum.
Strict/deep bundle signature checks passed. The app, CLI, Node and keychain helper
are arm64 and retain minimum macOS 11.0. The plist and embedded tag identify
0.0.13, with no embedded `omw-local-preview-v0.0.14` tag. Release notes and both
packaged provenance copies match the source records. Structured provenance checks
confirm the Windows preservation flags, and the no-cloud audit passed.

The mounted app and executable resources match staging. Signature-stripped
temporary copies of the source and packaged app are byte-identical after
normalizing only the signing-related `__LINKEDIT.vmsize` field. Normalized program
SHA-256: `57b81c61ac75913516e4deb1bb5b31d3d23e7473970a99a02ce6e4ffbcfc051b`.
Signed packaged app executable SHA-256:
`3f44db0b6e25bf67b74106174606f4bc7088f3606ffe0760020f71d7f96cc9e0`.
No QA diagnostics are embedded in the bundle. Previous intermediate package
hashes remain in the archived candidate report.

The final bundled Node/Agent actually streamed keyless Ollama without
Authorization, retained keyed Ollama and conversation history, cancelled a stream,
rejected overlapping prompts, honored approve/reject/cancel tool decisions and
routed approved commands to the expected pane. Provider/protocol failures behaved
as expected, and the fake key was absent from protocol output and stderr.

The mounted notification CLI passed install, idempotence, detection, five event
dispatch paths and uninstall using an isolated configuration. These checks are
recorded for the renumbered package in `.tmp/renumber-0013-package-check.log` and
`.tmp/renumber-0013-agent-check.log`; the optimized build is recorded in
`.tmp/renumber-0013-package-build.log`. The earlier intermediate candidate checks
remain in the corresponding `.tmp/backport-final-*.log` files.

The archived maintenance-only 0.0.13 DMG has SHA-256
`1c90e182cd3ea7aa0998eff87539d8420f085f081d5971bc9247935a7b316923`.
It and the retired intermediate candidate are under
`dist/archive/2026-10-07-unpublished/`, with their original checksum files.

## Limits

Local provider mocks verify transport/authentication and streaming behavior;
real OpenAI/Anthropic credentials and a real Ollama installation were not used.
The Settings test checks GET `/models`, not inference quality or model availability.
Servers that support chat but lack that endpoint can fail this narrow test.

The phone path is exercised through a local browser with real pairing and signed
requests. A physical phone, Tailscale exposure and remote mobile network are not
available for this acceptance. Read-only pairing remains the default; this work
does not add a phone write-permission UI or enable network exposure automatically.

Minimum macOS 11.0 is retained in the package, but older macOS and actual Windows
execution require those environments. This Mac acceptance supports Windows
preservation through platform gates and simulated Agent tests. The separate
Windows build is recorded in the combined release manifest; it does not change
the scope of this Mac report. This preview is ad-hoc signed and not Apple-notarized.
Installation/update swapping and cross-process production keychain authorization
were not performed. Existing logs were not rewritten.

The host locked during final optimized GUI acceptance. The QA process was alive,
but event activation and screenshots were blocked by the lock screen. Previous
source-QA UI checks and final compiled protocol/package checks are reported above;
they do not stand in for the two explicitly pending final GUI scenarios.

The QA app, mock provider, pair-page server and owned headless browser were
stopped after acceptance. The guarded pairing clipboard was restored, and the
read-only DMG/build-toolchain mounts were detached. Browser evidence is retained
under `.tmp/backport-playwright/`. The installed app was not replaced or stopped.
