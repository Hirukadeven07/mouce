import { useState } from "react";
import { monthTitle } from "./dates";
import { MonthView } from "./MonthView";

/** A month you can page through. Events arrive with the calendar feature. */
export function CalendarTab(props: { today: Date }) {
  const { today } = props;
  const [shown, setShown] = useState({ year: today.getFullYear(), month: today.getMonth() });
  const move = (delta: number) =>
    setShown((s) => {
      const d = new Date(s.year, s.month + delta, 1);
      return { year: d.getFullYear(), month: d.getMonth() };
    });
  const isThisMonth = shown.year === today.getFullYear() && shown.month === today.getMonth();
  return (
    <section className="card calendar" aria-labelledby="calendar-title">
      <div className="card-head">
        <h2 id="calendar-title" aria-live="polite">
          {monthTitle(shown.year, shown.month)}
        </h2>
        <div className="calendar-nav">
          <button type="button" onClick={() => move(-1)} aria-label="Previous month">
            ‹
          </button>
          <button
            type="button"
            onClick={() => setShown({ year: today.getFullYear(), month: today.getMonth() })}
            disabled={isThisMonth}
          >
            Today
          </button>
          <button type="button" onClick={() => move(1)} aria-label="Next month">
            ›
          </button>
        </div>
      </div>
      <MonthView year={shown.year} month={shown.month} today={today} large />
      <p className="empty">
        No events yet. Soon you'll be able to tell mouce “add dentist tomorrow at 3pm” and see it
        here.
      </p>
    </section>
  );
}

/** Things you asked mouce to remind you about. Filled once reminders exist. */
export function RemindersTab() {
  return (
    <section className="card" aria-labelledby="reminders-title">
      <div className="card-head">
        <h2 id="reminders-title">Reminders</h2>
      </div>
      <p className="empty">
        No reminders yet. Soon you'll be able to say “remind me to submit the lab report on Friday”
        and mouce will nudge you at the right time.
      </p>
    </section>
  );
}
