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
    expect(
      screen.getByRole("button", { name: /start scan/i })
    ).toBeInTheDocument();
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
  });
});
