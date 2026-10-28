import { useEffect, useRef, useState } from "react";
import { circleCancel, circleFinish, circleFrame, circleReady } from "../lib/ipc";
import { errorText } from "../settings/useSettings";
import { pathData, selectionFrom, type Point } from "./selection";

export const HINT = "Circle anything to ask mouce about it · Esc to cancel";
export const TOO_SMALL = "Draw around something a bit bigger";

/**
 * The full-screen layer for "Circle to look up": shows the frozen screen and lets the
 * user draw around the part they want to ask about.
 */
export function CircleLayer() {
  const [frame, setFrame] = useState<string | null>(null);
  const [points, setPoints] = useState<Point[]>([]);
  const [hint, setHint] = useState(HINT);
  const drawing = useRef(false);
  const done = useRef(false);

  useEffect(() => {
    let alive = true;
    circleFrame()
      .then((url) => alive && setFrame(url))
      .catch(() => void circleCancel().catch(() => {}));
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") void circleCancel().catch(() => {});
    };
    window.addEventListener("keydown", onKey);
    return () => {
      alive = false;
      window.removeEventListener("keydown", onKey);
    };
  }, []);

  async function finish(path: Point[]) {
    const selection = selectionFrom(path, window.innerWidth, window.innerHeight);
    if (!selection) {
      setHint(TOO_SMALL);
      setPoints([]);
      return;
    }
    done.current = true;
    try {
      await circleFinish(selection);
    } catch (e) {
      done.current = false;
      setHint(errorText(e));
      setPoints([]);
    }
  }

  return (
    <div
      className="circle-layer"
      onPointerDown={(e) => {
        if (done.current) return;
        if (e.button === 2) {
          void circleCancel().catch(() => {});
          return;
        }
        if (e.button !== 0) return;
        drawing.current = true;
        setHint(HINT);
        setPoints([{ x: e.clientX, y: e.clientY }]);
        try {
          e.currentTarget.setPointerCapture?.(e.pointerId);
        } catch {
          // Capture is best effort (missing in jsdom, or the pointer is gone).
        }
      }}
      onPointerMove={(e) => {
        if (!drawing.current) return;
        setPoints((p) => [...p, { x: e.clientX, y: e.clientY }]);
      }}
      onPointerUp={(e) => {
        if (!drawing.current) return;
        drawing.current = false;
        void finish([...points, { x: e.clientX, y: e.clientY }]);
      }}
    >
      {frame && (
        <img
          className="circle-frame"
          src={frame}
          alt=""
          draggable={false}
          onLoad={() => void circleReady().catch(() => {})}
        />
      )}
      <div className="circle-dim" />
      <svg className="circle-ink" aria-hidden="true">
        {points.length > 1 && <path d={pathData(points)} />}
      </svg>
      <p className="circle-hint" role="status">
        {hint}
      </p>
    </div>
  );
}
