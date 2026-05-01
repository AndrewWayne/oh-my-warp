# Inline Agent Command Execution Report

Status: Design report
Last updated: 2026-04-30

> **Note (2026-05-01):** path references below use `vendor/warp-fork/<...>` because the report was written against a pristine upstream submodule that has since been removed. The fork now lives in-tree at `vendor/warp-stripped/` (see `specs/fork-strategy.md` and PRD §8.5). Most cited line numbers remain accurate against `vendor/warp-stripped/`, but `omw_local` gating commits on `omw/strip-built-in-ai` may have shifted some line numbers in dispatchers and Cargo.toml files. References to `vendor/warp-fork` being "read-only" or to a sibling `oh-my-warp/warp-fork` repo are obsolete: the in-tree fork is freely editable, and there is no sibling repo.

## Executive Summary

This report compares two implementation patterns for invoking an agent from a
terminal session and proposes how `omw` should implement a custom built-in
inline agent command with `#`.

The two reference designs are:

1. Forge's zsh plugin realization of `:` commands.
2. Warp's terminal-owned in-session agent command execution.

The recommended `omw` approach is to use Forge as the UX reference and Warp as
the execution architecture:

- `# prompt` should be intercepted by the `omw` GUI input layer before it ever
  reaches the shell.
- The GUI should start an inline `omw-agent` turn through `omw-server`.
- Agent tool calls should execute shell commands inside the active visible GUI
  terminal session through a structured command API, not through hidden local
  subprocesses.
- The pi-agent bash tool should be wired through the PRD's planned
  `WarpSessionBashOperations` adapter.
- Approvals and audit should be mandatory parts of the execution path.

The shell-plugin model is still useful as a fallback or prototype, but it
should not be the core implementation.

## 1. Forge Realization: zsh/ZLE Interception

Forge implements its `:` command feature as a zsh plugin. The terminal emulator
does not need to know anything about agents. Instead, the shell plugin owns the
line editor integration and intercepts Enter before zsh executes the current
buffer.

### 1.1 Plugin Load Path

The plugin entry point is:

- `vendor/forge-code/shell-plugin/forge.plugin.zsh`

It loads modules for:

- configuration,
- syntax highlighting,
- terminal context capture,
- completions,
- action handlers,
- dispatcher logic,
- key bindings.

Relevant references:

- `vendor/forge-code/shell-plugin/forge.plugin.zsh:6`
- `vendor/forge-code/shell-plugin/forge.plugin.zsh:21`
- `vendor/forge-code/shell-plugin/forge.plugin.zsh:24`

Important module files:

- `vendor/forge-code/shell-plugin/lib/config.zsh`
- `vendor/forge-code/shell-plugin/lib/bindings.zsh`
- `vendor/forge-code/shell-plugin/lib/dispatcher.zsh`
- `vendor/forge-code/shell-plugin/lib/completion.zsh`
- `vendor/forge-code/shell-plugin/lib/context.zsh`
- `vendor/forge-code/shell-plugin/lib/helpers.zsh`

### 1.2 Enter Key Hijack

Forge registers custom ZLE widgets and binds Enter to `forge-accept-line`.

References:

- `vendor/forge-code/shell-plugin/lib/bindings.zsh:6`
- `vendor/forge-code/shell-plugin/lib/bindings.zsh:42`
- `vendor/forge-code/shell-plugin/lib/bindings.zsh:43`

This means that when the user types:

```zsh
: explain this repo
```

zsh does not immediately execute the line. The plugin receives the editor
buffer first.

### 1.3 Buffer Parser

The dispatcher checks the current `BUFFER` for supported `:` patterns.

References:

- `vendor/forge-code/shell-plugin/lib/dispatcher.zsh:90`
- `vendor/forge-code/shell-plugin/lib/dispatcher.zsh:99`
- `vendor/forge-code/shell-plugin/lib/dispatcher.zsh:108`

It recognizes forms such as:

```zsh
: prompt text
:sage prompt text
:new
:conversation
:commit
:sync
```

If the line does not match the Forge grammar, the plugin falls back to normal
`zle accept-line`.

### 1.4 History Preservation

Forge manually writes the original buffer into shell history before it runs the
agent command.

Reference:

- `vendor/forge-code/shell-plugin/lib/dispatcher.zsh:119`

This preserves the user-facing command in history. Pressing Up shows
`: prompt`, not an expanded internal `forge ...` invocation.

### 1.5 Dispatch Model

Forge dispatches built-in actions through a zsh `case` statement.

References:

- `vendor/forge-code/shell-plugin/lib/dispatcher.zsh:146`
- `vendor/forge-code/shell-plugin/lib/dispatcher.zsh:266`

Unknown or agent-named commands fall through to `_forge_action_default`.

References:

- `vendor/forge-code/shell-plugin/lib/dispatcher.zsh:10`
- `vendor/forge-code/shell-plugin/lib/dispatcher.zsh:266`

The default action can:

- set the active agent,
- run a custom Forge command,
- or execute `forge -p <prompt> --cid <conversation_id>`.

References:

- `vendor/forge-code/shell-plugin/lib/dispatcher.zsh:34`
- `vendor/forge-code/shell-plugin/lib/dispatcher.zsh:42`
- `vendor/forge-code/shell-plugin/lib/dispatcher.zsh:69`
- `vendor/forge-code/shell-plugin/lib/dispatcher.zsh:82`

### 1.6 Conversation State

Forge stores conversation and active-agent state in shell variables.

References:

- `vendor/forge-code/shell-plugin/lib/config.zsh:23`
- `vendor/forge-code/shell-plugin/lib/config.zsh:24`

This is simple and effective for a single shell session. It is not a durable
terminal model, and it does not naturally map to GUI block IDs, remote sessions,
or auditable command ownership.

### 1.7 TTY Handling

Forge has a special `_forge_exec_interactive` function that redirects stdin and
stdout to `/dev/tty`.

References:

- `vendor/forge-code/shell-plugin/lib/helpers.zsh:57`
- `vendor/forge-code/shell-plugin/lib/helpers.zsh:82`

This is necessary because a ZLE widget is not a normal command execution
environment. Without `/dev/tty`, interactive child programs may see non-TTY
stdin and fail or exit immediately.

This is an important lesson for any `omw` shell-plugin fallback.

### 1.8 Completion and Highlighting

Forge binds Tab to a custom completion widget.

References:

- `vendor/forge-code/shell-plugin/lib/bindings.zsh:45`
- `vendor/forge-code/shell-plugin/lib/completion.zsh:5`

It supports:

- `@...` file completion via `forge list files --porcelain`,
- `:...` command/agent completion via `forge list commands --porcelain`.

References:

- `vendor/forge-code/shell-plugin/lib/completion.zsh:17`
- `vendor/forge-code/shell-plugin/lib/completion.zsh:41`
- `vendor/forge-code/shell-plugin/lib/completion.zsh:55`

It also adds syntax-highlighting rules.

References:

- `vendor/forge-code/shell-plugin/lib/highlight.zsh:7`
- `vendor/forge-code/shell-plugin/lib/highlight.zsh:10`

### 1.9 Terminal Context Capture

Forge uses zsh `preexec` and `precmd` hooks to track recent commands and exit
codes.

References:

- `vendor/forge-code/shell-plugin/lib/context.zsh:69`
- `vendor/forge-code/shell-plugin/lib/context.zsh:81`
- `vendor/forge-code/shell-plugin/lib/context.zsh:119`
- `vendor/forge-code/shell-plugin/lib/context.zsh:120`

It also emits OSC 133 semantic prompt markers.

References:

- `vendor/forge-code/shell-plugin/lib/context.zsh:52`
- `vendor/forge-code/shell-plugin/lib/context.zsh:74`
- `vendor/forge-code/shell-plugin/lib/context.zsh:88`
- `vendor/forge-code/shell-plugin/lib/context.zsh:110`

Because Forge is outside the terminal emulator, it reconstructs terminal
context through shell hooks and scrollback conventions.

### 1.10 Forge Flow Summary

```text
User types ": prompt"
  -> zsh ZLE widget intercepts Enter
  -> parse BUFFER
  -> save original command to shell history
  -> create or reuse conversation id
  -> execute forge CLI child process
  -> reset ZLE buffer
```

Strengths:

- Fast to integrate.
- Works in normal terminals.
- No terminal fork required.
- Simple command grammar and completions.
- Easy to prototype.

Limitations:

- zsh-specific for the rich experience.
- Agent runs as a child CLI, not as a native terminal block.
- Shell variables hold important session state.
- Output capture and command lifecycle are indirect.
- Hard to integrate cleanly with GUI remote control, block IDs, approvals, and
  long-running command ownership.

## 2. Warp Realization: Terminal-Owned PTY and Block Execution

Warp's approach is fundamentally different. Warp owns the terminal UI, terminal
model, input editor, PTY controller, block metadata, and AI state. Agent actions
are app-level events that eventually write bytes into the PTY and observe block
lifecycle.

### 2.1 Native Agent Tool Schema

Warp's agent action model includes structured shell tools:

- `RequestCommandOutput`
- `WriteToLongRunningShellCommand`
- `ReadShellCommandOutput`
- `TransferShellCommandControlToUser`

References:

- `vendor/warp-fork/crates/ai/src/agent/action/mod.rs:36`
- `vendor/warp-fork/crates/ai/src/agent/action/mod.rs:61`
- `vendor/warp-fork/crates/ai/src/agent/action/mod.rs:126`
- `vendor/warp-fork/crates/ai/src/agent/action/mod.rs:161`

The key difference from the Forge model is that shell execution is not just a
string rewrite. It is a structured capability with typed results.

### 2.2 Permission Gate

Warp routes shell actions through `ShellCommandExecutor::should_autoexecute`.

Reference:

- `vendor/warp-fork/app/src/ai/blocklist/action_model/execute/shell_command.rs:106`

Normal command execution checks whether the command can be auto-executed.

Reference:

- `vendor/warp-fork/app/src/ai/blocklist/action_model/execute/shell_command.rs:127`

Writing into a long-running command checks PTY-write permission.

Reference:

- `vendor/warp-fork/app/src/ai/blocklist/action_model/execute/shell_command.rs:160`

This distinction matters for `omw`: executing a command and writing raw input
into a running process are separate security decisions.

### 2.3 Agent Command Execution Path

When the agent requests a command, Warp emits a shell-command executor event.

Reference:

- `vendor/warp-fork/app/src/ai/blocklist/action_model/execute/shell_command.rs:253`

`TerminalView` handles that event, associates it with an AI conversation/action,
and emits `Event::ExecuteCommand`.

References:

- `vendor/warp-fork/app/src/terminal/view.rs:6141`
- `vendor/warp-fork/app/src/terminal/view.rs:6211`
- `vendor/warp-fork/app/src/terminal/view.rs:6238`

The terminal manager wires that event to the PTY controller.

References:

- `vendor/warp-fork/app/src/terminal/writeable_pty/terminal_manager_util.rs:72`
- `vendor/warp-fork/app/src/terminal/writeable_pty/terminal_manager_util.rs:84`

The PTY controller starts a command block and writes shell-specific command
bytes.

References:

- `vendor/warp-fork/app/src/terminal/writeable_pty/pty_controller.rs:539`
- `vendor/warp-fork/app/src/terminal/writeable_pty/pty_controller.rs:551`
- `vendor/warp-fork/app/src/terminal/writeable_pty/pty_controller.rs:575`

The byte conversion function clears the current shell input line,
bracketed-pastes when possible, and appends shell-specific execution bytes.

References:

- `vendor/warp-fork/app/src/terminal/writeable_pty/pty_controller.rs:783`
- `vendor/warp-fork/app/src/terminal/writeable_pty/pty_controller.rs:789`
- `vendor/warp-fork/app/src/terminal/writeable_pty/pty_controller.rs:794`
- `vendor/warp-fork/app/src/terminal/writeable_pty/pty_controller.rs:832`

Finally, bytes are sent to the PTY event loop.

Reference:

- `vendor/warp-fork/app/src/terminal/writeable_pty/pty_controller.rs:735`

The core path is:

```text
Agent tool call
  -> ShellCommandExecutor::execute
  -> ShellCommandExecutorEvent::ExecuteCommand
  -> TerminalView::handle_shell_command_executor_event
  -> Event::ExecuteCommand
  -> terminal_manager_util
  -> PtyController::write_command
  -> bytes_to_execute_command
  -> Message::Input(bytes)
  -> PTY
```

### 2.4 Long-Running Command Interaction

Warp treats long-running command interaction separately. If the agent writes
into a running program, Warp emits `WriteAgentInputToPty`, not
`ExecuteCommand`.

References:

- `vendor/warp-fork/app/src/terminal/view.rs:6280`
- `vendor/warp-fork/app/src/terminal/view.rs:7537`
- `vendor/warp-fork/app/src/terminal/view.rs:1717`
- `vendor/warp-fork/app/src/terminal/writeable_pty/terminal_manager_util.rs:62`
- `vendor/warp-fork/app/src/terminal/writeable_pty/pty_controller.rs:617`

`AIAgentPtyWriteMode` controls how bytes are decorated.

References:

- `vendor/warp-fork/crates/ai/src/agent/action/mod.rs:693`
- `vendor/warp-fork/crates/ai/src/agent/action/mod.rs:700`

A raw keystroke, a line submission, and a bracketed-paste block are different
operations. The structured write mode lets Warp preserve that distinction.

### 2.5 Result Capture

Warp waits for block metadata or timeout and returns either:

- completed command output plus exit code,
- or a long-running terminal snapshot.

References:

- `vendor/warp-fork/app/src/ai/blocklist/action_model/execute/shell_command.rs:495`
- `vendor/warp-fork/app/src/ai/blocklist/action_model/execute/shell_command.rs:575`
- `vendor/warp-fork/app/src/ai/blocklist/action_model/execute/shell_command.rs:596`

This is the main architectural advantage over shell-only invocation: the agent
does not need to scrape terminal text blindly.

### 2.6 User "Use Agent" Entry

Warp's UI can call up Agent Mode from terminal state. The "Use Agent" footer
dispatches `SetInputModeAgent`.

References:

- `vendor/warp-fork/app/src/terminal/view/use_agent_footer/mod.rs:276`
- `vendor/warp-fork/app/src/terminal/view/use_agent_footer/mod.rs:278`

The terminal action handler decides whether to:

- ignore the request because a third-party CLI agent session is active,
- hand an agent-controlled long-running command back to the agent,
- tag the agent into a user-started long-running command,
- or switch input mode.

References:

- `vendor/warp-fork/app/src/terminal/view.rs:24859`
- `vendor/warp-fork/app/src/terminal/view.rs:24872`
- `vendor/warp-fork/app/src/terminal/view.rs:24885`
- `vendor/warp-fork/app/src/terminal/view/use_agent_footer/mod.rs:399`

### 2.7 Third-Party CLI Agent Support

Warp also supports Claude, Gemini, Codex, OpenCode, and similar third-party CLI
agents.

It models known CLI agents and command prefixes.

References:

- `vendor/warp-fork/app/src/terminal/cli_agent.rs:108`
- `vendor/warp-fork/app/src/terminal/cli_agent.rs:125`
- `vendor/warp-fork/app/src/terminal/cli_agent.rs:286`

It detects CLI agents from active long-running blocks.

Reference:

- `vendor/warp-fork/app/src/terminal/view/use_agent_footer/mod.rs:359`

It listens for structured OSC 777 plugin events with the sentinel
`warp://cli-agent`.

References:

- `vendor/warp-fork/app/src/terminal/cli_agent_sessions/event/mod.rs:12`
- `vendor/warp-fork/app/src/terminal/cli_agent_sessions/event/mod.rs:71`

Rich input writes prompts to the CLI agent PTY using agent-specific strategies.

References:

- `vendor/warp-fork/app/src/terminal/view/use_agent_footer/mod.rs:101`
- `vendor/warp-fork/app/src/terminal/view/use_agent_footer/mod.rs:121`
- `vendor/warp-fork/app/src/terminal/view/use_agent_footer/mod.rs:624`
- `vendor/warp-fork/app/src/terminal/view/use_agent_footer/mod.rs:825`

### 2.8 Warp Flow Summary

```text
User or agent action
  -> app-level terminal state machine
  -> permission and approval
  -> structured command or PTY-write event
  -> PtyController
  -> terminal block metadata
  -> output or snapshot back to agent/UI
```

Strengths:

- Correct PTY integration.
- Correct command lifecycle.
- First-class UI blocks.
- Good long-running command semantics.
- Can stream status, approvals, output, and snapshots.
- Fits remote control and audit.

Limitations:

- Requires modifying the terminal client.
- More moving parts.
- Requires internal APIs between GUI, server, and agent process.

## 3. OMW Context

The OMW PRD already points toward the Warp-style architecture.

The key requirement is that `omw-agent` should adopt pi-agent and implement
`WarpSessionBashOperations`, replacing pi-agent's isolated subprocess executor
with one that writes commands into the user's open terminal session.

References:

- `PRD.md:201`
- `PRD.md:204`

The forked client should call `omw-agent` through `omw-server`.

References:

- `PRD.md:72`
- `PRD.md:248`

The GUI is the PTY/session anchor.

References:

- `PRD.md:174`
- `docs/omw-remote-implementation.md:8`

`omw-server` is the local backend shim and audit writer.

References:

- `PRD.md:392`
- `PRD.md:422`

The current OMW crates are still phase-0 placeholders, so this design is
mostly greenfield.

References:

- `crates/omw-server/src/lib.rs:1`
- `crates/omw-remote/src/lib.rs:1`
- `crates/omw-policy/src/lib.rs:1`
- `crates/omw-audit/src/lib.rs:1`
- `apps/omw-agent/src/index.ts:1`

Also note that `vendor/warp-fork` is read-only from this umbrella repo. Actual
Warp fork patches should go to the sibling fork repo.

References:

- `specs/fork-strategy.md:53`
- `specs/fork-strategy.md:55`

## 4. Proposed OMW Feature: Native `#` Inline Agent Execution

### 4.1 User-Facing Behavior

Typing this in the OMW terminal input:

```text
# explain why the last command failed
```

should not execute a shell comment or shell command. It should:

1. create an inline OMW agent turn,
2. attach active terminal session and block context,
3. stream the assistant response inline in the terminal,
4. allow tool calls subject to approval,
5. execute approved shell commands inside the same visible terminal session,
6. audit the entire sequence.

### 4.2 Recommended Grammar

Keep v1 small:

```text
# <prompt>
```

Examples:

```text
# explain this stack trace
# run the tests and fix the failure
# what is this long-running server doing?
```

Optional v1.x extensions:

```text
#agent <agent-name> <prompt>
#model <model-id> <prompt>
#new <prompt>
```

Escape hatch:

```text
## this is a literal shell comment
```

Recommendation:

- `# ` at column 0 means inline agent.
- `##` means pass a literal `#` line to the shell or insert a shell comment.
- `#` in the middle of a normal shell command is ignored by OMW.
- Multiline buffers, heredocs, and continuation prompts should not be
  intercepted in v1.

This avoids breaking commands like:

```sh
echo foo # comment
git commit -m "#123 fix bug"
```

### 4.3 Why `#` Works Better Natively Than In The Shell

In bash, `#` is usually an interactive comment. In zsh, comment behavior
depends on options like `INTERACTIVE_COMMENTS`. A shell plugin can intercept
`#` before the shell sees it, but native OMW input handling is cleaner: the GUI
input editor can parse `#` before shell semantics apply.

Therefore:

- Native GUI `#` is the primary feature.
- zsh plugin `#` is optional compatibility.

## 5. Proposed Native Architecture

### 5.1 Component Responsibilities

```text
OMW GUI / Warp fork
  - parses # prompts
  - owns terminal session IDs, active block IDs, PTY controller
  - renders inline agent block
  - executes shell commands in visible terminal session
  - streams terminal block output/snapshots back to omw-server

omw-server
  - local broker between GUI and omw-agent
  - creates agent sessions
  - forwards command execution requests to GUI
  - owns audit writes
  - owns approval queue API

omw-agent
  - pi-agent runtime
  - provider calls
  - tool registry
  - beforeToolCall / afterToolCall policy hooks
  - WarpSessionBashOperations adapter

omw-policy
  - command classification and allowlist config
  - readonly / ask_before_write / trusted policy data

omw-audit
  - redaction
  - append-only hash chain
```

This follows the PRD component ownership map.

References:

- `PRD.md:406`
- `PRD.md:415`
- `PRD.md:416`
- `PRD.md:419`
- `PRD.md:422`

### 5.2 Required Internal Channels

The existing remote design proposes:

- session listing,
- PTY stream,
- input,
- resize.

References:

- `docs/omw-remote-implementation.md:55`
- `docs/omw-remote-implementation.md:60`

For agent command execution, OMW should add a higher-level command API in
addition to raw PTY input.

Raw PTY input is enough for remote keystrokes. It is not enough for agent tool
execution because the agent needs:

- command block ID,
- completion status,
- exit code,
- output,
- long-running snapshot,
- cancellation,
- ownership/control state.

Recommended internal APIs:

```http
GET  /internal/v1/sessions
WS   /internal/v1/sessions/:id/events
POST /internal/v1/sessions/:id/input
POST /internal/v1/sessions/:id/commands
POST /internal/v1/sessions/:id/commands/:command_id/input
GET  /internal/v1/sessions/:id/commands/:command_id/snapshot
POST /internal/v1/sessions/:id/commands/:command_id/cancel
```

The important distinction:

- `/input` writes raw bytes.
- `/commands` asks the GUI to execute a structured shell command and report
  lifecycle.

### 5.3 GUI Registration With Server

Because `omw-agent` is a sibling process, it cannot directly call Rust methods
inside the GUI. The GUI should maintain a persistent loopback control channel
to `omw-server`.

Startup flow:

```text
OMW GUI starts
  -> connects/registers with omw-server
  -> announces terminal view/session metadata
  -> streams block/PTY/session events to server
  -> listens for command-execution requests from server
```

Then:

```text
omw-agent -> omw-server -> GUI -> PTY
```

## 6. Native `#` Flow

### 6.1 Prompt-Only Happy Path

```text
User types "# explain this repo"
  -> GUI parser intercepts before shell execution
  -> GUI clears input buffer
  -> GUI creates inline agent placeholder block
  -> GUI POST /api/v1/agent/sessions to omw-server
  -> omw-server starts omw-agent session
  -> omw-agent streams assistant events
  -> GUI renders events inline
```

The PRD already sketches the agent-session endpoint.

Reference:

- `PRD.md:505`

The GUI should pass data like:

```json
{
  "origin": "terminal_hash_inline",
  "terminal_view_id": "...",
  "session_id": "...",
  "active_block_id": "...",
  "cwd": "...",
  "prompt": "explain this repo"
}
```

### 6.2 Agent Bash Tool Call

When pi-agent wants to run a shell command, the `bash` tool should not use
local subprocess spawning. It should use OMW's session adapter.

pi-agent already exposes `BashOperations`.

Reference:

- `vendor/pi-mono/packages/coding-agent/src/core/tools/bash.ts:49`

The default local implementation uses `spawn`.

References:

- `vendor/pi-mono/packages/coding-agent/src/core/tools/bash.ts:75`
- `vendor/pi-mono/packages/coding-agent/src/core/tools/bash.ts:84`

The bash tool accepts custom operations.

References:

- `vendor/pi-mono/packages/coding-agent/src/core/tools/bash.ts:152`
- `vendor/pi-mono/packages/coding-agent/src/core/tools/bash.ts:153`
- `vendor/pi-mono/packages/coding-agent/src/core/tools/bash.ts:273`
- `vendor/pi-mono/packages/coding-agent/src/core/tools/bash.ts:277`
- `vendor/pi-mono/packages/coding-agent/src/core/tools/bash.ts:347`

OMW should provide:

```ts
createBashTool(cwd, {
  operations: createWarpSessionBashOperations({
    omwServerUrl,
    terminalSessionId,
    agentSessionId,
  }),
});
```

This matches the PRD.

References:

- `PRD.md:204`
- `PRD.md:723`

### 6.3 `WarpSessionBashOperations` Flow

```text
pi-agent bash tool
  -> WarpSessionBashOperations.exec(command, cwd, onData, signal)
  -> omw-server POST /internal/v1/sessions/:id/commands
  -> omw-server forwards command request to GUI control channel
  -> GUI emits native Event::ExecuteCommand
  -> PtyController writes command into active PTY
  -> terminal block starts
  -> block output streams back through GUI -> omw-server -> omw-agent
  -> command completes or snapshots
  -> BashOperations resolves with exitCode
```

Pseudo-code:

```ts
import type { BashOperations } from "@mariozechner/pi-coding-agent";

export function createWarpSessionBashOperations(opts: {
  serverUrl: string;
  terminalSessionId: string;
  agentSessionId: string;
  toolCallId: string;
}): BashOperations {
  return {
    async exec(command, cwd, { onData, signal, timeout, env }) {
      const run = await postJson(
        `${opts.serverUrl}/internal/v1/sessions/${opts.terminalSessionId}/commands`,
        {
          agent_session_id: opts.agentSessionId,
          tool_call_id: opts.toolCallId,
          command,
          cwd,
          env,
          timeout,
          visibility: "visible",
          wait: "completion_or_snapshot"
        },
        { signal }
      );

      for await (const event of streamCommandEvents(run.events_url, { signal })) {
        if (event.type === "output") {
          onData(Buffer.from(event.bytes, "base64"));
        }

        if (event.type === "snapshot") {
          onData(Buffer.from(event.text));
        }

        if (event.type === "completed") {
          return { exitCode: event.exit_code ?? null };
        }
      }

      return { exitCode: null };
    }
  };
}
```

### 6.4 Command Execution Inside GUI

On the GUI side, OMW should reuse Warp's existing command path rather than
manually writing raw bytes.

For a visible command, the GUI should emit an event equivalent to:

```rust
Event::ExecuteCommand(ExecuteCommandEvent {
    command,
    session_id,
    source: OMWAgent { metadata },
    should_add_command_to_history: true,
})
```

Warp's analogous source path:

- `vendor/warp-fork/app/src/terminal/view.rs:6238`
- `vendor/warp-fork/app/src/terminal/writeable_pty/terminal_manager_util.rs:72`
- `vendor/warp-fork/app/src/terminal/writeable_pty/pty_controller.rs:539`

OMW will likely need a new source variant or metadata equivalent:

```rust
CommandExecutionSource::OMWAgent {
    metadata: OmwAgentInteractionMetadata,
}
```

Alternatively, the fork can reuse Warp's `CommandExecutionSource::AI` if enough
of the upstream AI metadata structure remains useful.

### 6.5 Long-Running Commands

If an agent command is long-running, OMW should not block indefinitely. Mirror
Warp's behavior:

```text
command starts
  -> wait N seconds
  -> if complete: return output + exit code
  -> if still running: return snapshot + block_id
  -> agent may later call read_shell_output(block_id)
  -> agent may write input to the running command
  -> agent may transfer control to user
```

Relevant Warp model:

- `vendor/warp-fork/app/src/ai/blocklist/action_model/execute/shell_command.rs:495`
- `vendor/warp-fork/app/src/ai/blocklist/action_model/execute/shell_command.rs:596`
- `vendor/warp-fork/app/src/ai/blocklist/action_model/execute/shell_command.rs:849`

This is the reason OMW should implement structured command execution, not only
raw PTY writes.

## 7. Where To Patch In The Warp Fork

Because `vendor/warp-fork` is read-only from this repo, these are patch targets
for the actual `oh-my-warp/warp-fork` repo.

### 7.1 Input Parser

Patch near command execution in terminal input handling.

Relevant upstream locations:

- `vendor/warp-fork/app/src/terminal/input.rs:5922`
- `vendor/warp-fork/app/src/terminal/input.rs:5970`
- `vendor/warp-fork/app/src/terminal/input.rs:13219`

Add a guard before normal command execution:

```rust
if source.is_user() {
    if let Some(prompt) = parse_hash_inline_agent_command(command) {
        ctx.emit(InputEvent::SubmitOmwInlineAgentPrompt { prompt });
        return true;
    }
}
```

Do not put the parser after `Event::ExecuteCommand`, because by then the
command may already be on its way to the shell.

### 7.2 TerminalView Handler

`TerminalView` should handle `SubmitOmwInlineAgentPrompt` by:

1. clearing the input buffer,
2. inserting an inline agent block,
3. calling `omw-server`,
4. subscribing to the agent event stream,
5. rendering streaming output, tool calls, and approvals.

Useful neighboring concepts:

- `vendor/warp-fork/app/src/terminal/view/use_agent_footer/mod.rs:276`
- `vendor/warp-fork/app/src/terminal/view.rs:24852`
- `vendor/warp-fork/app/src/terminal/view/use_agent_footer/mod.rs:399`

### 7.3 Command Execution Broker

When `omw-server` asks the GUI to execute a command, the GUI should route
through Warp's existing command path:

- `vendor/warp-fork/app/src/terminal/view.rs:6238`
- `vendor/warp-fork/app/src/terminal/writeable_pty/terminal_manager_util.rs:84`
- `vendor/warp-fork/app/src/terminal/writeable_pty/pty_controller.rs:539`

For raw writes into an active long-running command, reuse the
`WriteAgentInputToPty`-like path.

- `vendor/warp-fork/app/src/terminal/view.rs:7537`
- `vendor/warp-fork/app/src/terminal/writeable_pty/terminal_manager_util.rs:62`
- `vendor/warp-fork/app/src/terminal/writeable_pty/pty_controller.rs:617`

## 8. Policy and Approval Design

OMW's threat model requires no silent destructive actions.

Reference:

- `specs/threat-model.md:240`

pi-agent already has `beforeToolCall` and `afterToolCall` hooks.

References:

- `vendor/pi-mono/packages/agent/src/types.ts:103`
- `vendor/pi-mono/packages/agent/src/types.ts:209`
- `vendor/pi-mono/packages/agent/src/types.ts:223`

The loop invokes them around tool execution.

References:

- `vendor/pi-mono/packages/agent/src/agent-loop.ts:536`
- `vendor/pi-mono/packages/agent/src/agent-loop.ts:617`
- `vendor/pi-mono/packages/agent/src/agent-loop.ts:660`

Recommended OMW behavior:

```text
beforeToolCall
  -> classify tool call
  -> if read-only allowed: continue
  -> if write/exec/network requires approval: block and create approval request
  -> GUI/Web Controller shows approval prompt
  -> when approved: continue
  -> when rejected: return tool error

afterToolCall
  -> record decision/result
  -> append audit event through omw-server
  -> redact secrets
```

Even a `bash` command that looks read-only should be classified conservatively.
Good candidates for allow-by-default are commands like:

- `pwd`
- `date`
- `ls`
- `rg`
- `cat`
- `git diff`
- `git status`

Commands that should ask by default include:

- `rm`
- `mv`
- `chmod`
- `chown`
- `npm install`
- `pip install`
- `curl | sh`
- `git push`
- `docker run`
- commands with redirection,
- commands that modify the working tree or external state.

## 9. Audit Design

Every `#` session should create an audit trail:

```text
inline_agent_started
assistant_message_delta
tool_call_requested
approval_requested
approval_decided
terminal_command_started
terminal_command_snapshot
terminal_command_completed
file_write_requested
agent_completed
agent_cancelled
```

The PRD makes `omw-server` the single audit writer.

Reference:

- `PRD.md:422`

The threat model requires audit hash chains and redaction.

References:

- `specs/threat-model.md:232`
- `specs/threat-model.md:236`
- `specs/threat-model.md:237`

Therefore, `omw-agent` should not write audit files directly. It should POST
audit events to `omw-server`.

## 10. Shell Plugin Fallback With `#`

A Forge-like plugin is still useful for:

- users outside the OMW GUI,
- early prototyping before fork patches land,
- ssh sessions where OMW GUI input is not available,
- quick CLI workflows.

A minimal zsh fallback can copy the Forge model but use `#`.

### 10.1 ZLE Binding

```zsh
zle -N omw-accept-line
bindkey '^M' omw-accept-line
bindkey '^J' omw-accept-line
```

### 10.2 Parser

```zsh
function omw-accept-line() {
  local original_buffer="$BUFFER"

  if [[ "$BUFFER" =~ "^# (.*)$" ]]; then
    local prompt="${match[1]}"
    print -s -- "$original_buffer"
    CURSOR=${#BUFFER}
    zle redisplay
    omw agent --inline --prompt "$prompt" </dev/tty >/dev/tty
    BUFFER=""
    CURSOR=0
    zle reset-prompt
    return $?
  fi

  if [[ "$BUFFER" =~ "^##(.*)$" ]]; then
    BUFFER="#${match[1]}"
    zle accept-line
    return
  fi

  zle accept-line
}
```

### 10.3 What To Reuse From Forge

Reuse these ideas:

- Enter interception.
- Manual history preservation.
- `/dev/tty` redirection.
- Tab completion.
- syntax highlighting.
- `preexec` / `precmd` context ring if outside the GUI.
- OSC 133 markers if invoking from ZLE.

Forge references:

- `vendor/forge-code/shell-plugin/lib/bindings.zsh:42`
- `vendor/forge-code/shell-plugin/lib/dispatcher.zsh:119`
- `vendor/forge-code/shell-plugin/lib/helpers.zsh:82`
- `vendor/forge-code/shell-plugin/lib/context.zsh:69`
- `vendor/forge-code/shell-plugin/lib/context.zsh:81`

### 10.4 What Not To Reuse As Core

Do not make shell variables the primary OMW session state. The GUI/server should
own durable session identity, transcript IDs, block IDs, approvals, and audit.

## 11. Recommended Implementation Plan

### Phase 1: Native Parser Stub

Goal: `# hello` creates an inline block and returns a fake streamed response.

Work:

- Add `parse_hash_inline_agent_command`.
- Add input event/action.
- Add minimal inline agent block renderer.
- Do not call shell.
- Preserve command history as `# hello`.

Tests:

- `# hello` is intercepted.
- `## hello` escapes.
- `echo # hello` is not intercepted.
- multiline buffers are not intercepted.

### Phase 2: `omw-server` Agent Session API

Goal: GUI can start a real `omw-agent` session.

Work:

- Implement `POST /api/v1/agent/sessions`.
- Implement `WS /ws/v1/agent/:session_id`.
- Spawn or connect to `apps/omw-agent`.
- Stream events back to GUI.

References:

- `PRD.md:505`
- `PRD.md:506`

### Phase 3: GUI Session Registry

Goal: `omw-server` can broker commands into GUI terminal sessions.

Work:

- GUI registers terminal sessions with server.
- GUI streams terminal/block events to server.
- Server exposes internal session APIs.
- Add command RPC, not only raw input RPC.

References:

- `docs/omw-remote-implementation.md:55`
- `docs/omw-remote-implementation.md:60`

### Phase 4: `WarpSessionBashOperations`

Goal: pi-agent bash tool executes inside visible OMW terminal session.

Work:

- Add TypeScript adapter in `apps/omw-agent`.
- Use pi-agent `BashOperations`.
- Stream command output as `onData`.
- Resolve with exit code.
- Support abort signal and timeout.
- Return snapshots for long-running commands.

References:

- `vendor/pi-mono/packages/coding-agent/src/core/tools/bash.ts:49`
- `vendor/pi-mono/packages/coding-agent/src/core/tools/bash.ts:273`
- `vendor/pi-mono/packages/coding-agent/src/core/tools/bash.ts:347`

### Phase 5: Approval and Audit

Goal: no destructive command runs silently.

Work:

- Wire `beforeToolCall` to `omw-policy`.
- Add approval queue in server.
- Render approval cards in inline block.
- Log all events through `omw-server`.

References:

- `PRD.md:206`
- `PRD.md:415`
- `PRD.md:416`
- `specs/threat-model.md:240`

### Phase 6: Long-Running Control

Goal: agent can inspect and interact with running commands.

Work:

- Implement command snapshots.
- Add `read_shell_output`.
- Add `write_to_long_running_command`.
- Add transfer-control-to-user.
- Prevent user/agent simultaneous unsafe writes.

Use Warp's model as a guide:

- `vendor/warp-fork/app/src/ai/blocklist/action_model/execute/shell_command.rs:313`
- `vendor/warp-fork/app/src/ai/blocklist/action_model/execute/shell_command.rs:374`
- `vendor/warp-fork/app/src/terminal/view.rs:7600`

### Phase 7: zsh Fallback Plugin

Goal: optional `#` support outside native GUI input.

Work:

- Small plugin modeled after Forge.
- Invoke `omw agent --inline`.
- Use `/dev/tty`.
- Capture recent terminal context via hooks.

This should be a fallback, not the primary architecture.

## 12. Key Design Risks

### 12.1 Raw PTY Writes Are Not Enough

If `omw-agent` only posts bytes to `/sessions/:id/input`, it cannot reliably
know when a command ended or what exit code it had.

Mitigation:

- Add a structured command execution API that maps to GUI block lifecycle.

### 12.2 `#` Conflicts With Shell Comments

Mitigation:

- Native GUI intercept before shell.
- Strict grammar: only `# ` at start.
- `##` escape.
- No interception in multiline, heredoc, or continuation states.

### 12.3 CWD Drift

If the agent believes `cwd = A` but the visible shell is in `cwd = B`, commands
may run somewhere surprising.

Mitigation:

- GUI terminal session is the source of truth for cwd.
- Agent session context updates from GUI block/session metadata.
- In v1, reject command execution if requested cwd does not match active
  session cwd unless explicitly handled.

### 12.4 Command Side Effects

Running inside the real terminal means `cd`, exported variables, activated
venvs, and shell aliases may affect the user's session.

This is a feature, but it must be visible and auditable.

Mitigation:

- Show every command block.
- Default approval mode asks before side-effectful commands.
- Audit command and approval metadata.

### 12.5 Concurrent Agent/User Writes

If the user types while the agent controls a long-running command, output/input
can corrupt.

Mitigation:

- Mirror Warp's control state: user in control vs agent in control.
- Block user writes while agent owns a command, or require explicit takeover.
- Expose "Take control" and "Give control back" UI.

## 13. Recommended Final Shape

The best OMW `#` implementation is:

```text
# prompt
  -> native OMW GUI parser
  -> omw-server starts omw-agent session
  -> inline terminal agent block streams response
  -> pi-agent tools run through OMW adapters
  -> bash tool calls execute in active GUI terminal session
  -> approvals block dangerous tools
  -> output/snapshots flow back to agent
  -> audit logs every step
```

Do not implement the core as:

```text
# prompt
  -> shell expands to "omw agent -p prompt"
  -> hidden subprocess executes shell tools elsewhere
```

That would reproduce Forge's convenience, but it would miss OMW's central
product promise: local-first agent execution inside the user's actual terminal
session, with visible blocks, approvals, remote control, and audit.

Final recommendation:

- Use Forge as the UX and fallback-plugin reference.
- Use Warp as the execution architecture.
- Make `WarpSessionBashOperations` the central integration point between
  pi-agent and the visible terminal session.
