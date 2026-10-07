# Upstream Tracking for Maintenance Releases

The macOS 27 maintenance release starts from the exact omw 0.0.11 release
commit. It adopts selected, reviewed upstream fixes while retaining omw's
local mode, agent, notification, file-link, and other existing features.
See [the 0.0.13 provenance record](upstream/v0.0.13.json).

The user-authorized 0.0.13 Mac update also adopts four selected 0.0.12
improvements: provider connection tests, keyless Ollama, persistent per-pane
phone controls, and pairing-link log privacy. Rust additions are Mac-gated;
the Agent authentication workaround is gated by `process.platform === "darwin"`.
Windows behavior and artifacts remain unchanged. The exact reference and scopes
are in [the 0.0.13 provenance record](upstream/v0.0.13.json). The earlier
maintenance candidate was unpublished, so its repairs and these additions share
the 0.0.13 release. Retired internal candidate records are in `upstream/archive/`.

## Preservation boundary

The user's explicit requirement is to maintain omw as an omw release based
on 0.0.11, preserving its local modifications. Upstream changes must be
adapted within that baseline. A whole-tree replacement or a conversion to
the upstream product is outside this maintenance task.

The approved production changes are limited to native window event routing,
file-reference location parsing and opening, notification hook setup,
detection and dispatcher compatibility, permission feedback/native callbacks,
and API-key action log formatting. Source-position repairs include the LSP
entry-point conversions required by the shared file-opening contract. Test
exports, regression tests, provenance packaging, and release documentation
support those changes. Existing omw feature flags and agent, provider, remote,
tab/pane, and update implementations are retained from the baseline.

Future synchronization phases must preserve omw's existing behavior and
adapt conflicting upstream changes to that behavior. Each phase requires
verification of affected omw consumers before integration. The phase list
below is an order for evaluating changes, not authorization to replace our
implementation with a new upstream tree.

## Phased synchronization

Phased synchronization groups changes by affected subsystem and verifies
each group before proceeding. A practical order is:

1. macOS window events and input methods: verify native controls, resize,
   pane/tab dragging, and text input.
2. Terminal parsing and file links: verify Codex output, OSC 8, wrapped
   paths, Chinese filenames, and existing link formats.
3. Rendering and dependency changes: handle larger module/toolchain
   migrations, then repeat agent, local-mode, remote, and no-cloud checks.

This release implements the four approved maintenance areas and the
subsequently authorized file-position and notification repairs. Later
phases are candidates for separate work, not a claim that the whole vendor
tree already matches current upstream.

## Commit records

A commit SHA identifies an exact source revision. A release label such as
0.0.13 identifies our package, but cannot by itself describe which upstream
changes it includes. The provenance record captures the omw baseline,
the original vendor snapshot when known, each adopted commit, and local
adaptations. This lets a future maintainer identify missing fixes, avoid
applying a patch twice, and trace a regression to its source.

The exact upstream SHA of the original imported vendor tree is currently
unknown and is recorded as `null`. Selected adopted commits do not change
that field into a claim of a complete upstream synchronization.
