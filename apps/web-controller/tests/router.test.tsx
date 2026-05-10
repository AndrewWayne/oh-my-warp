import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import App from "../src/App";

describe("App routing", () => {
  it("renders Home at /", () => {
    render(
      <MemoryRouter initialEntries={["/"]}>
        <App />
      </MemoryRouter>
    );
    expect(screen.getByText("omw Web Controller")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: /pair a host/i })).toBeInTheDocument();
  });

  it("renders Pair at /pair", () => {
    render(
      <MemoryRouter initialEntries={["/pair"]}>
        <App />
      </MemoryRouter>
    );
    expect(screen.getByRole("heading", { name: "Pair" })).toBeInTheDocument();
    // jsdom doesn't expose navigator.mediaDevices and window.isSecureContext
    // is false, so Pair.tsx's cameraScanAvailable() returns false and the
    // page renders the "In-app QR scan unavailable" fallback instead of
    // the "Start scan" camera button. Assert the fallback heading + the
    // pairing-URL textarea, which together prove the page is fully wired
    // for the no-camera path (the primary phone flow is "OS camera scans
    // QR -> opens URL", so the in-app camera scan is a fallback anyway).
    expect(
      screen.getByRole("heading", { name: /in-app qr scan unavailable/i })
    ).toBeInTheDocument();
    expect(screen.getByLabelText(/Pairing URL/i)).toBeInTheDocument();
  });

  it("renders Terminal with route params", () => {
    render(
      <MemoryRouter initialEntries={["/terminal/host-42/sess-7"]}>
        <App />
      </MemoryRouter>
    );
    expect(
      screen.getByRole("heading", { name: "Terminal" })
    ).toBeInTheDocument();
    expect(screen.getByText(/host-42/)).toBeInTheDocument();
    expect(screen.getByText(/sess-7/)).toBeInTheDocument();
    expect(screen.getByTestId("app-root")).toHaveClass(
      "fixed",
      "inset-0",
      "overflow-hidden",
      "sm:static",
    );
    expect(screen.getByTestId("app-header")).toHaveClass("hidden", "sm:block");
    expect(screen.getByTestId("app-main")).toHaveClass(
      "overflow-hidden",
      "p-0",
      "sm:p-4",
    );
  });

  it("locks document scroll while the terminal route is mounted", () => {
    const { unmount } = render(
      <MemoryRouter initialEntries={["/terminal/host-42/sess-7"]}>
        <App />
      </MemoryRouter>
    );

    expect(document.documentElement.style.overflow).toBe("hidden");
    expect(document.documentElement.style.overscrollBehavior).toBe("none");
    expect(document.body.style.overflow).toBe("hidden");
    expect(document.body.style.overscrollBehavior).toBe("none");
    expect(document.body.style.position).toBe("fixed");
    expect(document.body.style.inset).toBe("0");
    expect(document.body.style.width).toBe("100%");
    expect(document.body.style.height).toBe("100%");

    unmount();

    expect(document.documentElement.style.overflow).toBe("");
    expect(document.documentElement.style.overscrollBehavior).toBe("");
    expect(document.body.style.overflow).toBe("");
    expect(document.body.style.overscrollBehavior).toBe("");
    expect(document.body.style.position).toBe("");
    expect(document.body.style.inset).toBe("");
    expect(document.body.style.width).toBe("");
    expect(document.body.style.height).toBe("");
  });
});
