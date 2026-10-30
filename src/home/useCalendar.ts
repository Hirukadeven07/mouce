import { useEffect, useState } from "react";
import { calendarBetween, calendarReminders, onCalendarChanged } from "../lib/ipc";
import type { CalendarItem } from "../lib/types";

/** Loads with `load`, and again whenever events or reminders change in any window. `null`
 * until the first load. A failed load leaves what was there. */
function useCalendarData(load: () => Promise<CalendarItem[]>, key: string): CalendarItem[] | null {
  const [items, setItems] = useState<CalendarItem[] | null>(null);
  const [version, setVersion] = useState(0);

  useEffect(() => {
    let alive = true;
    const unlisten = onCalendarChanged(() => alive && setVersion((v) => v + 1)).catch(() => null);
    return () => {
      alive = false;
      unlisten.then((un) => un?.()).catch(() => {});
    };
  }, []);

  useEffect(() => {
    let alive = true;
    load()
      .then((found) => alive && setItems(found))
      .catch(() => {});
    return () => {
      alive = false;
    };
    // `key` stands for what `load` reads.
  }, [key, version]);

  return items;
}

/** Events and reminders on the days `from` to `to` ("YYYY-MM-DD", both included). */
export function useCalendarRange(from: string, to: string): CalendarItem[] | null {
  return useCalendarData(() => calendarBetween(from, to), `${from}/${to}`);
}

/** Every reminder, the ones still to do first. */
export function useReminders(): CalendarItem[] | null {
  return useCalendarData(calendarReminders, "reminders");
}

/** `items` grouped by their date. */
export function byDate(items: CalendarItem[] | null): Map<string, CalendarItem[]> {
  const days = new Map<string, CalendarItem[]>();
  for (const item of items ?? []) {
    const day = days.get(item.date);
    if (day) day.push(item);
    else days.set(item.date, [item]);
  }
  return days;
}
