import type { CalendarItem, CalendarKind } from "../lib/types";
import { whenPhrase } from "../home/dates";
import { ItemForm, repeatText, type Draft } from "../home/ItemForm";

/** A "remind me…" or "add … to my calendar" message, from reading it to the outcome. Kept
 * only in this view: it is not part of the conversation the AI sees. */
export interface Plan {
  kind: CalendarKind;
  /** The message as typed. */
  note: string;
  /** Null while the AI reads the note. */
  draft: Draft | null;
  /** Set once it is saved, or `"cancelled"`. */
  outcome: CalendarItem | "cancelled" | null;
}

/** What mouce says once it is saved: "Added to your calendar: “Dentist”, tomorrow at 15:00." */
export function savedText(item: CalendarItem, now: Date): string {
  const when = whenPhrase(item.date, item.time, now);
  const repeats = repeatText(item.repeat);
  const detail = `“${item.title}”, ${when}${repeats ? `, ${repeats.toLowerCase()}` : ""}.`;
  return item.kind === "reminder" ? `Reminder set: ${detail}` : `Added to your calendar: ${detail}`;
}

/**
 * The message and mouce's side of it in the chat log: dots while the note is read, then the
 * form to check (nothing is saved until Save), then what happened. "Ask instead" sends the
 * message to the AI as an ordinary question.
 */
export function ChatPlan(props: {
  plan: Plan;
  onDone: (outcome: CalendarItem | "cancelled") => void;
  onAsk: () => void;
}) {
  const { plan, onDone, onAsk } = props;
  return (
    <>
      <div className="bubble-row bubble-row--mine">
        <div className="bubble bubble--mine">
          <p className="bubble-text">{plan.note}</p>
        </div>
      </div>
      <div className="bubble-row chat-plan">
        {plan.outcome === "cancelled" ? (
          <div className="bubble">
            <p className="bubble-text">Okay, nothing was saved.</p>
          </div>
        ) : plan.outcome ? (
          <div className="bubble" role="status">
            <p className="bubble-text">{savedText(plan.outcome, new Date())}</p>
          </div>
        ) : plan.draft ? (
          <>
            <ItemForm
              kind={plan.kind}
              initial={plan.draft.values}
              id={null}
              hint={plan.draft.hint}
              onDone={(saved) => onDone(saved ?? "cancelled")}
            />
            <button type="button" className="link chat-plan-ask" onClick={onAsk}>
              Ask mouce about it instead
            </button>
          </>
        ) : (
          <div className="bubble">
            <span className="typing" aria-label="mouce is reading that">
              <span />
              <span />
              <span />
            </span>
          </div>
        )}
      </div>
    </>
  );
}
