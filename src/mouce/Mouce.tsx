import { useEffect, useRef, useState } from "react";
import {
  alertsCurrent,
  getSettings,
  onAlerts,
  onSettingsChanged,
  onMouceState,
  startDragging,
  mouceClicked,
} from "../lib/ipc";
import { mouceSizePx, type Settings, type MouceState } from "../lib/types";
import logoUrl from "../assets/mouce.png";

// The mouce logo, cut out of images/mouce logo.jpg with a transparent
// background (src-tauri/icons/mouce.png is the same cutout at 512px).
export function Mouce(props: { state: MouceState; rotate: boolean; sizePx: number }) {
  const { state, rotate, sizePx } = props;
  return (
    <img
      className={"mouce mouce--" + state + (rotate ? "" : " mouce--still")}
      src={logoUrl}
      width={sizePx}
      height={sizePx}
      alt=""
      draggable={false}
    />
  );
}

const DRAG_THRESHOLD_PX = 3;

export function MouceWindow() {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [state, setState] = useState<MouceState>("idle");
  // A reminder is waiting on the card beside the mouce.
  const [reminding, setReminding] = useState(false);
  const press = useRef<{ x: number; y: number; dragging: boolean } | null>(null);

  useEffect(() => {
    let alive = true;
    // Attach a catch to each listen() promise right away so a failed
    // registration is never an unhandled rejection; cleanup still unlistens
    // whatever did register.
    const unlisteners: Array<Promise<(() => void) | null>> = [
      onMouceState((s) => alive && setState(s)),
      onSettingsChanged((s) => alive && setSettings(s)),
      onAlerts((due) => alive && setReminding(due.length > 0)),
    ].map((p) => p.catch(() => null));
    alertsCurrent()
      .then((due) => alive && setReminding(due.length > 0))
      .catch(() => {});
    getSettings()
      .then((s) => alive && setSettings((cur) => cur ?? s))
      .catch(() => {});
    return () => {
      alive = false;
      unlisteners.forEach((p) => p.then((un) => un?.()).catch(() => {}));
    };
  }, []);

  const sizePx = mouceSizePx(settings?.mouce_size ?? "medium");
  const rotate = settings?.mouce_rotation ?? true;

  return (
    <div
      className="mouce-window"
      role="button"
      aria-label={reminding ? "mouce — a reminder is waiting" : "mouce — click to chat"}
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
        if (p && !p.dragging) mouceClicked().catch(() => {});
      }}
      onPointerCancel={() => {
        press.current = null;
      }}
      onLostPointerCapture={() => {
        press.current = null;
      }}
    >
      <Mouce state={state === "idle" && reminding ? "reminder" : state} rotate={rotate} sizePx={sizePx} />
    </div>
  );
}
