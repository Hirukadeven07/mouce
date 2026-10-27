import { useEffect, useRef, useState } from "react";
import {
  getSettings,
  onSettingsChanged,
  onStarState,
  startDragging,
  starClicked,
} from "../lib/ipc";
import { starSizePx, type Settings, type StarState } from "../lib/types";

// Outer and inner radii of a regular 5-point star in a 100x100 viewBox,
// centred at (50, 50) with one point straight up.
function starPoints(): string {
  const outer = 46;
  const inner = 19;
  const pts: string[] = [];
  for (let i = 0; i < 10; i++) {
    const r = i % 2 === 0 ? outer : inner;
    const a = (Math.PI / 5) * i - Math.PI / 2;
    pts.push(`${(50 + r * Math.cos(a)).toFixed(2)},${(50 + r * Math.sin(a)).toFixed(2)}`);
  }
  return pts.join(" ");
}

const POINTS = starPoints();

export function Star(props: { state: StarState; rotate: boolean; sizePx: number }) {
  const { state, rotate, sizePx } = props;
  return (
    <svg
      className={"star star--" + state + (rotate ? "" : " star--still")}
      width={sizePx}
      height={sizePx}
      viewBox="0 0 100 100"
      aria-hidden="true"
      focusable="false"
    >
      <defs>
        <radialGradient id="star-fill" cx="50%" cy="50%" r="55%">
          <stop offset="0%" stopColor="#FFE9A3" />
          <stop offset="100%" stopColor="#F5C542" />
        </radialGradient>
      </defs>
      <polygon
        points={POINTS}
        fill="url(#star-fill)"
        stroke="#F5C542"
        strokeWidth="4"
        strokeLinejoin="round"
      />
    </svg>
  );
}

const DRAG_THRESHOLD_PX = 3;

export function StarWindow() {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [state, setState] = useState<StarState>("idle");
  const press = useRef<{ x: number; y: number; dragging: boolean } | null>(null);

  useEffect(() => {
    let alive = true;
    const unlisteners: Array<Promise<() => void>> = [
      onStarState((s) => alive && setState(s)),
      onSettingsChanged((s) => alive && setSettings(s)),
    ];
    getSettings()
      .then((s) => alive && setSettings((cur) => cur ?? s))
      .catch(() => {});
    return () => {
      alive = false;
      unlisteners.forEach((p) => p.then((un) => un()).catch(() => {}));
    };
  }, []);

  const sizePx = starSizePx(settings?.star_size ?? "medium");
  const rotate = settings?.star_rotation ?? true;

  return (
    <div
      className="star-window"
      onPointerDown={(e) => {
        if (e.button !== 0) return;
        press.current = { x: e.clientX, y: e.clientY, dragging: false };
      }}
      onPointerMove={(e) => {
        const p = press.current;
        if (!p || p.dragging) return;
        if (Math.hypot(e.clientX - p.x, e.clientY - p.y) > DRAG_THRESHOLD_PX) {
          p.dragging = true;
          startDragging().catch(() => {});
        }
      }}
      onPointerUp={() => {
        const p = press.current;
        press.current = null;
        if (p && !p.dragging) starClicked().catch(() => {});
      }}
    >
      <Star state={state} rotate={rotate} sizePx={sizePx} />
    </div>
  );
}
