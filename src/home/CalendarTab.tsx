import { useState } from "react";
import type { CalendarItem } from "../lib/types";
import { Icon } from "../ui/Icon";
import { fromIsoDate, gridRange, isoDate, longDate, monthTitle } from "./dates";
import { ItemForm, QuickAdd, valuesFromItem, type FormValues } from "./ItemForm";
import { ItemList } from "./ItemList";
import { MonthView } from "./MonthView";
import { byDate, useCalendarRange } from "./useCalendar";

/** The form on screen: a draft from a note, a new one, or a change to `id`. */
export interface OpenForm {
  values: FormValues;
  id: number | null;
  hint: string | null;
}

/** A month you can page through, with what is on each day and a box to add more. */
export function CalendarTab(props: { today: Date }) {
  const { today } = props;
  const [selected, setSelected] = useState(() => isoDate(today));
  const [form, setForm] = useState<OpenForm | null>(null);
  const day = fromIsoDate(selected);
  const shown = { year: day.getFullYear(), month: day.getMonth() };
  const { from, to } = gridRange(shown.year, shown.month, today);
  const items = useCalendarRange(from, to);
  const days = byDate(items);

  // Paging moves the selection to the 1st, or to today in this month.
  const move = (delta: number) => {
    const first = new Date(shown.year, shown.month + delta, 1);
    const thisMonth = first.getFullYear() === today.getFullYear() && first.getMonth() === today.getMonth();
    setSelected(isoDate(thisMonth ? today : first));
  };

  function saved(item: CalendarItem | null) {
    setForm(null);
    if (item) setSelected(item.date);
  }

  return (
    <section className="card calendar" aria-labelledby="calendar-title">
      <div className="card-head">
        <h2 id="calendar-title" className="headline" aria-live="polite">
          {monthTitle(shown.year, shown.month)}
        </h2>
        <div className="calendar-nav">
          <button type="button" className="icon-button" onClick={() => move(-1)} aria-label="Previous month">
            <Icon name="chevronLeft" size={16} />
          </button>
          <button type="button" onClick={() => setSelected(isoDate(today))} disabled={selected === isoDate(today)}>
            Today
          </button>
          <button type="button" className="icon-button" onClick={() => move(1)} aria-label="Next month">
            <Icon name="chevronRight" size={16} />
          </button>
        </div>
      </div>
      {form ? (
        <ItemForm kind="event" initial={form.values} id={form.id} hint={form.hint} onDone={saved} />
      ) : (
        <QuickAdd
          kind="event"
          now={today}
          date={selected}
          onDraft={(d) => setForm({ values: d.values, id: null, hint: d.hint })}
        />
      )}
      <div className="calendar-body">
        <MonthView
          year={shown.year}
          month={shown.month}
          today={today}
          large
          marked={new Set(days.keys())}
          selected={selected}
          onSelect={setSelected}
        />
        <div className="calendar-day">
          <h3 className="calendar-day-title">{longDate(day)}</h3>
          <ItemList
            items={items && (days.get(selected) ?? [])}
            now={today}
            empty="Nothing planned."
            label={`Plans for ${longDate(day)}`}
            onEdit={(item) => setForm({ values: valuesFromItem(item), id: item.id, hint: null })}
          />
        </div>
      </div>
    </section>
  );
}
