import { useId } from "react";
import type { MouceSize } from "../lib/types";
import { useSettings } from "../settings/useSettings";

const SIZES: Array<{ id: MouceSize; label: string }> = [
  { id: "small", label: "Small" },
  { id: "medium", label: "Medium" },
  { id: "large", label: "Large" },
];

/** How mouce looks and moves on the desktop. */
export function Customize() {
  const { settings, error, save } = useSettings();
  const ids = { size: useId(), sway: useId() };

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
        <p role="alert" className="error">
          {error}
        </p>
      )}
      <section>
        <h2>On your desktop</h2>
        <div className="field">
          <label htmlFor={ids.size}>Size</label>
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
        <div className="check">
          <input
            id={ids.sway}
            type="checkbox"
            checked={settings.mouce_rotation}
            onChange={(e) => void save({ mouce_rotation: e.target.checked })}
          />
          <label htmlFor={ids.sway}>Sway</label>
        </div>
        <p className="muted">Drag mouce to move it. It remembers where you left it.</p>
      </section>
    </div>
  );
}
