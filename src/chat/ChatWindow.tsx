import { useEffect, useId, useRef, useState } from "react";
import logoUrl from "../assets/mouce.png";
import { activeModel } from "../home/useAiStatus";
import {
  chatAttachment,
  chatClear,
  chatDiscardAttachment,
  chatHistory,
  chatSend,
  chatStop,
  circleStart,
  getSettings,
  onChatAttachment,
  onSettingsChanged,
  openHome,
} from "../lib/ipc";
import type { ChatTurn, Settings } from "../lib/types";
import { PROVIDERS } from "../settings/AiSettings";
import { errorText } from "../settings/useSettings";
import { Icon } from "../ui/Icon";

/** Matches `chat::LOOK_QUESTION` in Rust: the question when only the screen is shared. */
export const LOOK_QUESTION = "What's on my screen?";
/** Matches `chat::CIRCLE_QUESTION` in Rust: the question when only a circled area is sent. */
export const CIRCLE_QUESTION = "What is this?";

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

/** Where a circled area goes when it is asked about. */
export function circleNote(settings: Settings | null): string {
  if (!settings) return "Only this part of your screen is sent with your question.";
  if (settings.provider === "ollama") return "Only this part is used, and it stays on this computer.";
  return `Only this part is sent to ${providerName(settings)} with your question.`;
}

/** The question being answered right now, and what is written of the answer so far. */
interface Pending {
  question: string;
  looked: boolean;
  circled: boolean;
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
        {turn.circled && (
          <span className="bubble-chip">
            <Icon name="circle" size={13} />
            Circled
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
  // Preview of the circled area waiting to be asked about.
  const [attachment, setAttachment] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const scrollRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLTextAreaElement>(null);
  const mounted = useRef(true);
  const ids = { input: useId(), lookNote: useId(), circleNote: useId() };

  useEffect(() => {
    mounted.current = true;
    const unlisteners = [
      onSettingsChanged((s) => mounted.current && setSettings(s)),
      onChatAttachment((preview) => {
        if (!mounted.current) return;
        setAttachment(preview);
        inputRef.current?.focus();
      }),
    ].map((p) => p.catch(() => null));
    chatAttachment()
      .then((preview) => mounted.current && setAttachment(preview))
      .catch(() => {});
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
      unlisteners.forEach((p) => p.then((un) => un?.()).catch(() => {}));
    };
  }, []);

  useEffect(() => {
    const scroller = scrollRef.current;
    if (scroller) scroller.scrollTop = scroller.scrollHeight;
  }, [turns, pending]);

  async function ask(text: string, withScreen: boolean) {
    // Rust sends the circled area unless the whole screen is shared instead.
    const circled = !withScreen && attachment !== null;
    if (pending || (!text && !withScreen && !circled)) return;
    setError(null);
    setDraft("");
    setLook(false);
    const question = text || (withScreen ? LOOK_QUESTION : CIRCLE_QUESTION);
    setPending({ question, looked: withScreen, circled, answer: "" });
    if (circled) setAttachment(null);
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
    // A circled area comes back if its question failed; otherwise it has been used.
    const waiting = await chatAttachment().catch(() => null);
    if (mounted.current) setAttachment(waiting);
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
  const canSend = !busy && (draft.trim() !== "" || look || attachment !== null);
  const circleKeys = settings?.circle_hotkey;

  return (
    <div className="chat" ref={scrollRef}>
      <header className="chat-header">
        <div className="chat-bar glass">
          <button
            type="button"
            className="chat-avatar"
            aria-label="Open mouce home"
            title="Open mouce home"
            onClick={() => void openHome().catch(() => {})}
          >
            <img src={logoUrl} alt="" width={26} height={26} />
          </button>
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
            {circleKeys && (
              <p className="chat-empty-tip">Press {circleKeys} in any app to circle something.</p>
            )}
          </div>
        )}
        {turns.map((t, i) => (
          <Bubble key={i} turn={t} note={notes[i]} />
        ))}
        {pending && (
          <>
            <Bubble
              turn={{
                role: "user",
                text: pending.question,
                looked: pending.looked,
                circled: pending.circled,
              }}
            />
            <Bubble
              turn={{ role: "assistant", text: pending.answer, looked: false, circled: false }}
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
          {attachment && (
            <div className="chat-attachment">
              <img src={attachment} alt="The part of your screen you circled" />
              <div className="chat-attachment-text">
                <strong>Circled</strong>
                <p id={ids.circleNote}>{circleNote(settings)}</p>
              </div>
              <button
                type="button"
                className="icon-button chat-attachment-remove"
                aria-label="Remove the circled area"
                title="Remove"
                disabled={busy}
                onClick={() => {
                  setAttachment(null);
                  void chatDiscardAttachment().catch(() => {});
                }}
              >
                <Icon name="close" size={14} />
              </button>
            </div>
          )}
          <label htmlFor={ids.input} className="visually-hidden">
            Message
          </label>
          <textarea
            id={ids.input}
            ref={inputRef}
            rows={2}
            value={draft}
            placeholder={
              look
                ? "Ask about your screen (optional)"
                : attachment
                  ? "Ask about what you circled (optional)"
                  : "Ask mouce…"
            }
            aria-describedby={look ? ids.lookNote : attachment ? ids.circleNote : undefined}
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
            <div className="chat-tools">
              <button
                type="button"
                className={"chip" + (look ? " chip--on" : "")}
                aria-pressed={look}
                disabled={busy || attachment !== null}
                onClick={() => setLook((l) => !l)}
              >
                <Icon name="look" size={16} />
                Look at my screen
              </button>
              <button
                type="button"
                className="chip"
                disabled={busy}
                title={circleKeys ? `Circle to look up (${circleKeys})` : "Circle to look up"}
                onClick={() => void circleStart().catch(() => {})}
              >
                <Icon name="circle" size={16} />
                Circle
              </button>
            </div>
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
