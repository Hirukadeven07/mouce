import { useState } from "react";
import { calendarClearDone } from "../lib/ipc";
import { Icon } from "../ui/Icon";
import type { OpenForm } from "./CalendarTab";
import { ItemForm, QuickAdd, valuesFromItem } from "./ItemForm";
import { ItemList } from "./ItemList";
import { useReminders } from "./useCalendar";

/** Things you asked mouce to remind you about: the ones still to do, then the done ones. */
export function RemindersTab(props: { today: Date }) {
  const { today } = props;
  const reminders = useReminders();
  const [form, setForm] = useState<OpenForm | null>(null);
  const open = reminders?.filter((r) => !r.done) ?? null;
  const done = reminders?.filter((r) => r.done) ?? [];

  return (
    <div className="reminders">
      <section className="card" aria-label="Reminders">
        {form ? (
          <ItemForm kind="reminder" initial={form.values} id={form.id} hint={form.hint} onDone={() => setForm(null)} />
        ) : (
          <QuickAdd kind="reminder" now={today} onDraft={(d) => setForm({ values: d.values, id: null, hint: d.hint })} />
        )}
        {reminders?.length === 0 ? (
          <div className="empty-state">
            <Icon name="reminders" size={36} />
            <h2 className="headline">No reminders yet</h2>
            <p className="empty">
              Type something like “remind me to submit the lab report on Friday” and mouce will nudge you at
              the right time.
            </p>
          </div>
        ) : (
          <ItemList
            items={open}
            now={today}
            withDay
            empty="Nothing left to do."
            label="To do"
            onEdit={(item) => setForm({ values: valuesFromItem(item), id: item.id, hint: null })}
          />
        )}
      </section>
      {done.length > 0 && (
        <section className="card" aria-labelledby="done-title">
          <div className="card-head">
            <h2 id="done-title" className="headline">
              Done
            </h2>
            <button type="button" className="link" onClick={() => void calendarClearDone().catch(() => {})}>
              Clear done
            </button>
          </div>
          <ItemList items={done} now={today} withDay empty="" label="Done" />
        </section>
      )}
    </div>
  );
}
