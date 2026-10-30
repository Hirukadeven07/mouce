import { useState, type FormEvent, type KeyboardEvent } from "react";
import { calendarSave, calendarUnderstand } from "../lib/ipc";
import type { CalendarDraft, CalendarInput, CalendarItem, CalendarKind, Repeat } from "../lib/types";
import { isoDate, suggestedTime, todayFor } from "./dates";

/** What the form holds. `remind` is minutes before, as text; "" is no reminder. `repeat`
 * is "" when it does not repeat. */
export interface FormValues {
  title: string;
  date: string;
  /** "HH:MM"; unused for an all-day event. */
  time: string;
  allDay: boolean;
  remind: string;
  repeat: Repeat | "";
}

type Options = ReadonlyArray<readonly [string, string]>;

const REPEATS: ReadonlyArray<readonly [Repeat | "", string]> = [
  ["", "Doesn’t repeat"],
  ["daily", "Every day"],
  ["weekdays", "Every weekday"],
  ["weekly", "Every week"],
  ["monthly", "Every month"],
  ["yearly", "Every year"],
];

/** "Every week", for an item that repeats. */
export function repeatText(repeat: Repeat | null): string | null {
  return repeat ? (REPEATS.find(([value]) => value === repeat)?.[1] ?? null) : null;
}

const TIMED_REMINDERS: Options = [
  ["", "No reminder"],
  ["0", "At the start"],
  ["5", "5 minutes before"],
  ["10", "10 minutes before"],
  ["30", "30 minutes before"],
  ["60", "1 hour before"],
  ["1440", "1 day before"],
];

const ALL_DAY_REMINDERS: Options = [
  ["", "No reminder"],
  ["0", "On the day, at 9:00"],
  ["1440", "The day before, at 9:00"],
];

/** A timed event reminds 10 minutes before unless the user says otherwise. */
const DEFAULT_TIMED_REMIND = "10";

function remindOptions(allDay: boolean): Options {
  return allDay ? ALL_DAY_REMINDERS : TIMED_REMINDERS;
}

/** The form for what the AI read from a note; missing parts get sensible values. */
export function valuesFromDraft(kind: CalendarKind, draft: CalendarDraft, now: Date): FormValues {
  const date = draft.date ?? isoDate(now);
  const repeat = draft.repeat ?? "";
  if (kind === "reminder") {
    const time = draft.time ?? suggestedTime(date, now);
    return { title: draft.title, date, time, allDay: false, remind: "0", repeat };
  }
  return {
    title: draft.title,
    date,
    time: draft.time ?? "",
    allDay: draft.time === null,
    remind: draft.time === null ? "" : DEFAULT_TIMED_REMIND,
    repeat,
  };
}

/** An empty form for `date`. */
export function blankValues(kind: CalendarKind, date: string, now: Date): FormValues {
  return valuesFromDraft(kind, { title: "", date, time: null, repeat: null, understood: true }, now);
}

/** The form for changing a saved item. A repeating one is changed from where it starts. */
export function valuesFromItem(item: CalendarItem): FormValues {
  return {
    title: item.title,
    date: item.repeat ? item.start_date : item.date,
    time: item.time ?? "",
    allDay: item.time === null,
    remind: item.kind === "reminder" ? "0" : item.remind_before === null ? "" : String(item.remind_before),
    repeat: item.repeat ?? "",
  };
}

/** What to save. Rust works out when it reminds. */
export function inputFrom(kind: CalendarKind, v: FormValues): CalendarInput {
  return {
    kind,
    title: v.title,
    date: v.date,
    time: kind === "event" && v.allDay ? null : v.time || null,
    remind_before: kind === "reminder" || v.remind === "" ? null : Number(v.remind),
    repeat: v.repeat || null,
  };
}

/**
 * Checks an event or reminder before it is saved: what the AI read from a note, a new one
 * filled in by hand, or a change to one already saved (`id`). `onDone` gets what was saved,
 * or null when the user cancels.
 */
export function ItemForm(props: {
  kind: CalendarKind;
  initial: FormValues;
  id: number | null;
  /** Said above the fields, such as that the note could not be read. */
  hint?: string | null;
  onDone: (saved: CalendarItem | null) => void;
}) {
  const { kind, initial, id, hint, onDone } = props;
  const [v, setV] = useState(initial);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const set = (change: Partial<FormValues>) => setV((cur) => ({ ...cur, ...change }));
  const noun = kind === "event" ? "event" : "reminder";
  const heading = id === null ? `New ${noun}` : `Change ${noun}`;
  const idFor = (field: string) => `item-${field}`;

  // The two lists of reminders mean different things ("0" is the start, or 9:00 on the
  // day), so switching starts again from the usual choice.
  function setAllDay(allDay: boolean) {
    set({ allDay, remind: allDay ? "" : DEFAULT_TIMED_REMIND });
  }

  async function save(e: FormEvent) {
    e.preventDefault();
    setSaving(true);
    setError(null);
    try {
      onDone(await calendarSave(id, inputFrom(kind, v)));
    } catch (err) {
      setError(String(err));
      setSaving(false);
    }
  }

  function onKeyDown(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      onDone(null);
    }
  }

  const timed = kind === "reminder" || !v.allDay;
  return (
    <form className="item-form" aria-label={heading} onSubmit={(e) => void save(e)} onKeyDown={onKeyDown}>
      <h3 className="item-form-title">{heading}</h3>
      {hint && <p className="item-form-hint">{hint}</p>}
      <label className="item-field item-field--wide" htmlFor={idFor("title")}>
        <span>{kind === "event" ? "What" : "Remind me to"}</span>
        <input
          id={idFor("title")}
          type="text"
          value={v.title}
          maxLength={200}
          required
          autoFocus
          onChange={(e) => set({ title: e.target.value })}
        />
      </label>
      <div className="item-fields">
        <label className="item-field" htmlFor={idFor("date")}>
          <span>{v.repeat ? "Starts" : "Date"}</span>
          <input id={idFor("date")} type="date" value={v.date} required onChange={(e) => set({ date: e.target.value })} />
        </label>
        {timed && (
          <label className="item-field" htmlFor={idFor("time")}>
            <span>Time</span>
            <input id={idFor("time")} type="time" value={v.time} required onChange={(e) => set({ time: e.target.value })} />
          </label>
        )}
        {kind === "event" && (
          <label className="item-check">
            <input type="checkbox" checked={v.allDay} onChange={(e) => setAllDay(e.target.checked)} />
            All day
          </label>
        )}
      </div>
      <div className="item-fields">
        <label className="item-field item-field--grow" htmlFor={idFor("repeat")}>
          <span>Repeats</span>
          <select
            id={idFor("repeat")}
            value={v.repeat}
            onChange={(e) => set({ repeat: e.target.value as FormValues["repeat"] })}
          >
            {REPEATS.map(([value, label]) => (
              <option key={value} value={value}>
                {label}
              </option>
            ))}
          </select>
        </label>
        {kind === "event" && (
          <label className="item-field item-field--grow" htmlFor={idFor("remind")}>
            <span>Reminder</span>
            <select id={idFor("remind")} value={v.remind} onChange={(e) => set({ remind: e.target.value })}>
              {remindOptions(v.allDay).map(([value, label]) => (
                <option key={value} value={value}>
                  {label}
                </option>
              ))}
            </select>
          </label>
        )}
      </div>
      {error && (
        <p className="notice" role="alert">
          {error}
        </p>
      )}
      <div className="button-row button-row--end">
        <button type="button" onClick={() => onDone(null)}>
          Cancel
        </button>
        <button type="submit" className="primary" disabled={saving}>
          {id === null ? `Save ${noun}` : "Save"}
        </button>
      </div>
    </form>
  );
}

/** What `QuickAdd` hands on: the form to show and what to say about it. */
export interface Draft {
  values: FormValues;
  hint: string | null;
}

/** What to tell the user about how a note was read. */
function hintFor(draft: CalendarDraft): string | null {
  if (!draft.understood) return "mouce couldn't read that just now, so check the details.";
  if (draft.date === null) return "There was no date in that, so it's set for today.";
  return null;
}

/** Has the AI read `note` into a form to check. If it cannot, the form holds the note as
 * typed. */
export async function readNote(kind: CalendarKind, note: string, now: Date): Promise<Draft> {
  let draft: CalendarDraft;
  try {
    draft = await calendarUnderstand(kind, note, todayFor(now));
  } catch {
    draft = { title: note, date: null, time: null, repeat: null, understood: false };
  }
  return { values: valuesFromDraft(kind, draft, now), hint: hintFor(draft) };
}

/**
 * A box to type an event or reminder in plain language ("dentist tomorrow at 3pm"). The AI
 * reads it into a draft for the user to check; nothing is saved from here. Adding with the
 * box empty opens an empty form.
 */
export function QuickAdd(props: { kind: CalendarKind; now: Date; date?: string; onDraft: (draft: Draft) => void }) {
  const { kind, now, date, onDraft } = props;
  const [note, setNote] = useState("");
  const [reading, setReading] = useState(false);

  async function add(e: FormEvent) {
    e.preventDefault();
    const text = note.trim();
    if (!text) {
      onDraft({ values: blankValues(kind, date ?? isoDate(now), now), hint: null });
      return;
    }
    setReading(true);
    const draft = await readNote(kind, text, now);
    setReading(false);
    onDraft(draft);
  }

  const label = kind === "event" ? "New event" : "New reminder";
  const placeholder =
    kind === "event"
      ? "Add an event, like “dentist tomorrow at 3pm”"
      : "Remind me to… like “submit the lab report on Friday”";
  return (
    <form className="quick-add" onSubmit={(e) => void add(e)} aria-busy={reading}>
      <input
        type="text"
        aria-label={label}
        placeholder={placeholder}
        value={note}
        maxLength={300}
        disabled={reading}
        onChange={(e) => setNote(e.target.value)}
      />
      <button type="submit" className="primary" disabled={reading}>
        {reading ? "Reading…" : "Add"}
      </button>
    </form>
  );
}
