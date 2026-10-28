import { useEffect, useId, useRef, useState } from "react";
import logoUrl from "../assets/mouce.png";
import { activeModel } from "../home/useAiStatus";
import {
  chatClear,
  chatHistory,
  chatSend,
  chatStop,
  getSettings,
  onSettingsChanged,
} from "../lib/ipc";
import type { ChatTurn, Settings } from "../lib/types";
import { PROVIDERS } from "../settings/AiSettings";
import { errorText } from "../settings/useSettings";
import { Icon } from "../ui/Icon";

/** Matches `chat::LOOK_QUESTION` in Rust: the question when only the screen is shared. */
export const LOOK_QUESTION = "What's on my screen?";

function providerName(settings: Settings): string {
  return PROVIDERS.find((p) => p.id === settings.provider)?.label ?? settings.provider;
}

/** Where a screen picture goes, said plainly before the user shares one. */
export function lookNote(settings: Settings | null): string {
  if (!settings) return "mouce will take one picture of this screen to answer.";
  if (settings.provider === "ollama") {
    return "mouce will take one picture of this screen. It stays on this computer.";
  }
  return `mouce will take one picture of this screen and send it to ${providerName(settings)}.`;
}

/** The question being answered right now, and what is written of the answer so far. */
interface Pending {
  question: string;
  looked: boolean;
  answer: string;
}

function Bubble(props: { turn: ChatTurn; note?: string; writing?: boolean }) {
  const { turn, note, writing } = props;
  const mine = turn.role === "user";
  return (
    <div className={"bubble-row" + (mine ? " bubble-row--mine" : "")}>
      <div className={"bubble" + (mine ? " bubble--mine" : "")}>
        {turn.looked && (
          <span className="bubble-chip">
            <Icon name="look" size={13} />
            Shared screen
          </span>
        )}
        {turn.text && <p className="bubble-text">{turn.text}</p>}
        {writing && (
          <span className="typing" aria-label="mouce is writing">
            <span />
            <span />
            <span />
          </span>
        )}
      </div>
      {note && <p className="bubble-note">{note}</p>}
    </div>
  );
}

/** The chat panel beside the mouce: ask a question, optionally about the screen. */
export function ChatWindow() {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [turns, setTurns] = useState<ChatTurn[]>([]);
  // Notes under answers ("stopped", "answered by the local AI"), by turn index.
  const [notes, setNotes] = useState<Record<number, string>>({});
  const [pending, setPending] = useState<Pending | null>(null);
  const [draft, setDraft] = useState("");
  const [look, setLook] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const scrollRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const mounted = useRef(true);
  const ids = { input: useId(), lookNote: useId() };

  useEffect(() => {
    mounted.current = true;
    const unlisten = onSettingsChanged((s) => mounted.current && setSettings(s)).catch(() => null);
    getSettings()
      .then((s) => mounted.current && setSettings((cur) => cur ?? s))
      .catch(() => {});
    chatHistory()
      .then((t) => mounted.current && setTurns(t))
      .catch((e) => mounted.current && setError(errorText(e)));
    inputRef.current?.focus();
    // Opened again with the shortcut or the mouce: be ready to type straight away.
    const onFocus = () => inputRef.current?.focus();
    window.addEventListener("focus", onFocus);
    return () => {
      mounted.current = false;
      window.removeEventListener("focus", onFocus);
      unlisten.then((un) => un?.()).catch(() => {});
    };
  }, []);

  useEffect(() => {
    const scroller = scrollRef.current;
    if (scroller) scroller.scrollTop = scroller.scrollHeight;
  }, [turns, pending]);

  async function ask(text: string, withScreen: boolean) {
    if (pending || (!text && !withScreen)) return;
    setError(null);
    setDraft("");
    setLook(false);
    setPending({ question: text || LOOK_QUESTION, looked: withScreen, answer: "" });
    let note: string | null = null;
    try {
      const reply = await chatSend(text, withScreen, (chunk) => {
        if (!mounted.current) return;
        setPending((p) => p && { ...p, answer: p.answer + chunk });
      });
      if (reply.stopped) note = "Stopped";
      else if (reply.used_local && settings) {
        note = `Answered by the local AI because ${providerName(settings)} didn't respond`;
      }
    } catch (e) {
      if (!mounted.current) return;
      setError(errorText(e));
      setDraft(text);
      setLook(withScreen);
    }
    // The chat in Rust is the source of truth: a failed or empty question is gone again.
    const latest = await chatHistory().catch(() => null);
    if (!mounted.current) return;
    if (latest) {
      setTurns(latest);
      const last = latest.length - 1;
      if (note && latest[last]?.role === "assistant") setNotes((n) => ({ ...n, [last]: note! }));
    }
    setPending(null);
    inputRef.current?.focus();
  }

  async function newChat() {
    try {
      await chatClear();
      setTurns([]);
      setNotes({});
      setError(null);
    } catch (e) {
      setError(errorText(e));
    }
    inputRef.current?.focus();
  }

  const model = settings ? activeModel(settings) : null;
  const busy = pending !== null;
  const canSend = !busy && (draft.trim() !== "" || look);

  return (
    <div className="chat" ref={scrollRef}>
      <header className="chat-header">
        <div className="chat-bar glass">
          <img className="chat-avatar" src={logoUrl} alt="" width={26} height={26} />
          <div className="chat-title">
            <h1>mouce</h1>
            {settings && (
              <p className="chat-model">
                {settings.provider === "ollama" ? "Local" : providerName(settings)}
                {model ? ` · ${model}` : ""}
              </p>
            )}
          </div>
          <button
            type="button"
            className="icon-button"
            aria-label="New chat"
            title="New chat"
            onClick={() => void newChat()}
            disabled={busy || turns.length === 0}
          >
            <Icon name="compose" size={17} />
          </button>
        </div>
      </header>

      <div className="chat-log" role="log" aria-label="Conversation">
        {turns.length === 0 && !pending && (
          <div className="chat-empty">
            <p className="headline">Ask me anything</p>
            <p className="muted">Or let me look at your screen.</p>
            <button type="button" onClick={() => void ask("", true)}>
              <Icon name="look" size={16} />
              {LOOK_QUESTION}
            </button>
          </div>
        )}
        {turns.map((t, i) => (
          <Bubble key={i} turn={t} note={notes[i]} />
        ))}
        {pending && (
          <>
            <Bubble turn={{ role: "user", text: pending.question, looked: pending.looked }} />
            <Bubble
              turn={{ role: "assistant", text: pending.answer, looked: false }}
              writing={pending.answer === ""}
            />
          </>
        )}
      </div>

      <form
        className="chat-compose"
        onSubmit={(e) => {
          e.preventDefault();
          if (canSend) void ask(draft.trim(), look);
        }}
      >
        {error && (
          <p className="notice chat-error" role="alert">
            {error}
          </p>
        )}
        <div className="compose-box glass">
          <label htmlFor={ids.input} className="visually-hidden">
            Message
          </label>
          <textarea
            id={ids.input}
            ref={inputRef}
            rows={2}
            value={draft}
            placeholder={look ? "Ask about your screen (optional)" : "Ask mouce…"}
            aria-describedby={look ? ids.lookNote : undefined}
            onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
                e.preventDefault();
                e.currentTarget.form?.requestSubmit();
              }
            }}
          />
          {look && (
            <p id={ids.lookNote} className="chat-look-note">
              {lookNote(settings)}
            </p>
          )}
          <div className="chat-actions">
            <button
              type="button"
              className={"chip" + (look ? " chip--on" : "")}
              aria-pressed={look}
              disabled={busy}
              onClick={() => setLook((l) => !l)}
            >
              <Icon name="look" size={16} />
              Look at my screen
            </button>
            {busy ? (
              <button
                type="button"
                className="primary icon-button"
                aria-label="Stop"
                title="Stop"
                onClick={() => void chatStop().catch(() => {})}
              >
                <Icon name="stop" size={16} />
              </button>
            ) : (
              <button type="submit" className="primary icon-button" aria-label="Send" title="Send" disabled={!canSend}>
                <Icon name="send" size={16} />
              </button>
            )}
          </div>
        </div>
      </form>
    </div>
  );
}
