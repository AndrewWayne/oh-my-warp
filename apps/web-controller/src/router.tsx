import { Routes, Route } from "react-router-dom";
import Home from "./pages/Home";
import Pair from "./pages/Pair";
import Terminal from "./pages/Terminal";

export function AppRoutes() {
  return (
    <Routes>
      <Route path="/" element={<Home />} />
      <Route path="/pair" element={<Pair />} />
      <Route path="/pair/:t" element={<Pair />} />
      <Route path="/terminal/:hostId/:sessionId" element={<Terminal />} />
    </Routes>
  );
}
