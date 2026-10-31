import type { HomeTabId } from "../lib/types";
import { useSettings } from "../settings/useSettings";

/** A shortcut as keys to press, "Off", or (for a gesture) words. */
type Keys = string[] | "off";

/** The keys of a saved shortcut ("Ctrl+Alt+M"); an empty one is turned off. */
function hotkey(saved: string): Keys {
  return saved ? saved.split("+") : "off";
}

function KeysView(props: { keys: Keys }) {
  if (props.keys === "off") return <span className="shortcut-off">Off</span>;
  return (
    <span className="shortcut-keys">
      {props.keys.map((key, i) => (
        <kbd key={i}>{key}</kbd>
      ))}
    </span>
  );
}

function Row(props: { label: string; hint?: string; keys: Keys }) {
  return (
    <div className="form-row">
      <div className="form-line">
        <div className="form-label">
          {props.label}
          {props.hint && <small>{props.hint}</small>}
        </div>
        <KeysView keys={props.keys} />
      </div>
    </div>
  );
}

/** Every shortcut mouce has, in one place: the ones that work in any app, and the ones inside mouce. */
export function Shortcuts(props: { onNavigate: (tab: HomeTabId) => void }) {
  const { settings, error } = useSettings();

  if (!settings) {
    return (
      <div className="page">
        <p role="status">{error ?? "Loading…"}</p>
      </div>
    );
  }

  const dictionary: Keys = settings.dictionary_enabled ? ["Ctrl", "Right-click"] : "off";

  return (
    <div className="page shortcuts">
      <h2 className="group-title">In any app</h2>
      <div className="group">
        <Row label="Open the chat" keys={hotkey(settings.chat_hotkey)} />
        <Row label="Circle to look up" hint="Draw around anything on screen to ask about it." keys={hotkey(settings.circle_hotkey)} />
        <Row label="Look up a word" hint="Point at the word." keys={dictionary} />
        <Row label="Explain a phrase" hint="Select the phrase, then point inside it." keys={dictionary} />
      </div>
      <p className="group-footer">
        Change the chat and circle shortcuts in{" "}
        <button type="button" className="link" onClick={() => props.onNavigate("settings")}>
          Settings
        </button>
        .
      </p>

      <h2 className="group-title">While circling</h2>
      <div className="group">
        <Row label="Cancel" keys={["Esc"]} />
        <Row label="Cancel with the mouse" keys={["Right-click"]} />
      </div>

      <h2 className="group-title">In the chat</h2>
      <div className="group">
        <Row label="Send" keys={["Enter"]} />
        <Row label="New line" keys={["Shift", "Enter"]} />
      </div>

      <h2 className="group-title">In mouce home</h2>
      <div className="group">
        <Row label="Move between tabs" keys={["↑", "↓"]} />
        <Row label="Move between days in the calendar" keys={["←", "↑", "↓", "→"]} />
        <Row label="Close a form without saving" keys={["Esc"]} />
      </div>
    </div>
  );
}
