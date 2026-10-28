import { useEffect, useId, useState } from "react";
import { getAutostart, setAutostart } from "../lib/ipc";
import { AiSettings } from "./AiSettings";
import { errorText, useSettings } from "./useSettings";

/** The Settings tab of the home window. */
export function Settings() {
  const { settings, error, save } = useSettings();
  const ids = { history: useId(), startup: useId(), chatKey: useId() };
  const [chatKey, setChatKey] = useState("");
  const [autostart, setAutostartState] = useState<boolean | null>(null);
  const [autostartError, setAutostartError] = useState<string | null>(null);
  const [autostartBusy, setAutostartBusy] = useState(false);

  // Show the saved shortcut whenever it changes (saved here, refused, or changed elsewhere).
  const savedChatKey = settings?.chat_hotkey;
  useEffect(() => {
    if (savedChatKey !== undefined) setChatKey(savedChatKey);
  }, [savedChatKey]);

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
      {error && <p role="alert" className="error">{error}</p>}

      <section>
        <h2>AI</h2>
        <AiSettings />
      </section>

      <section>
        <h2>Shortcuts</h2>
        <form
          className="field"
          onSubmit={(e) => {
            e.preventDefault();
            void save({ chat_hotkey: chatKey.trim() });
          }}
        >
          <label htmlFor={ids.chatKey}>Open the chat from any app</label>
          <div className="row">
            <input
              id={ids.chatKey}
              type="text"
              value={chatKey}
              placeholder="Off"
              spellCheck={false}
              onChange={(e) => setChatKey(e.target.value)}
            />
            <button type="submit" disabled={chatKey.trim() === settings.chat_hotkey}>
              Save
            </button>
          </div>
          <p className="muted">For example Ctrl+Alt+M. Leave it empty to turn the shortcut off.</p>
        </form>
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
          Screen captures are only sent to the AI you pick above. mouce cannot open, change, or delete
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
            disabled={autostart === null || autostartBusy}
            onChange={(e) => void onAutostart(e.target.checked)}
          />
          <label htmlFor={ids.startup}>Start mouce when Windows starts</label>
        </div>
        {autostartError && <p role="alert" className="error">{autostartError}</p>}
      </section>
    </div>
  );
}
