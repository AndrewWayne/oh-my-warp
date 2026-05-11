interface KeyboardDockEdgeInput {
  layoutViewportHeight: number;
  visualViewportHeight: number;
  visualViewportOffsetTop: number;
  previousDockEdge: number;
}

export function computeKeyboardDockEdge({
  layoutViewportHeight,
  visualViewportHeight,
  visualViewportOffsetTop,
  previousDockEdge,
}: KeyboardDockEdgeInput): number {
  if (layoutViewportHeight <= 0 || visualViewportHeight <= 0) return 0;

  const rawEdge = clamp(
    visualViewportOffsetTop + visualViewportHeight,
    0,
    layoutViewportHeight,
  );
  const minSaneEdge = Math.min(
    layoutViewportHeight,
    Math.max(240, layoutViewportHeight * 0.45),
  );

  if (rawEdge >= minSaneEdge) return rawEdge;
  if (previousDockEdge >= minSaneEdge) return previousDockEdge;
  return minSaneEdge;
}

function clamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, value));
}
