import { useEffect, useState } from "react";

export interface VisualViewportSize {
  height: number;
  width: number;
  offsetLeft: number;
  offsetTop: number;
}

function read(): VisualViewportSize {
  const vv = (typeof window !== "undefined"
    ? (window as Window & { visualViewport?: VisualViewport })
        .visualViewport
    : undefined) as VisualViewport | undefined;
  if (vv) {
    return {
      height: vv.height,
      width: vv.width,
      offsetLeft: vv.offsetLeft,
      offsetTop: vv.offsetTop,
    };
  }
  return {
    height: typeof window !== "undefined" ? window.innerHeight : 0,
    width: typeof window !== "undefined" ? window.innerWidth : 0,
    offsetLeft: 0,
    offsetTop: 0,
  };
}

export function useVisualViewportSize(): VisualViewportSize {
  const [size, setSize] = useState<VisualViewportSize>(() => read());

  useEffect(() => {
    const vv = (window as Window & { visualViewport?: VisualViewport })
      .visualViewport;
    const update = () => setSize(read());
    if (vv) {
      vv.addEventListener("resize", update);
      vv.addEventListener("scroll", update);
      return () => {
        vv.removeEventListener("resize", update);
        vv.removeEventListener("scroll", update);
      };
    }
    window.addEventListener("resize", update);
    return () => {
      window.removeEventListener("resize", update);
    };
  }, []);

  return size;
}
