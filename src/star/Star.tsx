import { useEffect, useRef, useState } from "react";
import {
  getSettings,
  onSettingsChanged,
  onStarState,
  startDragging,
  starClicked,
} from "../lib/ipc";
import { starSizePx, type Settings, type StarState } from "../lib/types";

// The Mouce logo: a grey mouse head with a silver rim, drawn in a 100x100
// viewBox. Keep in step with src-tauri/icons/mouce.png (the app icon source).
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
        <linearGradient id="mouce-rim" gradientUnits="userSpaceOnUse" x1="10" y1="8" x2="90" y2="95">
          <stop offset="0%" stopColor="#FCFCFD" />
          <stop offset="100%" stopColor="#6E7278" />
        </linearGradient>
        <linearGradient id="mouce-face" gradientUnits="userSpaceOnUse" x1="30" y1="10" x2="60" y2="92">
          <stop offset="0%" stopColor="#D9DBDE" />
          <stop offset="100%" stopColor="#A8ABB0" />
        </linearGradient>
      </defs>
      <g fill="url(#mouce-rim)" stroke="url(#mouce-rim)" strokeWidth="4">
        <circle cx="24" cy="28" r="18" />
        <circle cx="76" cy="28" r="18" />
        <ellipse cx="50" cy="60" rx="36" ry="33" />
      </g>
      <g fill="url(#mouce-face)">
        <circle cx="24" cy="28" r="15" />
        <circle cx="76" cy="28" r="15" />
        <ellipse cx="50" cy="60" rx="33" ry="30" />
      </g>
      <g fill="#1F3566" stroke="#E6E8EB" strokeWidth="1.2">
        <circle cx="37" cy="58" r="5.5" />
        <circle cx="63" cy="58" r="5.5" />
      </g>
      <path d="M45.5 66 H54.5 L50 71.5 Z" fill="#141414" stroke="#141414" strokeWidth="2" strokeLinejoin="round" />
      <path d="M42 76 Q50 82.5 58 76" fill="none" stroke="#141414" strokeWidth="3" strokeLinecap="round" />
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
    // Attach a catch to each listen() promise right away so a failed
    // registration is never an unhandled rejection; cleanup still unlistens
    // whatever did register.
    const unlisteners: Array<Promise<(() => void) | null>> = [
      onStarState((s) => alive && setState(s)),
      onSettingsChanged((s) => alive && setSettings(s)),
    ].map((p) => p.catch(() => null));
    getSettings()
      .then((s) => alive && setSettings((cur) => cur ?? s))
      .catch(() => {});
    return () => {
      alive = false;
      unlisteners.forEach((p) => p.then((un) => un?.()).catch(() => {}));
    };
  }, []);

  const sizePx = starSizePx(settings?.star_size ?? "medium");
  const rotate = settings?.star_rotation ?? true;

  return (
    <div
      className="star-window"
      role="button"
      aria-label="Mouce — click to open settings"
      onContextMenu={(e) => e.preventDefault()}
      onPointerDown={(e) => {
        if (e.button !== 0) return;
        press.current = { x: e.clientX, y: e.clientY, dragging: false };
        try {
          e.currentTarget.setPointerCapture?.(e.pointerId);
        } catch {
          // Capture is best effort (missing in jsdom, or the pointer is gone).
        }
      }}
      onPointerMove={(e) => {
        const p = press.current;
        if (!p || p.dragging) return;
        if ((e.buttons & 1) === 0) {
          // The button was released somewhere we did not see.
          press.current = null;
          return;
        }
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
      onPointerCancel={() => {
        press.current = null;
      }}
      onLostPointerCapture={() => {
        press.current = null;
      }}
    >
      <Star state={state} rotate={rotate} sizePx={sizePx} />
    </div>
  );
}
