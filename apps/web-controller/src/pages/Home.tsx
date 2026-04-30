import { Link } from "react-router-dom";

export default function Home() {
  return (
    <section className="max-w-2xl mx-auto space-y-4">
      <h1 className="text-2xl font-semibold">omw Web Controller</h1>
      <p className="text-neutral-400">
        BYORC client for omw — paired terminal and agent control over Tailscale.
      </p>
      <Link
        to="/pair"
        className="inline-block px-4 py-2 rounded bg-neutral-800 hover:bg-neutral-700"
      >
        Pair a host
      </Link>
    </section>
  );
}
