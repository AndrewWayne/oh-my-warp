import { useEffect } from "react";
import { Link, useLocation } from "react-router-dom";
import { AppRoutes } from "./router";

export default function App() {
  const location = useLocation();
  const isTerminalRoute = location.pathname.startsWith("/terminal/");

  useEffect(() => {
    if (!isTerminalRoute) return;

    const html = document.documentElement;
    const body = document.body;
    const previous = {
      htmlOverflow: html.style.overflow,
      htmlOverscrollBehavior: html.style.overscrollBehavior,
      bodyOverflow: body.style.overflow,
      bodyOverscrollBehavior: body.style.overscrollBehavior,
      bodyPosition: body.style.position,
      bodyInset: body.style.inset,
      bodyWidth: body.style.width,
      bodyHeight: body.style.height,
    };

    html.style.overflow = "hidden";
    html.style.overscrollBehavior = "none";
    body.style.overflow = "hidden";
    body.style.overscrollBehavior = "none";
    body.style.position = "fixed";
    body.style.inset = "0";
    body.style.width = "100%";
    body.style.height = "100%";

    return () => {
      html.style.overflow = previous.htmlOverflow;
      html.style.overscrollBehavior = previous.htmlOverscrollBehavior;
      body.style.overflow = previous.bodyOverflow;
      body.style.overscrollBehavior = previous.bodyOverscrollBehavior;
      body.style.position = previous.bodyPosition;
      body.style.inset = previous.bodyInset;
      body.style.width = previous.bodyWidth;
      body.style.height = previous.bodyHeight;
    };
  }, [isTerminalRoute]);

  return (
    <div
      data-testid="app-root"
      className={
        isTerminalRoute
          ? "fixed inset-0 flex flex-col overflow-hidden bg-neutral-950 sm:static sm:min-h-screen sm:bg-transparent"
          : "min-h-screen flex flex-col"
      }
    >
      <header
        data-testid="app-header"
        className={`border-b border-neutral-800 px-4 py-3 ${
          isTerminalRoute ? "hidden sm:block" : ""
        }`}
      >
        <Link to="/" className="font-mono text-lg">
          omw
        </Link>
      </header>
      <main
        data-testid="app-main"
        className={`flex-1 min-h-0 ${
          isTerminalRoute ? "overflow-hidden p-0 sm:p-4" : "p-4"
        }`}
      >
        <AppRoutes />
      </main>
    </div>
  );
}
