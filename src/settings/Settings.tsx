import { useEffect, useId, useState } from "react";
import { getAutostart, setAutostart } from "../lib/ipc";
import { Switch } from "../ui/Switch";
import { AiSettings } from "./AiSettings";
import { errorText, useSettings } from "./useSettings";

/** One keyboard shortcut in a settings row: type it, then Save. Empty turns it off. */
function ShortcutField(props: {
  label: string;
  saved: string;
  describedBy: string;
  onSave: (keys: string) => void;
}) {
  const { label, saved, describedBy, onSave } = props;
  const id = useId();
  const [keys, setKeys] = useState(saved);

  // Show the saved shortcut whenever it changes (saved here, refused, or changed elsewhere).
  useEffect(() => setKeys(saved), [saved]);

  return (
    <form
      className="form-row"
      onSubmit={(e) => {
        e.preventDefault();
        onSave(keys.trim());
      }}
    >
      <div className="form-line">
        <label className="form-label" htmlFor={id}>
          {label}
        </label>
        <div className="form-control">
          <input
            id={id}
            type="text"
            value={keys}
            placeholder="Off"
            spellCheck={false}
            aria-describedby={describedBy}
            onChange={(e) => setKeys(e.target.value)}
          />
          <button type="submit" disabled={keys.trim() === saved}>
            Save
          </button>
        </div>
      </div>
    </form>
  );
}

/** The Settings tab of the home window. */
export function Settings() {
  const { settings, error, save } = useSettings();
  const ids = { history: useId(), startup: useId(), keysNote: useId() };
  const [autostart, setAutostartState] = useState<boolean | null>(null);
  const [autostartError, setAutostartError] = useState<string | null>(null);
  const [autostartBusy, setAutostartBusy] = useState(false);

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
    setAutostartBusy(true);
    setAutostartState(enabled);
    setAutostartError(null);
    try {
      await setAutostart(enabled);
    } catch (e) {
      setAutostartError(errorText(e));
      // Show what Windows really has, not what we hoped for.
      try {
        setAutostartState(await getAutostart());
      } catch {
        setAutostartState(before);
      }
    } finally {
      setAutostartBusy(false);
    }
  }

  if (!settings) {
    return (
      <div className="page">
        <p role="status">{error ?? "Loading…"}</p>
      </div>
    );
  }

  return (
    <div className="page">
      {error && <p role="alert" className="notice">{error}</p>}

      <h2 className="group-title">AI</h2>
      <AiSettings />

      <h2 className="group-title">Shortcuts</h2>
      <div className="group">
        <ShortcutField
          label="Open the chat from any app"
          saved={settings.chat_hotkey}
          describedBy={ids.keysNote}
          onSave={(keys) => void save({ chat_hotkey: keys })}
        />
        <ShortcutField
          label="Circle to look up"
          saved={settings.circle_hotkey}
          describedBy={ids.keysNote}
          onSave={(keys) => void save({ circle_hotkey: keys })}
        />
      </div>
      <p className="group-footer" id={ids.keysNote}>
        For example Ctrl+Alt+M or Ctrl+Alt+O. Leave a shortcut empty to turn it off.
      </p>

      <h2 className="group-title">Privacy</h2>
      <div className="group">
        <div className="form-row">
          <div className="form-line">
            <label className="form-label" htmlFor={ids.history}>
              Keep chat history
            </label>
            <Switch
              id={ids.history}
              checked={settings.history_enabled}
              onChange={(e) => void save({ history_enabled: e.target.checked })}
            />
          </div>
        </div>
      </div>
      <p className="group-footer">
        Screen captures are only sent to the AI you pick above. mouce cannot open, change, or delete
        your files.
      </p>

      <h2 className="group-title">Startup</h2>
      <div className="group">
        <div className="form-row">
          <div className="form-line">
            <label className="form-label" htmlFor={ids.startup}>
              Start mouce when Windows starts
            </label>
            <Switch
              id={ids.startup}
              checked={autostart === true}
              disabled={autostart === null || autostartBusy}
              onChange={(e) => void onAutostart(e.target.checked)}
            />
          </div>
          {autostartError && <p role="alert" className="error">{autostartError}</p>}
        </div>
      </div>
    </div>
  );
}
