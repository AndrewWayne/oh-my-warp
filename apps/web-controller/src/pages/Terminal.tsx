import { useParams } from "react-router-dom";

export default function Terminal() {
  const { hostId, sessionId } = useParams();
  return (
    <section className="max-w-4xl mx-auto space-y-4">
      <h1 className="text-2xl font-semibold">Terminal</h1>
      <p className="text-neutral-400">Phase I will fill this in.</p>
      <p className="font-mono text-xs text-neutral-500">
        host: {hostId} · session: {sessionId}
      </p>
    </section>
  );
}
