import { useCallback, useEffect, useRef, useState } from "react";
import { Link, useNavigate, useParams } from "react-router-dom";
import {
  deletePairing,
  getPairing,
  type PairingRecord,
} from "../lib/storage/idb";
import {
  deleteSession,
  listSessions,
  type SessionMeta,
} from "../lib/sessions";

const REFRESH_INTERVAL_MS = 5000;

type LoadState =
  | { kind: "loading" }
  | { kind: "ready"; sessions: SessionMeta[] }
  | { kind: "error"; message: string };

export default function Sessions() {
  const { hostId } = useParams();
  const navigate = useNavigate();

  const [pairing, setPairing] = useState<PairingRecord | null>(null);
  const [load, setLoad] = useState<LoadState>({ kind: "loading" });
  const [stopping, setStopping] = useState<string | null>(null);
  const [stopError, setStopError] = useState<string | null>(null);

  // Avoid stale-set after unmount.
  const aliveRef = useRef(true);
  useEffect(() => {
    aliveRef.current = true;
    return () => {
      aliveRef.current = false;
    };
  }, []);

  const refresh = useCallback(async (p: PairingRecord) => {
    try {
      const sessions = await listSessions(p);
      sessions.sort((a, b) => (a.createdAt < b.createdAt ? 1 : -1));
      if (aliveRef.current) setLoad({ kind: "ready", sessions });
    } catch (e) {
      if (aliveRef.current) {
        setLoad({
          kind: "error",
          message: e instanceof Error ? e.message : String(e),
        });
      }
    }
  }, []);

  // Load pairing on mount; redirect if missing.
  useEffect(() => {
    if (!hostId) return;
    let cancelled = false;
    (async () => {
      try {
        const p = await getPairing(hostId);
        if (cancelled) return;
        if (!p) {
          navigate("/pair");
          return;
        }
        setPairing(p);
        void refresh(p);
      } catch (e) {
        if (cancelled) return;
        setLoad({
          kind: "error",
          message: `Failed to load pairing: ${
            e instanceof Error ? e.message : String(e)
          }`,
        });
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [hostId, navigate, refresh]);

  // Auto-refresh while mounted + pairing loaded.
  useEffect(() => {
    if (!pairing) return;
    const handle = setInterval(() => {
      void refresh(pairing);
    }, REFRESH_INTERVAL_MS);
    return () => clearInterval(handle);
  }, [pairing, refresh]);

  function handleOpen(s: SessionMeta) {
    if (!hostId) return;
    navigate(
      `/terminal/${encodeURIComponent(hostId)}/${encodeURIComponent(s.id)}`,
    );
  }

  async function handleStop(s: SessionMeta) {
    if (!pairing) return;
    setStopError(null);
    setStopping(s.id);
    try {
      await deleteSession(pairing, s.id);
      await refresh(pairing);
    } catch (e) {
      setStopError(
        `Couldn't stop session: ${e instanceof Error ? e.message : String(e)}`,
      );
    } finally {
      if (aliveRef.current) setStopping(null);
    }
  }

  async function handleForgetHost() {
    if (!hostId) return;
    if (
      typeof window !== "undefined" &&
      !window.confirm(
        "Forget this host? You'll need to pair again to reconnect.",
      )
    ) {
      return;
    }
    try {
      await deletePairing(hostId);
    } catch (e) {
      setStopError(
        `Couldn't forget host: ${e instanceof Error ? e.message : String(e)}`,
      );
      return;
    }
    navigate("/");
  }

  return (
    <section className="max-w-3xl mx-auto space-y-4">
      <div className="flex items-center justify-between gap-3">
        <div className="min-w-0">
          <h1 className="text-2xl font-semibold truncate">
            {pairing?.hostId ?? hostId ?? "Sessions"}
          </h1>
          {pairing && (
            <div className="text-xs text-neutral-500 truncate">
              {pairing.hostUrl} · paired{" "}
              {new Date(pairing.pairedAt).toLocaleString()}
            </div>
          )}
        </div>
        <div className="flex items-center gap-2 shrink-0">
          <Link
            to="/"
            className="px-3 py-1.5 rounded bg-neutral-800 hover:bg-neutral-700 text-sm"
          >
            Hosts
          </Link>
          <button
            type="button"
            onClick={() => pairing && void refresh(pairing)}
            disabled={!pairing}
            className="px-3 py-1.5 rounded bg-neutral-800 hover:bg-neutral-700 disabled:bg-neutral-900 disabled:text-neutral-600 text-sm"
          >
            Refresh
          </button>
        </div>
      </div>

      {load.kind === "loading" && (
        <p className="text-sm text-neutral-500">Loading sessions…</p>
      )}

      {load.kind === "error" && (
        <div
          role="alert"
          className="rounded border border-red-700 bg-red-900/30 p-3 text-sm text-red-200"
        >
          Failed to load sessions: {load.message}
        </div>
      )}

      {stopError && (
        <div
          role="alert"
          className="rounded border border-red-700 bg-red-900/30 p-3 text-sm text-red-200"
        >
          {stopError}
        </div>
      )}

      {load.kind === "ready" && load.sessions.length === 0 && (
        <div className="rounded border border-neutral-800 bg-neutral-900/40 p-4 text-sm text-neutral-400">
          No active sessions on this host. Open a pane in Warp on the desktop to
          share it here.
        </div>
      )}

      {load.kind === "ready" && load.sessions.length > 0 && (
        <ul className="divide-y divide-neutral-800 rounded border border-neutral-800">
          {load.sessions.map((s) => (
            <li
              key={s.id}
              className="flex items-center justify-between gap-3 p-3"
            >
              <div className="min-w-0 flex-1">
                <div className="font-mono text-sm truncate">{s.name}</div>
                <div className="text-xs text-neutral-500 truncate">
                  <span className="font-mono">{s.id.slice(0, 8)}</span>
                  {" · "}created {new Date(s.createdAt).toLocaleString()}
                  {!s.alive && (
                    <span className="ml-2 text-red-400">(stopped)</span>
                  )}
                </div>
              </div>
              <div className="flex items-center gap-2 shrink-0">
                <button
                  type="button"
                  onClick={() => handleOpen(s)}
                  disabled={!s.alive}
                  className="px-3 py-1.5 rounded bg-blue-600 hover:bg-blue-500 disabled:bg-neutral-800 disabled:text-neutral-500 text-sm font-semibold"
                >
                  Open
                </button>
                <button
                  type="button"
                  onClick={() => void handleStop(s)}
                  disabled={stopping !== null}
                  title="Stop sharing — keeps the pane open on the laptop"
                  className="px-3 py-1.5 rounded bg-neutral-800 hover:bg-neutral-700 disabled:bg-neutral-900 disabled:text-neutral-600 text-sm"
                >
                  {stopping === s.id ? "Stopping…" : "Stop"}
                </button>
              </div>
            </li>
          ))}
        </ul>
      )}

      <div className="flex items-center justify-between pt-2">
        <Link
          to="/pair"
          className="text-sm text-neutral-400 hover:text-neutral-200"
        >
          Pair another host
        </Link>
        <button
          type="button"
          onClick={() => void handleForgetHost()}
          className="text-sm text-red-300 hover:text-red-200"
        >
          Forget this host
        </button>
      </div>
    </section>
  );
}
