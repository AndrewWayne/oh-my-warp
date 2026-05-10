import {
  useEffect,
  useState,
  type CSSProperties,
  type PointerEvent,
} from "react";
import {
  terminalControlBytes,
  type TerminalControlKey,
} from "../lib/terminal-control-bytes";

interface KeyDef {
  id: TerminalControlKey;
  label: string;
  ariaLabel: string;
}

const PRIMARY: KeyDef[] = [
  { id: "shift-tab", label: "⇧⇥", ariaLabel: "shift tab" },
  { id: "esc", label: "esc", ariaLabel: "esc" },
  { id: "tab", label: "tab", ariaLabel: "tab" },
  { id: "ctrl-c", label: "^C", ariaLabel: "^C" },
  { id: "arrow-up", label: "↑", ariaLabel: "arrow up" },
  { id: "arrow-down", label: "↓", ariaLabel: "arrow down" },
  { id: "enter", label: "↵", ariaLabel: "enter" },
];

const OVERFLOW: KeyDef[] = [
  { id: "ctrl-d", label: "^D", ariaLabel: "^D" },
  { id: "ctrl-l", label: "^L", ariaLabel: "^L" },
  { id: "slash", label: "/", ariaLabel: "/" },
  { id: "pipe", label: "|", ariaLabel: "|" },
  { id: "question", label: "?", ariaLabel: "?" },
  { id: "arrow-left", label: "←", ariaLabel: "arrow left" },
  { id: "arrow-right", label: "→", ariaLabel: "arrow right" },
];

interface Props {
  enabled: boolean;
  onSendBytes: (bytes: Uint8Array) => void;
  /** Fired when the drawer opens or closes so the parent can refit xterm. */
  onLayoutChange?: () => void;
  keyboardDock?: {
    offsetLeft: number;
    offsetY: number;
    width: number;
  };
}

export default function TerminalShortcutStrip({
  enabled,
  onSendBytes,
  onLayoutChange,
  keyboardDock,
}: Props) {
  const [moreOpen, setMoreOpen] = useState(false);

  useEffect(() => {
    onLayoutChange?.();
  }, [moreOpen, onLayoutChange]);

  // pointerdown on a button would otherwise steal focus from xterm's hidden
  // textarea, dismissing the iOS native keyboard. preventDefault keeps focus
  // where it was; click still fires after.
  const preserveFocus = (e: PointerEvent<HTMLButtonElement>) => {
    e.preventDefault();
  };

  const tap = (id: TerminalControlKey) => {
    onSendBytes(terminalControlBytes(id));
  };

  const docked = keyboardDock !== undefined;
  const placeholderHeight = moreOpen ? 102 : 54;
  const dockStyle: CSSProperties | undefined = docked
    ? {
        transform: `translate3d(${Math.round(keyboardDock.offsetLeft)}px, ${Math.round(
          keyboardDock.offsetY,
        )}px, 0)`,
        width: `${Math.round(keyboardDock.width)}px`,
      }
    : undefined;
  const btnClass =
    "h-11 w-11 shrink-0 rounded-md border border-white/10 bg-white/[0.07] px-2 text-neutral-100 text-[13px] font-mono whitespace-nowrap touch-manipulation transition-colors active:bg-white/[0.14] focus-visible:outline focus-visible:outline-2 focus-visible:outline-emerald-400 disabled:opacity-40 disabled:cursor-not-allowed cursor-pointer";
  const rowClass =
    "flex flex-nowrap items-center justify-between gap-1 overflow-x-auto overscroll-x-contain";
  const surfaceClass = docked
    ? "fixed bottom-0 left-0 z-50 flex flex-col gap-1 border-y border-neutral-700/70 bg-[#1b1c1f] px-1.5 py-1 shadow-[0_-1px_0_rgba(255,255,255,0.06)]"
    : "sticky bottom-0 z-10 flex shrink-0 flex-col gap-1 border-y border-neutral-900 bg-[#1b1c1f] px-1.5 py-1 shadow-[0_-1px_0_rgba(255,255,255,0.05)] sm:static sm:border-y-0 sm:bg-transparent sm:px-0 sm:py-0 sm:shadow-none";

  const content = (
    <div
      data-testid="terminal-shortcut-surface"
      className={surfaceClass}
      style={dockStyle}
    >
      <div
        data-testid="terminal-shortcut-primary-row"
        className={rowClass}
      >
        {PRIMARY.map((k) => (
          <button
            key={k.id}
            type="button"
            aria-label={k.ariaLabel}
            disabled={!enabled}
            onPointerDown={preserveFocus}
            onClick={() => tap(k.id)}
            className={btnClass}
          >
            {k.label}
          </button>
        ))}
        <button
          type="button"
          aria-label="more"
          aria-expanded={moreOpen}
          disabled={!enabled}
          onPointerDown={preserveFocus}
          onClick={() => setMoreOpen((v) => !v)}
          className={btnClass}
        >
          ...
        </button>
      </div>
      {moreOpen ? (
        <div
          data-testid="terminal-shortcut-overflow"
          className={rowClass}
        >
          {OVERFLOW.map((k) => (
            <button
              key={k.id}
              type="button"
              aria-label={k.ariaLabel}
              disabled={!enabled}
              onPointerDown={preserveFocus}
              onClick={() => tap(k.id)}
              className={btnClass}
            >
              {k.label}
            </button>
          ))}
        </div>
      ) : null}
    </div>
  );

  if (docked) {
    return (
      <div
        data-testid="terminal-shortcut-strip"
        className="shrink-0"
        style={{ height: placeholderHeight }}
      >
        {content}
      </div>
    );
  }

  return (
    <div
      data-testid="terminal-shortcut-strip"
      className="shrink-0"
    >
      {content}
    </div>
  );
}
