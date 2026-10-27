import { useEffect, useId, useState } from "react";
import { getAutostart, setAutostart } from "../lib/ipc";
import type { StarSize } from "../lib/types";
import { AiSettings } from "./AiSettings";
import { errorText, useSettings } from "./useSettings";

const SIZES: Array<{ id: StarSize; label: string }> = [
  { id: "small", label: "Small" },
  { id: "medium", label: "Medium" },
  { id: "large", label: "Large" },
];

export function Settings() {
  const { settings, error, save } = useSettings();
  const ids = { size: useId(), rotate: useId(), history: useId(), startup: useId() };
  const [autostart, setAutostartState] = useState<boolean | null>(null);
  const [autostartError, setAutostartError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    getAutostart()
      .then((on) => !cancelled && setAutostartState(on))
      .catch((e) => !cancelled && setAutostartError(errorText(e)));
    return () => {
      cancelled = true;
    };
  }, []);

  async function onAutostart(enabled: boolean) {
    const before = autostart;
    setAutostartState(enabled);
    setAutostartError(null);
    try {
      await setAutostart(enabled);
    } catch (e) {
      setAutostartState(before);
      setAutostartError(errorText(e));
    }
  }

  if (!settings) {
    return (
      <main className="page">
        <p role="status">{error ?? "Loading…"}</p>
      </main>
    );
  }

  return (
    <main className="page">
      <h1>Star settings</h1>
      {error && <p role="alert" className="error">{error}</p>}

      <section>
        <h2>AI</h2>
        <AiSettings />
      </section>

      <section>
        <h2>Star</h2>
        <div className="field">
          <label htmlFor={ids.size}>Size</label>
          <select
            id={ids.size}
            value={settings.star_size}
            onChange={(e) => void save({ star_size: e.target.value as StarSize })}
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
            id={ids.rotate}
            type="checkbox"
            checked={settings.star_rotation}
            onChange={(e) => void save({ star_rotation: e.target.checked })}
          />
          <label htmlFor={ids.rotate}>Rotate</label>
        </div>
      </section>

      <section>
        <h2>Shortcuts</h2>
        <p>Circle to look up: Ctrl+Alt+Space</p>
      </section>

      <section>
        <h2>Privacy</h2>
        <div className="check">
          <input
            id={ids.history}
            type="checkbox"
            checked={settings.history_enabled}
            onChange={(e) => void save({ history_enabled: e.target.checked })}
          />
          <label htmlFor={ids.history}>Keep chat history</label>
        </div>
        <p className="muted">
          Screen captures are only sent to the AI you pick above. Star cannot open, change, or delete
          your files.
        </p>
      </section>

      <section>
        <h2>Startup</h2>
        <div className="check">
          <input
            id={ids.startup}
            type="checkbox"
            checked={autostart === true}
            disabled={autostart === null}
            onChange={(e) => void onAutostart(e.target.checked)}
          />
          <label htmlFor={ids.startup}>Start Star when Windows starts</label>
        </div>
        {autostartError && <p role="alert" className="error">{autostartError}</p>}
      </section>
    </main>
  );
}
