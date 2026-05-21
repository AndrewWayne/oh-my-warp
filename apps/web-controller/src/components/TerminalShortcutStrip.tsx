import {
  useEffect,
  useRef,
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

type PointerClickToken = TerminalControlKey | "more" | "hide";

const PRIMARY: KeyDef[] = [
  { id: "shift-tab", label: "⇧⇥", ariaLabel: "shift tab" },
  { id: "esc", label: "esc", ariaLabel: "esc" },
  { id: "tab", label: "tab", ariaLabel: "tab" },
  { id: "ctrl-c", label: "^C", ariaLabel: "^C" },
  { id: "arrow-up", label: "↑", ariaLabel: "arrow up" },
  { id: "arrow-down", label: "↓", ariaLabel: "arrow down" },
  { id: "arrow-left", label: "←", ariaLabel: "arrow left" },
  { id: "arrow-right", label: "→", ariaLabel: "arrow right" },
];

const OVERFLOW: KeyDef[] = [
  { id: "ctrl-d", label: "^D", ariaLabel: "^D" },
  { id: "ctrl-l", label: "^L", ariaLabel: "^L" },
  { id: "slash", label: "/", ariaLabel: "/" },
  { id: "pipe", label: "|", ariaLabel: "|" },
  { id: "question", label: "?", ariaLabel: "?" },
];

interface Props {
  enabled: boolean;
  onSendBytes: (bytes: Uint8Array) => void;
  onHideKeyboard?: () => void;
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
  onHideKeyboard,
  onLayoutChange,
  keyboardDock,
}: Props) {
  const [moreOpen, setMoreOpen] = useState(false);
  const pendingPointerClicksRef = useRef<Map<PointerClickToken, number>>(new Map());
  const pendingPointerClicksTimerRef = useRef<number | null>(null);

  useEffect(() => {
    onLayoutChange?.();
  }, [moreOpen, onLayoutChange]);

  useEffect(() => {
    return () => {
      if (pendingPointerClicksTimerRef.current != null) {
        window.clearTimeout(pendingPointerClicksTimerRef.current);
      }
    };
  }, []);

  // pointerdown on a button would otherwise steal focus from xterm's hidden
  // textarea. preventDefault keeps focus where it was.
  const preserveFocus = (e: PointerEvent<HTMLButtonElement>) => {
    e.preventDefault();
  };

  const tap = (id: TerminalControlKey) => {
    onSendBytes(terminalControlBytes(id));
  };

  const toggleMore = () => {
    setMoreOpen((v) => !v);
  };

  const rememberPointerActivation = (token: PointerClickToken) => {
    const pending = pendingPointerClicksRef.current;
    pending.set(token, (pending.get(token) ?? 0) + 1);
    if (pendingPointerClicksTimerRef.current != null) {
      window.clearTimeout(pendingPointerClicksTimerRef.current);
    }
    pendingPointerClicksTimerRef.current = window.setTimeout(() => {
      pending.clear();
      pendingPointerClicksTimerRef.current = null;
    }, 1000);
  };

  const consumePointerActivation = (token: PointerClickToken) => {
    const pending = pendingPointerClicksRef.current;
    const count = pending.get(token) ?? 0;
    if (count <= 0) return false;
    if (count === 1) {
      pending.delete(token);
    } else {
      pending.set(token, count - 1);
    }
    if (pending.size === 0 && pendingPointerClicksTimerRef.current != null) {
      window.clearTimeout(pendingPointerClicksTimerRef.current);
      pendingPointerClicksTimerRef.current = null;
    }
    return true;
  };

  const sendOnPointerDown =
    (id: TerminalControlKey) => (e: PointerEvent<HTMLButtonElement>) => {
      preserveFocus(e);
      if (!enabled) return;
      tap(id);
      rememberPointerActivation(id);
    };

  const sendOnClick = (id: TerminalControlKey) => () => {
    if (!enabled) return;
    if (consumePointerActivation(id)) return;
    tap(id);
  };

  const moreOnPointerDown = (e: PointerEvent<HTMLButtonElement>) => {
    preserveFocus(e);
    if (!enabled) return;
    toggleMore();
    rememberPointerActivation("more");
  };

  const moreOnClick = () => {
    if (!enabled) return;
    if (consumePointerActivation("more")) return;
    toggleMore();
  };

  const hideOnPointerDown = (e: PointerEvent<HTMLButtonElement>) => {
    preserveFocus(e);
    if (!enabled) return;
    onHideKeyboard?.();
    rememberPointerActivation("hide");
  };

  const hideOnClick = () => {
    if (!enabled) return;
    if (consumePointerActivation("hide")) return;
    onHideKeyboard?.();
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
  const primaryBtnClass =
    "h-11 w-[39px] shrink-0 rounded-md border border-white/10 bg-white/[0.07] px-0.5 text-neutral-100 text-[12px] font-mono whitespace-nowrap touch-manipulation transition-colors active:bg-white/[0.14] focus-visible:outline focus-visible:outline-2 focus-visible:outline-emerald-400 disabled:opacity-40 disabled:cursor-not-allowed cursor-pointer";
  const overflowBtnClass =
    "h-11 w-11 shrink-0 rounded-md border border-white/10 bg-white/[0.07] px-2 text-neutral-100 text-[13px] font-mono whitespace-nowrap touch-manipulation transition-colors active:bg-white/[0.14] focus-visible:outline focus-visible:outline-2 focus-visible:outline-emerald-400 disabled:opacity-40 disabled:cursor-not-allowed cursor-pointer";
  const hideBtnClass =
    "h-11 w-16 shrink-0 rounded-md border border-white/10 bg-white/[0.07] px-2 text-neutral-100 text-[12px] font-mono whitespace-nowrap touch-manipulation transition-colors active:bg-white/[0.14] focus-visible:outline focus-visible:outline-2 focus-visible:outline-emerald-400 disabled:opacity-40 disabled:cursor-not-allowed cursor-pointer";
  const rowClass =
    "flex min-w-0 flex-nowrap items-center justify-between gap-px overflow-x-auto overscroll-x-contain";
  const topRowClass = "flex min-w-0 items-center";
  const surfaceClass = docked
    ? "fixed bottom-0 left-0 z-50 flex flex-col gap-1 border-y border-neutral-700/70 bg-[#1b1c1f] px-1 py-1 shadow-[0_-1px_0_rgba(255,255,255,0.06)]"
    : "sticky bottom-0 z-10 flex shrink-0 flex-col gap-1 border-y border-neutral-900 bg-[#1b1c1f] px-1 py-1 shadow-[0_-1px_0_rgba(255,255,255,0.05)] sm:static sm:border-y-0 sm:bg-transparent sm:px-0 sm:py-0 sm:shadow-none";

  const content = (
    <div
      data-testid="terminal-shortcut-surface"
      className={surfaceClass}
      style={dockStyle}
    >
      <div className={topRowClass}>
        <div
          data-testid="terminal-shortcut-primary-row"
          className={`${rowClass} flex-1`}
        >
          {PRIMARY.map((k) => (
            <button
              key={k.id}
              type="button"
              aria-label={k.ariaLabel}
              disabled={!enabled}
              onPointerDown={sendOnPointerDown(k.id)}
              onClick={sendOnClick(k.id)}
              className={primaryBtnClass}
            >
              {k.label}
            </button>
          ))}
          <button
            type="button"
            aria-label={moreOpen ? "hide extra shortcuts" : "show extra shortcuts"}
            aria-expanded={moreOpen}
            disabled={!enabled}
            onPointerDown={moreOnPointerDown}
            onClick={moreOnClick}
            className={primaryBtnClass}
          >
            ⋯
          </button>
        </div>
      </div>
      {moreOpen ? (
        <div
          data-testid="terminal-shortcut-overflow"
          className={rowClass}
        >
          <button
            type="button"
            aria-label="hide keyboard"
            title="Hide keyboard"
            disabled={!enabled}
            onPointerDown={hideOnPointerDown}
            onClick={hideOnClick}
            className={hideBtnClass}
          >
            hide
          </button>
          {OVERFLOW.map((k) => (
            <button
              key={k.id}
              type="button"
              aria-label={k.ariaLabel}
              disabled={!enabled}
              onPointerDown={sendOnPointerDown(k.id)}
              onClick={sendOnClick(k.id)}
              className={overflowBtnClass}
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
