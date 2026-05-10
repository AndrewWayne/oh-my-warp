import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { Link, useNavigate, useParams } from "react-router-dom";
import { Terminal as XTerm } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import { getPairing, type PairingRecord } from "../lib/storage/idb";
import { connectPty, type PtyConnection } from "../lib/pty-ws";
import { listSessions } from "../lib/sessions";
import TerminalShortcutStrip from "../components/TerminalShortcutStrip";
import { useVisualViewportSize } from "../hooks/useVisualViewportSize";
import {
  shouldSendTerminalResize,
  type TerminalGridSize,
} from "../lib/terminal-resize";
import { configureTerminalInputTraits } from "../lib/terminal-input-traits";

type Status = "loading" | "connecting" | "connected" | "disconnected" | "error";

export default function Terminal() {
  const { hostId, sessionId } = useParams();
  const navigate = useNavigate();
  const shellRef = useRef<HTMLElement | null>(null);
  const containerRef = useRef<HTMLDivElement | null>(null);
  const [status, setStatus] = useState<Status>("loading");
  const [errorMsg, setErrorMsg] = useState<string>("");
  const [retryNonce, setRetryNonce] = useState(0);
  const [debugLog, setDebugLog] = useState<string[]>([]);
  const debugLogRef = useRef<string[]>([]);
  const appendDebug = (msg: string) => {
    const stamped = `[${new Date().toISOString().slice(11, 19)}] ${msg}`;
    debugLogRef.current = [...debugLogRef.current, stamped].slice(-30);
    setDebugLog(debugLogRef.current);
  };

  // Refs used by the shortcut strip and fit scheduler. They live outside
  // the connect-effect so the helpers below can read them without
  // re-running the effect.
  const xtermRef = useRef<XTerm | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const connectionRef = useRef<PtyConnection | null>(null);
  const lastSizeRef = useRef<TerminalGridSize>({ rows: 0, cols: 0 });
  const rafIdRef = useRef<number | null>(null);
  const fitDebounceIdRef = useRef<number | null>(null);

  const viewport = useVisualViewportSize();
  const [shellTop, setShellTop] = useState(0);
  const layoutViewportHeight =
    typeof window !== "undefined" ? window.innerHeight : 0;
  const layoutViewportWidth =
    typeof window !== "undefined" ? window.innerWidth : 0;
  const compactInputMode =
    viewport.height > 0 &&
    layoutViewportHeight > 0 &&
    viewport.height < layoutViewportHeight * 0.72;
  const effectiveViewportHeight =
    compactInputMode && viewport.height > 0
      ? viewport.height
      : layoutViewportHeight || viewport.height;
  const terminalHeight =
    effectiveViewportHeight > 0
      ? Math.max(240, effectiveViewportHeight - shellTop)
      : undefined;
  const keyboardDock =
    compactInputMode && layoutViewportHeight > 0
      ? {
          offsetLeft: viewport.offsetLeft,
          offsetY: viewport.offsetTop + viewport.height - layoutViewportHeight,
          width: viewport.width || layoutViewportWidth,
        }
      : undefined;

  const measureShellTop = useCallback(() => {
    const next = Math.max(0, Math.round(shellRef.current?.getBoundingClientRect().top ?? 0));
    setShellTop((prev) => (prev === next ? prev : next));
  }, []);

  useLayoutEffect(() => {
    measureShellTop();
  }, [viewport.height, viewport.offsetTop, measureShellTop]);

  // Fit scheduler. Resize is fed by several event sources; wait for iOS
  // visualViewport animations to settle, then dedupe rows/cols before sending.
  //
  // event sources                       fit pass               outbound
  // ─────────────                       ────────               ────────
  // visualViewport resize/scroll  ┐
  // window resize/orientation     ├──▶ scheduleFit ──▶ fit() ──▶ if stable,
  // shortcut drawer open/close    │     (debounce+rAF)             useful rows/
  // daemon size frame             ┘                                cols changed,
  //                                                              sendControl
  const runFit = useCallback(() => {
    const fit = fitRef.current;
    const xterm = xtermRef.current;
    if (!fit || !xterm) return;
    const wasAtBottom = isScrolledToBottom(xterm);
    try {
      fit.fit();
    } catch {
      return;
    }
    const next = { rows: xterm.rows, cols: xterm.cols };
    const last = lastSizeRef.current;
    if (!shouldSendTerminalResize(next, last)) return;
    lastSizeRef.current = next;
    const conn = connectionRef.current;
    if (conn) {
      void conn.sendControl({ type: "resize", ...next }).catch(() => {
        /* swallow */
      });
    }
    if (wasAtBottom) xterm.scrollToBottom();
  }, []);

  const scheduleFit = useCallback(() => {
    if (rafIdRef.current != null) cancelAnimationFrame(rafIdRef.current);
    if (fitDebounceIdRef.current != null) {
      clearTimeout(fitDebounceIdRef.current);
      fitDebounceIdRef.current = null;
    }
    fitDebounceIdRef.current = window.setTimeout(() => {
      fitDebounceIdRef.current = null;
      rafIdRef.current = requestAnimationFrame(() => {
        rafIdRef.current = null;
        runFit();
      });
    }, 180);
  }, [runFit]);

  const sendTerminalBytes = useCallback((bytes: Uint8Array) => {
    const conn = connectionRef.current;
    if (!conn) return;
    void conn.sendInput(bytes).catch(() => {
      /* swallow; close handler will surface */
    });
    xtermRef.current?.focus();
    xtermRef.current?.scrollToBottom();
  }, []);

  useEffect(() => {
    scheduleFit();
  }, [terminalHeight, viewport.offsetTop, scheduleFit]);

  useEffect(() => {
    if (!hostId || !sessionId) return;
    let cancelled = false;
    let xterm: XTerm | null = null;
    let fit: FitAddon | null = null;
    let connection: PtyConnection | null = null;
    let onResize: (() => void) | null = null;

    (async () => {
      setStatus("loading");
      let pairing: PairingRecord | undefined;
      try {
        pairing = await getPairing(hostId);
      } catch (e) {
        if (cancelled) return;
        setErrorMsg(`Failed to load pairing: ${errStr(e)}`);
        setStatus("error");
        return;
      }
      if (!pairing) {
        if (!cancelled) navigate("/pair");
        return;
      }

      if (!containerRef.current || cancelled) return;
      xterm = new XTerm({
        cursorBlink: true,
        fontFamily:
          'ui-monospace, SFMono-Regular, "SF Mono", Menlo, Consolas, monospace',
        fontSize: 13,
        theme: { background: "#0a0a0a" },
      });
      fit = new FitAddon();
      xterm.loadAddon(fit);
      xterm.open(containerRef.current);
      configureTerminalInputTraits(containerRef.current);
      try {
        fit.fit();
      } catch {
        /* jsdom can't measure; ignore */
      }
      xtermRef.current = xterm;
      fitRef.current = fit;
      lastSizeRef.current = { rows: xterm.rows, cols: xterm.cols };

      setStatus("connecting");
      appendDebug("connectPty start");
      try {
        connection = await connectPty({
          pairing,
          sessionId,
          onDebug: appendDebug,
        });
      } catch (e) {
        appendDebug(`connectPty rejected: ${errStr(e)}`);
        if (cancelled) return;
        setErrorMsg(`Failed to connect: ${errStr(e)}`);
        setStatus("error");
        return;
      }
      appendDebug("connectPty resolved");
      if (cancelled) {
        connection.close();
        return;
      }
      connectionRef.current = connection;
      setStatus("connected");

      const enc = new TextEncoder();
      xterm.onData((data) => {
        sendTerminalBytes(enc.encode(data));
      });

      connection.onOutput((bytes) => {
        if (xterm) xterm.write(bytes);
      });

      // Daemon sends a `{type:"size", rows, cols}` Control frame on attach
      // with the laptop pane's actual size. Two cases:
      //
      // 1. Browser/wide client (fit-derived size >= laptop): match the
      //    laptop. xterm.resize(laptop_cols, laptop_rows). Bytes flow at
      //    laptop's coords and render correctly. NO upstream resize, so
      //    laptop user sees no change. (This is what fixed the desktop
      //    browser duplicate-render bug.)
      //
      // 2. Phone/narrow client (fit-derived size < laptop): the laptop's
      //    cursor positioning would clamp on phone's smaller grid and pile
      //    up. Instead, tell the laptop to shrink to phone's fit-size; the
      //    laptop's TUI re-flows for the narrow viewport via SIGWINCH and
      //    bytes flow at phone size — no clamping, readable text.
      if (typeof connection.onControl === "function") {
        connection.onControl((payload) => {
          if (
            !xterm ||
            typeof payload !== "object" ||
            payload === null ||
            (payload as { type?: unknown }).type !== "size"
          ) {
            return;
          }
          const p = payload as { rows?: number; cols?: number };
          const laptopRows = typeof p.rows === "number" ? p.rows : 0;
          const laptopCols = typeof p.cols === "number" ? p.cols : 0;
          if (laptopRows <= 0 || laptopCols <= 0) return;

          const phoneRows = xterm.rows;
          const phoneCols = xterm.cols;
          appendDebug(
            `size msg laptop=${laptopRows}x${laptopCols} phone=${phoneRows}x${phoneCols}`,
          );

          const TOO_NARROW = 80;
          if (phoneCols >= TOO_NARROW) {
            xterm.resize(laptopCols, laptopRows);
            lastSizeRef.current = { rows: laptopRows, cols: laptopCols };
          } else if (connection) {
            const next = { rows: phoneRows, cols: phoneCols };
            const last = lastSizeRef.current;
            if (!shouldSendTerminalResize(next, last)) {
              appendDebug(
                `skip unstable phone resize ${phoneRows}x${phoneCols}`,
              );
              return;
            }
            appendDebug(
              `request laptop shrink to ${phoneRows}x${phoneCols} (phone < 80 cols)`,
            );
            void connection
              .sendControl({ type: "resize", ...next })
              .catch((err) => appendDebug(`sendControl resize failed: ${errStr(err)}`));
            lastSizeRef.current = next;
          }
        });
      }

      connection.onClose((info) => {
        if (cancelled) return;
        setErrorMsg(`Connection closed (${info.code}${info.reason ? `: ${info.reason}` : ""})`);
        setStatus("disconnected");
        if (
          info.code === 1006 ||
          info.code === 1011 ||
          info.code === 4500
        ) {
          void listSessions(pairing!)
            .then((sessions) => {
              if (cancelled) return;
              const stillAlive = sessions.some(
                (s) => s.id === sessionId && s.alive,
              );
              if (!stillAlive && hostId) {
                navigate(`/host/${encodeURIComponent(hostId)}`, {
                  replace: true,
                });
              }
            })
            .catch(() => {
              /* leave user on disconnected screen with Retry */
            });
        }
      });

      onResize = () => {
        scheduleFit();
      };
      window.addEventListener("resize", onResize);
      window.addEventListener("orientationchange", onResize);
    })();

    return () => {
      cancelled = true;
      if (onResize) {
        window.removeEventListener("resize", onResize);
        window.removeEventListener("orientationchange", onResize);
      }
      if (rafIdRef.current != null) {
        cancelAnimationFrame(rafIdRef.current);
        rafIdRef.current = null;
      }
      if (fitDebounceIdRef.current != null) {
        clearTimeout(fitDebounceIdRef.current);
        fitDebounceIdRef.current = null;
      }
      if (connection) connection.close();
      if (xterm) xterm.dispose();
      xtermRef.current = null;
      fitRef.current = null;
      connectionRef.current = null;
      lastSizeRef.current = { rows: 0, cols: 0 };
    };
  }, [hostId, sessionId, navigate, retryNonce, scheduleFit, sendTerminalBytes]);

  return (
    <section
      ref={shellRef}
      data-testid="terminal-shell"
      className={`w-full max-w-5xl min-w-0 mx-auto flex flex-col overflow-hidden bg-neutral-950 sm:bg-transparent ${
        compactInputMode ? "gap-1" : "gap-2 sm:gap-3"
      }`}
      style={{
        height: terminalHeight ? `${terminalHeight}px` : undefined,
      }}
    >
      <div
        data-testid="terminal-mobile-toolbar"
        className="flex min-h-10 min-w-0 items-center justify-between gap-2 border-b border-neutral-900 bg-neutral-950 px-2 py-1 sm:min-h-0 sm:flex-col sm:items-stretch sm:justify-start sm:gap-2 sm:border-b-0 sm:bg-transparent sm:px-0 sm:py-0 md:flex-row md:items-center md:justify-between md:gap-4"
      >
        <div className="flex min-w-0 items-center gap-2 sm:gap-3">
          {hostId ? (
            <Link
              to={`/host/${encodeURIComponent(hostId)}`}
              data-testid="terminal-back-button"
              aria-label="Back to sessions"
              className="inline-flex h-8 w-8 items-center justify-center rounded border border-neutral-800 text-lg leading-none text-neutral-200 hover:bg-neutral-900 sm:h-auto sm:w-auto sm:gap-1 sm:px-2 sm:py-1 sm:text-xs"
            >
              <span aria-hidden="true">←</span>
              <span className="sr-only sm:not-sr-only">Sessions</span>
            </Link>
          ) : null}
          <h1 className="sr-only text-2xl font-semibold sm:not-sr-only">
            Terminal
          </h1>
        </div>
        <div className="flex min-w-0 flex-wrap items-center gap-3 text-xs sm:justify-end">
          <StatusBadge status={status} />
          <span
            data-testid="terminal-host-session-meta"
            className="hidden min-w-0 max-w-full break-all font-mono text-neutral-500 sm:inline sm:text-right"
          >
            host: {hostId} · session: {sessionId}
          </span>
        </div>
      </div>

      {status === "error" || status === "disconnected" ? (
        <div
          role="alert"
          className="rounded border border-red-700 bg-red-900/30 p-3 text-sm text-red-200 flex items-center justify-between gap-3"
        >
          <span>{errorMsg || "Disconnected."}</span>
          <button
            type="button"
            onClick={() => setRetryNonce((n) => n + 1)}
            className="px-3 py-1 rounded bg-red-700 hover:bg-red-600 text-xs font-semibold"
          >
            Retry
          </button>
        </div>
      ) : null}

      <div
        ref={containerRef}
        data-testid="xterm-container"
        className="flex-1 min-h-0 min-w-0 overflow-hidden border-y border-neutral-900 bg-black p-1 sm:rounded sm:border sm:border-neutral-800 sm:p-2"
      />

      <TerminalShortcutStrip
        enabled={status === "connected"}
        onSendBytes={sendTerminalBytes}
        onLayoutChange={scheduleFit}
        keyboardDock={keyboardDock}
      />

      {/* On-device debug log — primary purpose is to surface WebSocket
          lifecycle events on iOS Safari where DevTools isn't accessible. */}
      {debugLog.length > 0 && status !== "connected" ? (
        <details
          open
          className="rounded border border-neutral-800 bg-neutral-950 text-[11px] font-mono"
        >
          <summary className="px-2 py-1 cursor-pointer text-neutral-300">
            debug ({debugLog.length} events)
          </summary>
          <pre className="px-2 py-1 max-h-48 overflow-auto text-neutral-400 whitespace-pre-wrap break-all">
            {debugLog.join("\n")}
          </pre>
        </details>
      ) : null}
    </section>
  );
}

function StatusBadge({ status }: { status: Status }) {
  const label =
    status === "loading"
      ? "loading"
      : status === "connecting"
      ? "connecting"
      : status === "connected"
      ? "connected"
      : status === "disconnected"
      ? "disconnected"
      : "error";
  const cls =
    status === "connected"
      ? "bg-emerald-900/50 text-emerald-200 border-emerald-800"
      : status === "connecting" || status === "loading"
      ? "bg-neutral-800 text-neutral-200 border-neutral-700"
      : "bg-red-900/40 text-red-200 border-red-800";
  return (
    <span
      data-testid="conn-status"
      className={`px-2 py-0.5 rounded border text-[11px] uppercase tracking-wide ${cls}`}
    >
      {label}
    </span>
  );
}

function isScrolledToBottom(xterm: XTerm): boolean {
  const activeBuffer = xterm.buffer.active;
  return activeBuffer.viewportY >= activeBuffer.baseY;
}

function errStr(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}
