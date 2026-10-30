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
  const ids = {
    history: useId(),
    startup: useId(),
    keysNote: useId(),
    dictionary: useId(),
    dictionaryNote: useId(),
    plans: useId(),
    calendarNote: useId(),
    quiet: useId(),
    quietFrom: useId(),
    quietTo: useId(),
    fullScreen: useId(),
  };
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

      <h2 className="group-title">Dictionary</h2>
      <div className="group">
        <div className="form-row">
          <div className="form-line">
            <label className="form-label" htmlFor={ids.dictionary}>
              Look up a word with Ctrl + right-click
            </label>
            <Switch
              id={ids.dictionary}
              checked={settings.dictionary_enabled}
              aria-describedby={ids.dictionaryNote}
              onChange={(e) => void save({ dictionary_enabled: e.target.checked })}
            />
          </div>
        </div>
      </div>
      <p className="group-footer" id={ids.dictionaryNote}>
        Hold Ctrl and right-click a word in any app to see what it means, or select a phrase and
        Ctrl + right-click inside it. Meanings come from a
        dictionary built into mouce, which works offline. With an AI set up, the word and its
        sentence also go to it, to explain the word as it is used there. Password boxes are never
        read.
      </p>

      <h2 className="group-title">Calendar</h2>
      <div className="group">
        <div className="form-row">
          <div className="form-line">
            <label className="form-label" htmlFor={ids.plans}>
              Answer questions about my plans in the chat
            </label>
            <Switch
              id={ids.plans}
              checked={settings.calendar_in_chat}
              aria-describedby={ids.calendarNote}
              onChange={(e) => void save({ calendar_in_chat: e.target.checked })}
            />
          </div>
        </div>
        <div className="form-row">
          <div className="form-line">
            <label className="form-label" htmlFor={ids.quiet}>
              Quiet hours
              <small>Reminders wait until quiet hours end.</small>
            </label>
            <Switch
              id={ids.quiet}
              checked={settings.quiet_hours}
              onChange={(e) => void save({ quiet_hours: e.target.checked })}
            />
          </div>
          {settings.quiet_hours && (
            <div className="form-line quiet-times">
              <label htmlFor={ids.quietFrom}>From</label>
              <input
                id={ids.quietFrom}
                type="time"
                value={settings.quiet_from}
                onChange={(e) => e.target.value && void save({ quiet_from: e.target.value })}
              />
              <label htmlFor={ids.quietTo}>to</label>
              <input
                id={ids.quietTo}
                type="time"
                value={settings.quiet_to}
                onChange={(e) => e.target.value && void save({ quiet_to: e.target.value })}
              />
            </div>
          )}
        </div>
        <div className="form-row">
          <div className="form-line">
            <label className="form-label" htmlFor={ids.fullScreen}>
              Wait while an app is full screen
              <small>A film, a presentation or a game.</small>
            </label>
            <Switch
              id={ids.fullScreen}
              checked={settings.wait_for_full_screen}
              onChange={(e) => void save({ wait_for_full_screen: e.target.checked })}
            />
          </div>
        </div>
      </div>
      <p className="group-footer" id={ids.calendarNote}>
        When you ask the chat about your plans (&ldquo;what&rsquo;s on tomorrow?&rdquo;), your events for the
        next two weeks and your reminders go to the AI you picked above with that question. Your
        calendar is kept on this computer.
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
