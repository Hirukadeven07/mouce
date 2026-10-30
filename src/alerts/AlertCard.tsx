import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { alertsAct, alertsCurrent, alertsFit, alertsOpen, onAlerts } from "../lib/ipc";
import type { AlertAction, CalendarItem } from "../lib/types";
import { dayLabel, isoDate, localMs } from "../home/dates";

/** Space around the card for its shadow, in CSS pixels (matches `.alerts` in alerts.css). */
const MARGIN = 10;
const MINUTE = 60_000;

function minutes(n: number): string {
  return n === 1 ? "1 minute" : `${n} minutes`;
}

/** "tomorrow" or "on Mon 2 Nov", to go in the middle of a sentence. */
function onDay(date: string, now: Date): string {
  const day = dayLabel(date, now);
  return /^(Today|Tomorrow|Yesterday)$/.test(day) ? day.toLowerCase() : `on ${day}`;
}

/** What the card says about when: "Starts in 10 minutes · 15:00", "Was due yesterday at 09:00". */
export function whenText(item: CalendarItem, now: Date): string {
  const today = isoDate(now);
  if (item.time === null) {
    return `${dayLabel(item.date, now)}, all day`;
  }
  const at = item.date === today ? `at ${item.time}` : `${onDay(item.date, now)} at ${item.time}`;
  const left = Math.round((localMs(item.date, item.time) - now.getTime()) / MINUTE);
  if (item.kind === "reminder") {
    return left < -1 ? `Was due ${at}` : "Due now";
  }
  if (left > 1 && left < 120) return `Starts in ${minutes(left)} · ${item.time}`;
  if (left >= 120) return `Starts ${at}`;
  if (left >= -1) return `Starting now · ${item.time}`;
  return `Started ${at}`;
}

/** Keeps "in 10 minutes" true while the card stays up. */
function useNow(): Date {
  const [now, setNow] = useState(() => new Date());
  useEffect(() => {
    const id = window.setInterval(() => setNow(new Date()), 30_000);
    return () => window.clearInterval(id);
  }, []);
  return now;
}

/** How long each snooze waits, as the card offers them. */
const SNOOZES: ReadonlyArray<readonly [AlertAction, string]> = [
  ["snooze", "10 min"],
  ["snooze_hour", "1 hour"],
  ["snooze_tomorrow", "Tomorrow"],
];

/** One thing that is due: what and when, and what to do about it. Snooze first asks for
 * how long (`snoozing`). */
function DueItem(props: {
  item: CalendarItem;
  now: Date;
  snoozing: boolean;
  setSnoozing: (on: boolean) => void;
  onAct: (action: AlertAction) => void;
}) {
  const { item, now, snoozing, setSnoozing, onAct } = props;
  const where = item.kind === "reminder" ? "Reminders" : "the calendar";
  return (
    <li className="alerts-item">
      <button
        type="button"
        className="alerts-title"
        title={`Open in ${where}`}
        onClick={() => void alertsOpen(item.id).catch(() => {})}
      >
        {item.title}
      </button>
      <p className="alerts-when">{whenText(item, now)}</p>
      {snoozing ? (
        <div className="button-row" role="group" aria-label={`Snooze “${item.title}” for`}>
          {SNOOZES.map(([action, label]) => (
            <button key={action} type="button" onClick={() => onAct(action)}>
              {label}
            </button>
          ))}
          <button type="button" className="link" onClick={() => setSnoozing(false)}>
            Back
          </button>
        </div>
      ) : (
        <div className="button-row">
          <button type="button" onClick={() => setSnoozing(true)}>
            Snooze…
          </button>
          {item.kind === "reminder" ? (
            <button type="button" className="primary" onClick={() => onAct("done")}>
              Done
            </button>
          ) : (
            <button type="button" className="primary" onClick={() => onAct("dismiss")}>
              Got it
            </button>
          )}
        </div>
      )}
    </li>
  );
}

/**
 * The card beside the mouce when an event or reminder is due. It never takes the keyboard
 * and makes no sound; it stays until each one is snoozed, done or put away.
 */
export function AlertCard() {
  const [due, setDue] = useState<CalendarItem[]>([]);
  const [error, setError] = useState<string | null>(null);
  // The item showing its snooze choices. Kept here so the card is measured again when they
  // open or close.
  const [snoozing, setSnoozing] = useState<number | null>(null);
  const cardRef = useRef<HTMLElement>(null);
  const now = useNow();

  useEffect(() => {
    let live = true;
    const unlisten = onAlerts((d) => live && setDue(d)).catch(() => null);
    // The first reminder arrives before this page has loaded.
    alertsCurrent()
      .then((d) => live && setDue(d))
      .catch(() => {});
    return () => {
      live = false;
      unlisten.then((un) => un?.()).catch(() => {});
    };
  }, []);

  // Rust sizes the window to the card and shows it beside the mouce.
  useLayoutEffect(() => {
    const el = cardRef.current;
    if (!el || due.length === 0) return;
    // `scrollHeight` rounds down; the extra pixel keeps a fraction from adding a scrollbar.
    void alertsFit(el.scrollHeight + 2 * MARGIN + 1).catch(() => {});
  }, [due, error, snoozing]);

  function act(id: number, action: AlertAction) {
    setError(null);
    alertsAct(id, action).catch((e) => setError(String(e)));
  }

  if (due.length === 0) return null;

  return (
    <div className="alerts">
      <article className="alerts-card" ref={cardRef} aria-labelledby="alerts-title">
        <h1 id="alerts-title" className="alerts-label">
          {due.length === 1 ? "Reminder" : `${due.length} reminders`}
        </h1>
        <ul className="alerts-list">
          {due.map((item) => (
            <DueItem
              key={item.id}
              item={item}
              now={now}
              snoozing={snoozing === item.id}
              setSnoozing={(on) => setSnoozing(on ? item.id : null)}
              onAct={(action) => act(item.id, action)}
            />
          ))}
        </ul>
        {error && (
          <p className="notice" role="alert">
            {error}
          </p>
        )}
      </article>
    </div>
  );
}
