import { useEffect, useId, useState } from "react";
import type { MouceSize } from "../lib/types";
import { useSettings } from "../settings/useSettings";
import { Switch } from "../ui/Switch";

const SIZES: Array<{ id: MouceSize; label: string }> = [
  { id: "small", label: "Small" },
  { id: "medium", label: "Medium" },
  { id: "large", label: "Large" },
];

const LESS_TRANSPARENCY = "(prefers-reduced-transparency: reduce)";

/** Whether Windows has transparency effects turned off, which also turns off Liquid Glass. */
function useLessTransparency(): boolean {
  const [less, setLess] = useState(
    () => typeof window.matchMedia === "function" && window.matchMedia(LESS_TRANSPARENCY).matches,
  );
  useEffect(() => {
    if (typeof window.matchMedia !== "function") return;
    const query = window.matchMedia(LESS_TRANSPARENCY);
    const onChange = () => setLess(query.matches);
    query.addEventListener("change", onChange);
    return () => query.removeEventListener("change", onChange);
  }, []);
  return less;
}

/** How mouce looks and moves on the desktop, and how its windows look. */
export function Customize() {
  const { settings, error, save } = useSettings();
  const lessTransparency = useLessTransparency();
  const ids = { size: useId(), sway: useId(), glass: useId(), glassHint: useId(), glassNote: useId() };

  if (!settings) {
    return (
      <div className="page">
        <p role="status">{error ?? "Loading…"}</p>
      </div>
    );
  }

  return (
    <div className="page">
      {error && (
        <p role="alert" className="notice">
          {error}
        </p>
      )}

      <h2 className="group-title">On your desktop</h2>
      <div className="group">
        <div className="form-row">
          <div className="form-line">
            <label className="form-label" htmlFor={ids.size}>
              Size
            </label>
            <select
              id={ids.size}
              value={settings.mouce_size}
              onChange={(e) => void save({ mouce_size: e.target.value as MouceSize })}
            >
              {SIZES.map((s) => (
                <option key={s.id} value={s.id}>
                  {s.label}
                </option>
              ))}
            </select>
          </div>
        </div>
        <div className="form-row">
          <div className="form-line">
            <label className="form-label" htmlFor={ids.sway}>
              Sway
            </label>
            <Switch
              id={ids.sway}
              checked={settings.mouce_rotation}
              onChange={(e) => void save({ mouce_rotation: e.target.checked })}
            />
          </div>
        </div>
      </div>
      <p className="group-footer">Drag mouce to move it. It remembers where you left it.</p>

      <h2 className="group-title">Appearance</h2>
      <div className="group">
        <div className="form-row">
          <div className="form-line">
            <div className="form-label">
              <label htmlFor={ids.glass}>Liquid Glass</label>
              <small id={ids.glassHint}>Controls float on clear glass that bends the light behind it.</small>
            </div>
            <Switch
              id={ids.glass}
              checked={settings.liquid_glass && !lessTransparency}
              disabled={lessTransparency}
              aria-describedby={`${ids.glassHint} ${ids.glassNote}`}
              onChange={(e) => void save({ liquid_glass: e.target.checked })}
            />
          </div>
        </div>
      </div>
      <p className="group-footer" id={ids.glassNote}>
        {lessTransparency
          ? "Transparency effects are off in Windows settings, so mouce uses solid surfaces."
          : "Turn it off for solid surfaces everywhere."}
      </p>
    </div>
  );
}
