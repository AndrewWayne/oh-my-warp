# Codex Input Editing Diagnosis

2026-10-12: The user reported difficult line deletion, copying/pasting and
selection editing in the Codex CLI composer inside oh my warp. This is a new
input-integration investigation; the earlier accepted file-link changes did not
cover these editing workflows. No application fix, rebuild, installation or
production configuration change was performed here.

## Findings

| Operation | Evidence | Attribution |
| --- | --- | --- |
| Mac Command+Backspace, labelled Delete on a Mac keyboard | `terminal/view/init.rs` binds it to Ctrl+U only under `LongRunningCommand`. `TerminalView::keymap_context` explicitly omits that context while alternate screen is active. Current Codex enables alternate screen. | Confirmed oh my warp shortcut-context gap. |
| Native Codex selection followed by Command+C | The app's Copy action reads terminal/subagent selections and selected blocks. Codex renders its own selection using cell styles; that does not create a terminal selection. The app intercepts the Copy action rather than forwarding Codex's copy key. | Confirmed integration gap in the inspected source. Actual Mac GUI dispatch was not exercised. |
| Shift+mouse drag followed by Delete | Shift forces oh my warp to intercept mouse events and create a terminal-screen selection. Deletion is sent to the running program; the terminal selection is not an editable Codex selection. | Two different selection owners, requiring clear compatibility behavior. |
| Native mouse drag followed by Delete or replacement text | Isolated Codex 0.162.1 fullscreen PTY selects BRAVO, deletes it, and replaces it successfully. Inline mode does not enable the same mouse interaction. | Current Codex has this capability in fullscreen mode. Do not claim it is absent from Codex. |
| Shift+Left selection | Sending the terminal's Shift+Left sequence six times creates no selection in either sampled default-mode keymap. Subsequent Backspace removes one character. | Codex's sampled keyboard behavior differs from a standard Mac text field. |
| Plain/Chinese/multiline paste | Bracketed Chinese and multiline paste reaches the isolated composer correctly. The app has a normal clipboard-to-PTY bracketed-paste path. | No blanket paste defect reproduced. Real Command+V, clipboard formats and the user's running session still need a specific reproduction. |

Relevant source locations:

- `vendor/warp-stripped/app/src/terminal/view/init.rs:534`: deletion binding.
- `vendor/warp-stripped/app/src/terminal/view.rs:26394`: alternate-screen context exclusion.
- `vendor/warp-stripped/app/src/terminal/view.rs:15032`: terminal Copy action.
- `vendor/warp-stripped/app/src/terminal/view.rs:14952`: Paste routing.
- `vendor/warp-stripped/app/src/terminal/alt_screen/mod.rs:11`: Shift mouse interception.

## Reproduction And Limits

The installed CLI reports 0.162.1. Existing long-lived Codex processes may still
run the version with which they started; their UI version was not inspected.
Tests launched private `--no-daemon`, read-only Codex processes in a new isolated
HOME/CODEX_HOME. A dummy provider points only to loopback port 1, with no real
credentials. Drafts were never submitted to a model. Session records contain
zero user/model turn records. Test processes were terminated afterward.

The PTY harness uses pyte 0.8.2 in an isolated uv environment to inspect rendered
cells and cursor positions. The final fullscreen and inline captures support
19 outcome assertions, including negative observations; these are diagnostic
checks, not 19 product fixes or GUI acceptance cases. Evidence lives in
`.tmp/codex-input-lab-20261012/{fullscreen-r2,inline,summary}.json`, corresponding
ANSI captures, `probe.py` and `summarize.mjs`.

The first probe's Ctrl+Insert copy directly wrote `BRAVO ` to the real macOS
clipboard. The original clipboard had not been backed up and cannot be restored
from this test. The user was informed; later probes omit copying. The first
capture also reused a multiline draft after clearing only its current line;
the final capture uses complete draft cleanup and is the accepted comparison.

These are raw keyboard/SGR-mouse tests plus source tracing. They do not replace
foreground testing of the app's Command keys, drag hit-testing, IME or clipboard
formats. No Accessibility/Screen Recording permission request or foreground
automation was performed. The installed executable remains SHA-256
`3f44db0b6e25bf67b74106174606f4bc7088f3606ffe0760020f71d7f96cc9e0`.

## Immediate Workarounds And Repair Boundary

Ctrl+U deletes from the cursor to the current line's start; Ctrl+K deletes to its
end. Ctrl+W deletes the preceding word. These worked in both sampled modes.
For editing in current Codex fullscreen mode, drag without Shift. For terminal
screen copying, Shift+drag followed by Command+C uses the terminal's selection;
that selection cannot be used to delete the corresponding input text.

A targeted repair can add Mac Codex alternate-screen mappings for line editing
and coordinate Copy with whichever selection owner is active. It should preserve
native Codex input, other TUI applications, Chinese IME and the accepted file
links. A new independent composer is a much larger change and is unnecessary
for the confirmed shortcut gap. Actual paste failures need a concrete example
before changing clipboard routing. No implementation is included in this report.

Official OpenAI documentation confirms that `/keymap` inspects and customizes
Codex shortcuts, and Ctrl+G opens the editor configured by VISUAL/EDITOR for
longer prompts. The documentation does not promise all Mac native text-field
shortcuts:

- <https://learn.chatgpt.com/docs/developer-commands?surface=cli#remap-tui-shortcuts-with-keymap>
- <https://learn.chatgpt.com/docs/cli-customization#prompt-editor>

## Undo And Redo Follow-Up

The user subsequently asked about Command+Z and Command+Shift+Z. In
`editor/view/mod.rs:402` the app registers Undo and Redo only for `EditorView`;
Codex's native composer is a PTY interface rather than that editable buffer.
The app's undo history therefore cannot undo Codex input changes.

The isolated 0.162.1 `undo` capture confirms that Ctrl+Z tears down active TUI
modes; SIGCONT sent only to the private test process group restores the same
draft. Ctrl+Z is a suspend operation, not an undo substitute. Ctrl+underscore
also leaves the sampled draft unchanged. Synthetic Super+Z/Super+Shift+Z CSI-u
sequences leave the draft unchanged; these are byte probes, not real Mac GUI
key dispatch or evidence that all custom/Vim keymaps lack undo support.

Ctrl+U followed by Ctrl+Y successfully removes and reinserts
`ALPHA BRAVO CHARLIE`. Ctrl+Y is yank from Codex's kill buffer, not arbitrary
editing undo or redo. This offers limited recovery for text deleted with the
kill shortcuts; it does not undo ordinary typing or Backspace. Full native Mac
undo/redo would require an actual composer edit-history capability and appropriate
routing, rather than mapping Command+Z to Ctrl+Z. For longer prompts, the already
documented Ctrl+G external-editor flow lets the chosen editor provide its own
undo history. No application/configuration change was made. Evidence:
`.tmp/codex-input-lab-20261012/undo.{json,ansi}`.
