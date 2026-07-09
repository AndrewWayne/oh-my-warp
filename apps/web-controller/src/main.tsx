import React from "react";
import ReactDOM from "react-dom/client";
import { BrowserRouter } from "react-router-dom";
import { registerSW } from "virtual:pwa-register";
import App from "./App";
import "./index.css";

// Register the PWA service worker (offline app shell + Add-to-Home-Screen).
// `autoUpdate` pulls new client versions in the background; `immediate`
// activates a waiting worker so a resumed phone session gets the latest
// bundle without a manual reload. No-op where the SW API is unavailable.
registerSW({ immediate: true });

ReactDOM.createRoot(document.getElementById("root")!).render(
  <React.StrictMode>
    <BrowserRouter>
      <App />
    </BrowserRouter>
  </React.StrictMode>
);
