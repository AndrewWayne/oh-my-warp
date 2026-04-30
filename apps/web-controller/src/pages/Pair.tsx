import { useParams } from "react-router-dom";

export default function Pair() {
  const { t } = useParams();
  return (
    <section className="max-w-2xl mx-auto space-y-4">
      <h1 className="text-2xl font-semibold">Pair</h1>
      <p className="text-neutral-400">Phase H will fill this in.</p>
      {t && (
        <p className="font-mono text-xs text-neutral-500">
          pairing token (truncated): {t.slice(0, 16)}…
        </p>
      )}
    </section>
  );
}
