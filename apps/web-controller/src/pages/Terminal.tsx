import { useEffect, useRef, useState } from "react";
import { Link, useNavigate, useParams } from "react-router-dom";
import { Terminal as XTerm } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import "@xterm/xterm/css/xterm.css";
import { getPairing, type PairingRecord } from "../lib/storage/idb";
import { connectPty, type PtyConnection } from "../lib/pty-ws";
import { listSessions } from "../lib/sessions";

type Status = "loading" | "connecting" | "connected" | "disconnected" | "error";

export default function Terminal() {
  const { hostId, sessionId } = useParams();
  const navigate = useNavigate();
  const containerRef = useRef<HTMLDivElement | null>(null);
  const [status, setStatus] = useState<Status>("loading");
  const [errorMsg, setErrorMsg] = useState<string>("");
  const [retryNonce, setRetryNonce] = useState(0);
  const [debugLog, setDebugLog] = useState<string[]>([]);
  // Stable ref for accumulating logs without triggering re-renders for every line.
  const debugLogRef = useRef<string[]>([]);
  const appendDebug = (msg: string) => {
    const stamped = `[${new Date().toISOString().slice(11, 19)}] ${msg}`;
    debugLogRef.current = [...debugLogRef.current, stamped].slice(-30);
    setDebugLog(debugLogRef.current);
  };

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
      try {
        fit.fit();
      } catch {
        /* jsdom can't measure; ignore */
      }

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
      setStatus("connected");

      const enc = new TextEncoder();
      xterm.onData((data) => {
        if (!connection) return;
        void connection.sendInput(enc.encode(data)).catch(() => {
          /* swallow; close handler will surface */
        });
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

        // Phone's natural size is whatever fit.fit() set after open(). If
        // the phone is at least as large as the laptop, match the laptop;
        // otherwise drive the laptop down to phone size.
        const phoneRows = xterm.rows;
        const phoneCols = xterm.cols;
        appendDebug(
          `size msg laptop=${laptopRows}x${laptopCols} phone=${phoneRows}x${phoneCols}`,
        );

        // Threshold: only trigger an upstream laptop-resize when the phone
        // is genuinely too narrow for a normal TUI to fit (< 80 cols, the
        // canonical terminal width). Above 80 cols we always match the
        // laptop's size — desktop browsers, even at narrower window
        // widths, end up here and the laptop user sees no change.
        // iPhone Safari at default viewport falls below 80 cols and gets
        // the laptop shrunk to its own size so claude code re-flows.
        const TOO_NARROW = 80;
        if (phoneCols >= TOO_NARROW) {
          // Wide enough — match laptop, no upstream resize. (Browser case.)
          xterm.resize(laptopCols, laptopRows);
        } else if (connection) {
          // iPhone-like — keep our own (smaller) size and ask the laptop
          // to shrink. Laptop's TUI re-flows for the narrow viewport via
          // SIGWINCH; new bytes will arrive at phone size.
          appendDebug(`request laptop shrink to ${phoneRows}x${phoneCols} (phone < 80 cols)`);
          void connection
            .sendControl({ type: "resize", rows: phoneRows, cols: phoneCols })
            .catch((err) => appendDebug(`sendControl resize failed: ${errStr(err)}`));
        }
      });

      connection.onClose((info) => {
        if (cancelled) return;
        setErrorMsg(`Connection closed (${info.code}${info.reason ? `: ${info.reason}` : ""})`);
        setStatus("disconnected");
        // v0.4-thin Stage C: when the close looks like the session went away
        // (1006 abnormal-close — host-side pump aborted; or 1011 server
        // error), check whether the session still exists. If not, the user
        // is stranded on a dead Terminal page whose Retry button would 404 —
        // navigate them back to the Sessions list instead.
        if (
          info.code === 1006 ||
          info.code === 1011 ||
          // 4500 is the daemon's own "pty_io" close code from
          // `crates/omw-remote/src/ws/pty.rs`; same idea: session is gone.
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
              // listSessions failure (e.g. host unreachable) — leave the
              // user on the disconnected screen with the Retry option.
            });
        }
      });

      onResize = () => {
        if (!fit || !connection || !xterm) return;
        try {
          fit.fit();
        } catch {
          return;
        }
        const { cols, rows } = xterm;
        void connection
          .sendControl({ type: "resize", cols, rows })
          .catch(() => {
            /* swallow */
          });
      };
      window.addEventListener("resize", onResize);
    })();

    return () => {
      cancelled = true;
      if (onResize) window.removeEventListener("resize", onResize);
      if (connection) connection.close();
      if (xterm) xterm.dispose();
    };
  }, [hostId, sessionId, navigate, retryNonce]);

  return (
    <section className="max-w-5xl mx-auto space-y-3">
      <div className="flex items-center justify-between gap-4">
        <div className="flex items-center gap-3">
          {hostId ? (
            <Link
              to={`/host/${encodeURIComponent(hostId)}`}
              data-testid="terminal-back-button"
              aria-label="Back to sessions"
              className="inline-flex items-center gap-1 px-2 py-1 rounded border border-neutral-700 text-xs text-neutral-200 hover:bg-neutral-800"
            >
              ← Sessions
            </Link>
          ) : null}
          <h1 className="text-2xl font-semibold">Terminal</h1>
        </div>
        <div className="flex items-center gap-3 text-xs">
          <StatusBadge status={status} />
          <span className="font-mono text-neutral-500">
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
        className="h-[70vh] rounded border border-neutral-800 bg-black p-2"
      />

      {/* On-device debug log — primary purpose is to surface WebSocket
          lifecycle events on iOS Safari where DevTools isn't accessible.
          Renders the most recent ~30 events from connectPty / WS / signature
          verification / etc. Hidden once the connection is healthy and an
          output frame has flowed (status === "connected" and the log shows
          a "connectPty resolved" line). For now we render unconditionally so
          we can diagnose stuck-connecting cases. */}
      {debugLog.length > 0 ? (
        <details
          open={status !== "connected"}
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

function errStr(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}
