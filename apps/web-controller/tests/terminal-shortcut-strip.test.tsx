import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import TerminalShortcutStrip from "../src/components/TerminalShortcutStrip";
import { terminalControlBytes } from "../src/lib/terminal-control-bytes";

describe("TerminalShortcutStrip", () => {
  it("renders left/right arrows in the primary row without a redundant Enter key", () => {
    render(<TerminalShortcutStrip enabled onSendBytes={() => undefined} />);
    const primary = within(screen.getByTestId("terminal-shortcut-primary-row"));

    expect(primary.getByRole("button", { name: /shift.?tab/i })).toBeInTheDocument();
    expect(primary.getByRole("button", { name: /^esc$/i })).toBeInTheDocument();
    expect(primary.getByRole("button", { name: /^tab$/i })).toBeInTheDocument();
    expect(primary.getByRole("button", { name: /\^C/i })).toBeInTheDocument();
    expect(primary.getByRole("button", { name: /arrow up/i })).toBeInTheDocument();
    expect(primary.getByRole("button", { name: /arrow down/i })).toBeInTheDocument();
    expect(primary.getByRole("button", { name: /arrow left/i })).toBeInTheDocument();
    expect(primary.getByRole("button", { name: /arrow right/i })).toBeInTheDocument();
    expect(primary.getByRole("button", { name: /show extra shortcuts/i })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /^enter$/i })).toBeNull();
    expect(screen.queryByRole("button", { name: /hide keyboard/i })).toBeNull();
  });

  it("keeps the primary controls in one 44px-tall horizontal lane", () => {
    render(<TerminalShortcutStrip enabled onSendBytes={() => undefined} />);
    const surface = screen.getByTestId("terminal-shortcut-surface");
    expect(surface).toHaveClass("sticky", "bottom-0", "z-10");
    const primaryRow = screen.getByTestId("terminal-shortcut-primary-row");
    expect(primaryRow).toHaveClass("flex-nowrap");
    expect(primaryRow).toHaveClass("overflow-x-auto");
    expect(primaryRow).toHaveClass("justify-between");
    expect(primaryRow).toHaveClass("gap-px");
    const shortcutButtons = within(primaryRow).getAllByRole("button");
    for (const btn of shortcutButtons) {
      expect(btn).toHaveClass("w-[39px]");
      expect(btn).toHaveClass("h-11");
    }
  });

  it("keeps the compact row within a 375px phone width", () => {
    const surfacePaddingX = 8;
    const primaryButtons = 9;
    const primaryButtonWidth = 39;
    const primaryButtonGaps = 8;

    const minimumWidth =
      surfacePaddingX +
      primaryButtons * primaryButtonWidth +
      primaryButtonGaps;

    expect(minimumWidth).toBeLessThanOrEqual(375);
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

  it("sends primary shortcuts on pointerdown and suppresses the follow-up click", () => {
    const onSendBytes = vi.fn();
    render(<TerminalShortcutStrip enabled onSendBytes={onSendBytes} />);
    const btn = screen.getByRole("button", { name: /shift.?tab/i });

    fireEvent.pointerDown(btn, { pointerType: "touch" });
    expect(onSendBytes).toHaveBeenCalledTimes(1);
    expect(Array.from(onSendBytes.mock.calls[0][0])).toEqual(
      Array.from(terminalControlBytes("shift-tab")),
    );

    fireEvent.click(btn);
    expect(onSendBytes).toHaveBeenCalledTimes(1);
  });

  it("keeps keyboard-activated shortcut clicks working", () => {
    const onSendBytes = vi.fn();
    render(<TerminalShortcutStrip enabled onSendBytes={onSendBytes} />);

    fireEvent.click(screen.getByRole("button", { name: /^esc$/i }));
    expect(onSendBytes).toHaveBeenCalledTimes(1);
    expect(Array.from(onSendBytes.mock.calls[0][0])).toEqual(
      Array.from(terminalControlBytes("esc")),
    );
  });

  it("sends overflow shortcuts on pointerdown and suppresses the follow-up click", async () => {
    const user = userEvent.setup();
    const onSendBytes = vi.fn();
    render(<TerminalShortcutStrip enabled onSendBytes={onSendBytes} />);
    await user.click(screen.getByRole("button", { name: /show extra shortcuts/i }));
    const btn = screen.getByRole("button", { name: /\^D/i });

    fireEvent.pointerDown(btn, { pointerType: "touch" });
    expect(onSendBytes).toHaveBeenCalledTimes(1);
    expect(Array.from(onSendBytes.mock.calls[0][0])).toEqual(
      Array.from(terminalControlBytes("ctrl-d")),
    );

    fireEvent.click(btn);
    expect(onSendBytes).toHaveBeenCalledTimes(1);
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

  it("opens the overflow drawer on pointerdown without closing on the follow-up click", () => {
    render(<TerminalShortcutStrip enabled onSendBytes={() => undefined} />);
    const more = screen.getByRole("button", { name: /show extra shortcuts/i });

    fireEvent.pointerDown(more, { pointerType: "touch" });
    expect(screen.getByTestId("terminal-shortcut-overflow")).toBeInTheDocument();

    fireEvent.click(more);
    expect(screen.getByTestId("terminal-shortcut-overflow")).toBeInTheDocument();
  });

  it("toggles the overflow drawer when more is tapped without duplicating primary arrows", async () => {
    const user = userEvent.setup();
    render(<TerminalShortcutStrip enabled onSendBytes={() => undefined} />);
    const more = screen.getByRole("button", { name: /show extra shortcuts/i });

    await user.click(more);
    expect(screen.getByRole("button", { name: /hide keyboard/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /\^D/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /\^L/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /\//i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /\|/i })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /\?/i })).toBeInTheDocument();
    const overflow = within(screen.getByTestId("terminal-shortcut-overflow"));
    expect(overflow.queryByRole("button", { name: /arrow left/i })).toBeNull();
    expect(overflow.queryByRole("button", { name: /arrow right/i })).toBeNull();

    await user.click(screen.getByRole("button", { name: /hide extra shortcuts/i }));
    expect(screen.queryByRole("button", { name: /\^D/i })).toBeNull();
  });

  it("hide keyboard lives in the overflow drawer and calls the hide callback without sending bytes", async () => {
    const user = userEvent.setup();
    const onSendBytes = vi.fn();
    const onHideKeyboard = vi.fn();
    render(
      <TerminalShortcutStrip
        enabled
        onSendBytes={onSendBytes}
        onHideKeyboard={onHideKeyboard}
      />,
    );
    await user.click(screen.getByRole("button", { name: /show extra shortcuts/i }));
    const btn = screen.getByRole("button", { name: /hide keyboard/i });

    expect(btn).toHaveTextContent("hide");
    fireEvent.pointerDown(btn, { pointerType: "touch" });
    fireEvent.click(btn);
    expect(onHideKeyboard).toHaveBeenCalledTimes(1);
    expect(onSendBytes).not.toHaveBeenCalled();
  });

  it("does not double-send when two pointer activations resolve clicks out of order", () => {
    const onSendBytes = vi.fn();
    render(<TerminalShortcutStrip enabled onSendBytes={onSendBytes} />);
    const esc = screen.getByRole("button", { name: /^esc$/i });
    const tab = screen.getByRole("button", { name: /^tab$/i });

    fireEvent.pointerDown(esc, { pointerType: "touch" });
    fireEvent.pointerDown(tab, { pointerType: "touch" });
    expect(onSendBytes).toHaveBeenCalledTimes(2);

    fireEvent.click(esc);
    fireEvent.click(tab);
    expect(onSendBytes).toHaveBeenCalledTimes(2);
  });
});
