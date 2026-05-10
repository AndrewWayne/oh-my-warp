import { describe, expect, it, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import TerminalShortcutStrip from "../src/components/TerminalShortcutStrip";
import { terminalControlBytes } from "../src/lib/terminal-control-bytes";

describe("TerminalShortcutStrip", () => {
  it("renders all primary keys", () => {
    render(<TerminalShortcutStrip enabled onSendBytes={() => undefined} />);
    expect(screen.getByRole("button", { name: /shift.?tab/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^esc$/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^tab$/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /\^C/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /arrow up/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /arrow down/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /^enter$/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /more/i })).toBeInTheDocument();
  });

  it("keeps the primary controls in one 44px-tall horizontal lane", () => {
    render(<TerminalShortcutStrip enabled onSendBytes={() => undefined} />);
    const surface = screen.getByTestId("terminal-shortcut-surface");
    expect(surface).toHaveClass("sticky", "bottom-0", "z-10");
    const primaryRow = screen.getByTestId("terminal-shortcut-primary-row");
    expect(primaryRow).toHaveClass("flex-nowrap");
    expect(primaryRow).toHaveClass("overflow-x-auto");
    expect(primaryRow).toHaveClass("justify-between");
    for (const btn of screen.getAllByRole("button")) {
      expect(btn).toHaveClass("w-11");
      expect(btn).toHaveClass("h-11");
    }
  });

  it("can dock the surface to the visual viewport while preserving layout space", () => {
    render(
      <TerminalShortcutStrip
        enabled
        onSendBytes={() => undefined}
        keyboardDock={{ offsetLeft: 0, offsetY: -304, width: 390 }}
      />,
    );

    expect(screen.getByTestId("terminal-shortcut-strip")).toHaveStyle({
      height: "54px",
    });
    const surface = screen.getByTestId("terminal-shortcut-surface");
    expect(surface).toHaveClass("fixed", "bottom-0", "left-0", "z-50");
    expect(surface).toHaveStyle({
      transform: "translate3d(0px, -304px, 0)",
      width: "390px",
    });
  });

  it("calls onSendBytes with the exact bytes for the tapped primary key", async () => {
    const user = userEvent.setup();
    const onSendBytes = vi.fn();
    render(<TerminalShortcutStrip enabled onSendBytes={onSendBytes} />);

    await user.click(screen.getByRole("button", { name: /shift.?tab/i }));
    expect(onSendBytes).toHaveBeenCalledTimes(1);
    expect(Array.from(onSendBytes.mock.calls[0][0])).toEqual(
      Array.from(terminalControlBytes("shift-tab")),
    );

    await user.click(screen.getByRole("button", { name: /^esc$/i }));
    expect(Array.from(onSendBytes.mock.calls[1][0])).toEqual(
      Array.from(terminalControlBytes("esc")),
    );

    await user.click(screen.getByRole("button", { name: /\^C/i }));
    expect(Array.from(onSendBytes.mock.calls[2][0])).toEqual(
      Array.from(terminalControlBytes("ctrl-c")),
    );
  });

  it("disables every button when enabled is false", () => {
    render(<TerminalShortcutStrip enabled={false} onSendBytes={() => undefined} />);
    const buttons = screen.getAllByRole("button");
    expect(buttons.length).toBeGreaterThan(0);
    for (const b of buttons) {
      expect(b).toBeDisabled();
    }
  });

  it("does not call onSendBytes when buttons are disabled", async () => {
    const user = userEvent.setup();
    const onSendBytes = vi.fn();
    render(<TerminalShortcutStrip enabled={false} onSendBytes={onSendBytes} />);
    await user.click(screen.getByRole("button", { name: /^esc$/i }));
    expect(onSendBytes).not.toHaveBeenCalled();
  });

  it("calls preventDefault on pointerdown so xterm keeps focus", () => {
    const onSendBytes = vi.fn();
    render(<TerminalShortcutStrip enabled onSendBytes={onSendBytes} />);
    const btn = screen.getByRole("button", { name: /^esc$/i });

    const ev = new Event("pointerdown", { bubbles: true, cancelable: true });
    btn.dispatchEvent(ev);
    expect(ev.defaultPrevented).toBe(true);
  });

  it("more drawer is closed by default and the overflow keys are not in the DOM", () => {
    render(<TerminalShortcutStrip enabled onSendBytes={() => undefined} />);
    expect(screen.queryByRole("button", { name: /\^D/i })).toBeNull();
    expect(screen.queryByRole("button", { name: /\^L/i })).toBeNull();
  });

  it("toggles the overflow drawer when more is tapped", async () => {
    const user = userEvent.setup();
    render(<TerminalShortcutStrip enabled onSendBytes={() => undefined} />);
    const more = screen.getByRole("button", { name: /more/i });

    await user.click(more);
    expect(screen.getByRole("button", { name: /\^D/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /\^L/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /\//i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /\|/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /\?/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /arrow left/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /arrow right/i })).toBeInTheDocument();

    await user.click(more);
    expect(screen.queryByRole("button", { name: /\^D/i })).toBeNull();
  });

  it("overflow key tap sends bytes and keeps the drawer open", async () => {
    const user = userEvent.setup();
    const onSendBytes = vi.fn();
    render(<TerminalShortcutStrip enabled onSendBytes={onSendBytes} />);
    await user.click(screen.getByRole("button", { name: /more/i }));

    await user.click(screen.getByRole("button", { name: /\^D/i }));
    expect(Array.from(onSendBytes.mock.calls[0][0])).toEqual(
      Array.from(terminalControlBytes("ctrl-d")),
    );
    // Drawer remains open so the user can chain another control.
    expect(screen.getByRole("button", { name: /\^L/i })).toBeInTheDocument();
  });
});
