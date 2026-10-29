import { useEffect, useLayoutEffect, useRef, useState } from "react";
import {
  dictionaryChat,
  dictionaryClose,
  dictionaryCurrent,
  dictionaryFit,
  onDictionaryCard,
} from "../lib/ipc";
import type { WordCard } from "../lib/types";
import { Icon } from "../ui/Icon";

/** Meanings shown for each part of speech until "more meanings" is pressed. */
export const SHOWN_SENSES = 3;
/** How long a card saying there is no word stays up, in milliseconds. */
export const NOTICE_MS = 2500;
/** Space around the card for its shadow, in CSS pixels (matches `.dict` in dictionary.css). */
const MARGIN = 10;

function canSpeak(): boolean {
  return typeof window !== "undefined" && "speechSynthesis" in window;
}

/** Says the word with a voice built into Windows; nothing leaves the computer. */
function speak(word: string) {
  const utterance = new SpeechSynthesisUtterance(word);
  utterance.lang = "en-US";
  window.speechSynthesis.cancel();
  window.speechSynthesis.speak(utterance);
}

/** What the AI says the word means where it was found. */
function Explanation(props: { card: WordCard }) {
  const { card } = props;
  if (card.explaining === "off") return null;
  if (card.explaining === "failed") {
    return card.entries.length > 0 ? (
      <p className="dict-note">The AI couldn't explain it this time, so these are the dictionary's meanings.</p>
    ) : null;
  }
  const writing = card.explaining === "writing";
  return (
    <section className="dict-ai" aria-live="polite" aria-busy={writing}>
      <h2 className="dict-label">
        <Icon name="sparkle" size={13} />
        {card.context ? "In this sentence" : "Meaning"}
      </h2>
      {card.explanation ? (
        <p>{card.explanation}</p>
      ) : (
        <span className="dict-dots" role="status" aria-label="mouce is thinking">
          <span />
          <span />
          <span />
        </span>
      )}
    </section>
  );
}

/**
 * The card beside the pointer after Ctrl + right-click on a word: its meanings from the
 * dictionary built into mouce, and what the AI says it means in its sentence. The window
 * never takes the keyboard, so it closes with its button, a click elsewhere, or "Chat about
 * this".
 */
export function DictionaryCard() {
  const [card, setCard] = useState<WordCard | null>(null);
  const [more, setMore] = useState(false);
  const cardRef = useRef<HTMLElement>(null);

  useEffect(() => {
    let live = true;
    const unlisten = onDictionaryCard((c) => live && setCard(c)).catch(() => null);
    // The first word arrives before this page has loaded.
    dictionaryCurrent()
      .then((c) => live && c && setCard((cur) => (cur && cur.id >= c.id ? cur : c)))
      .catch(() => {});
    return () => {
      live = false;
      unlisten.then((un) => un?.()).catch(() => {});
    };
  }, []);

  const id = card?.id;
  useEffect(() => setMore(false), [id]);

  // Rust sizes the window to the card and shows it beside the pointer.
  useLayoutEffect(() => {
    const el = cardRef.current;
    if (!card || !el) return;
    void dictionaryFit(card.id, el.scrollHeight + 2 * MARGIN).catch(() => {});
  }, [card, more]);

  const missing = card?.missing ?? null;
  useEffect(() => {
    if (id === undefined || !missing) return;
    const timer = window.setTimeout(() => void dictionaryClose(id).catch(() => {}), NOTICE_MS);
    return () => window.clearTimeout(timer);
  }, [id, missing]);

  if (!card) return null;

  if (card.missing) {
    return (
      <div className="dict">
        <article className="dict-card dict-card--notice" ref={cardRef} role="status">
          <p>
            {card.missing === "secret"
              ? "This is a password box, so mouce leaves it alone."
              : "mouce couldn't find a word there."}
          </p>
        </article>
      </div>
    );
  }

  const head = card.entries[0];
  const title = head?.word ?? card.word;
  const pronunciation = card.entries.find((e) => e.pronunciation)?.pronunciation;
  const fromForm = head !== undefined && head.word.toLowerCase() !== card.word.toLowerCase();
  const hidden = card.entries.reduce((n, e) => n + Math.max(0, e.senses.length - SHOWN_SENSES), 0);
  const thinking = card.explaining === "writing" || card.explaining === "done";

  return (
    <div className="dict">
      <article className="dict-card" ref={cardRef} aria-labelledby="dict-word">
        <header className="dict-header">
          <div className="dict-heading">
            <h1 id="dict-word" className="dict-word">
              {title}
            </h1>
            {(pronunciation || fromForm) && (
              <p className="dict-sub">
                {pronunciation && <span className="dict-ipa">/{pronunciation}/</span>}
                {fromForm && <span>from “{card.word}”</span>}
              </p>
            )}
          </div>
          {canSpeak() && (
            <button
              type="button"
              className="icon-button"
              aria-label={`Say “${title}”`}
              title="Say it"
              onClick={() => speak(title)}
            >
              <Icon name="speaker" size={17} />
            </button>
          )}
          <button
            type="button"
            className="icon-button"
            aria-label="Close"
            title="Close"
            onClick={() => void dictionaryClose(card.id).catch(() => {})}
          >
            <Icon name="close" size={14} />
          </button>
        </header>

        <Explanation card={card} />

        {card.entries.map((entry) => (
          <section className="dict-entry" key={entry.word + entry.part_of_speech}>
            <h2 className="dict-label">
              {entry.part_of_speech}
              {entry.word !== title && <span className="dict-also"> · {entry.word}</span>}
            </h2>
            <ol className="dict-senses">
              {(more ? entry.senses : entry.senses.slice(0, SHOWN_SENSES)).map((s, i) => (
                <li key={i}>
                  {s.meaning}
                  {s.example && <q className="dict-example">{s.example}</q>}
                </li>
              ))}
            </ol>
          </section>
        ))}

        {card.entries.length === 0 && !thinking && (
          <p className="dict-note">
            {card.explaining === "failed"
              ? "This word isn't in mouce's dictionary, and the AI couldn't be reached."
              : "This word isn't in mouce's dictionary."}
          </p>
        )}

        <footer className="dict-footer">
          {hidden > 0 && (
            <button type="button" className="link" aria-expanded={more} onClick={() => setMore((m) => !m)}>
              {more ? "Fewer meanings" : `${hidden} more meaning${hidden === 1 ? "" : "s"}`}
            </button>
          )}
          <button type="button" onClick={() => void dictionaryChat(card.id).catch(() => {})}>
            <Icon name="chat" size={16} />
            Chat about this
          </button>
        </footer>
        {card.entries.length > 0 && <p className="dict-credit">Meanings from Open English WordNet</p>}
      </article>
    </div>
  );
}
