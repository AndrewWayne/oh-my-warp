import { useEffect, useState } from "react";
import { Link, useNavigate } from "react-router-dom";
import { listPairings, type PairingRecord } from "../lib/storage/idb";

export default function Home() {
  const navigate = useNavigate();
  const [pairings, setPairings] = useState<PairingRecord[] | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const list = await listPairings();
        if (cancelled) return;
        setPairings(list);
      } catch (e) {
        if (cancelled) return;
        setLoadError(e instanceof Error ? e.message : String(e));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  function handleOpen(p: PairingRecord) {
    navigate(`/host/${encodeURIComponent(p.hostId)}`);
  }

  return (
    <section className="max-w-2xl mx-auto space-y-4">
      <h1 className="text-2xl font-semibold">omw Web Controller</h1>
      <p className="text-neutral-400">
        BYORC client for omw — paired terminal and agent control over Tailscale.
      </p>

      {pairings === null && !loadError && (
        <p className="text-sm text-neutral-500">Loading paired hosts…</p>
      )}

      {loadError && (
        <div
          role="alert"
          className="rounded border border-red-700 bg-red-900/30 p-3 text-sm text-red-200"
        >
          Failed to load pairings: {loadError}
        </div>
      )}

      {pairings && pairings.length > 0 && (
        <div className="space-y-2">
          <h2 className="text-sm font-semibold uppercase tracking-wide text-neutral-300">
            Paired hosts
          </h2>
          <ul className="divide-y divide-neutral-800 rounded border border-neutral-800">
            {pairings.map((p) => (
              <li
                key={p.hostId}
                className="flex items-center justify-between gap-3 p-3"
              >
                <div className="min-w-0">
                  <div className="font-mono text-sm truncate">{p.hostId}</div>
                  <div className="text-xs text-neutral-500 truncate">
                    {p.hostUrl} · paired{" "}
                    {new Date(p.pairedAt).toLocaleString()}
                  </div>
                </div>
                <button
                  type="button"
                  onClick={() => handleOpen(p)}
                  className="px-3 py-1.5 rounded bg-blue-600 hover:bg-blue-500 text-sm font-semibold whitespace-nowrap"
                >
                  Open
                </button>
              </li>
            ))}
          </ul>
        </div>
      )}

      <div>
        <Link
          to="/pair"
          className="inline-block px-4 py-2 rounded bg-neutral-800 hover:bg-neutral-700"
        >
          {pairings && pairings.length > 0 ? "Pair another host" : "Pair a host"}
        </Link>
      </div>
    </section>
  );
}
