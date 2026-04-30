import { Link } from "react-router-dom";
import { AppRoutes } from "./router";

export default function App() {
  return (
    <div className="min-h-screen flex flex-col">
      <header className="border-b border-neutral-800 px-4 py-3">
        <Link to="/" className="font-mono text-lg">
          omw
        </Link>
      </header>
      <main className="flex-1 p-4">
        <AppRoutes />
      </main>
    </div>
  );
}
