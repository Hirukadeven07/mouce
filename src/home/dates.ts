// Date helpers for the home window. Weeks start on Monday.

import type { Today } from "../lib/types";

export interface DayCell {
  date: Date;
  /** False for the leading and trailing days from the months either side. */
  inMonth: boolean;
  isToday: boolean;
}

export function sameDay(a: Date, b: Date): boolean {
  return (
    a.getFullYear() === b.getFullYear() && a.getMonth() === b.getMonth() && a.getDate() === b.getDate()
  );
}

/** Whole weeks covering `month` (0-based) of `year`, Monday first. */
export function monthGrid(year: number, month: number, today: Date): DayCell[] {
  const first = new Date(year, month, 1);
  const lead = (first.getDay() + 6) % 7;
  const daysInMonth = new Date(year, month + 1, 0).getDate();
  const total = Math.ceil((lead + daysInMonth) / 7) * 7;
  const cells: DayCell[] = [];
  for (let i = 0; i < total; i++) {
    const date = new Date(year, month, 1 - lead + i);
    cells.push({ date, inMonth: date.getMonth() === month, isToday: sameDay(date, today) });
  }
  return cells;
}

export const WEEKDAYS = ["M", "T", "W", "T", "F", "S", "S"];

export function greeting(now: Date): string {
  const h = now.getHours();
  if (h < 12) return "Good morning";
  if (h < 18) return "Good afternoon";
  return "Good evening";
}

/** "Wednesday 28 October" */
export function longDate(d: Date): string {
  return d.toLocaleDateString("en-GB", { weekday: "long", day: "numeric", month: "long" });
}

/** "October 2026" */
export function monthTitle(year: number, month: number): string {
  return new Date(year, month, 1).toLocaleDateString("en-GB", { month: "long", year: "numeric" });
}

const pad = (n: number) => String(n).padStart(2, "0");

/** "2026-10-30", on the computer's clock. */
export function isoDate(d: Date): string {
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

/** Midnight at the start of "2026-10-30", on the computer's clock. */
export function fromIsoDate(date: string): Date {
  const [y, m, d] = date.split("-").map(Number);
  return new Date(y, m - 1, d);
}

/** "15:05" */
export function hhmm(d: Date): string {
  return `${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

/** `date` at `time` ("15:00"), in Unix milliseconds. */
export function localMs(date: string, time: string): number {
  const [h, m] = time.split(":").map(Number);
  const d = fromIsoDate(date);
  d.setHours(h, m, 0, 0);
  return d.getTime();
}

/** The date `days` after `date` (or before, when negative). */
export function addDays(date: string, days: number): string {
  const d = fromIsoDate(date);
  d.setDate(d.getDate() + days);
  return isoDate(d);
}

/** Today on the computer's clock, as the AI needs it to work out "tomorrow". */
export function todayFor(now: Date): Today {
  return {
    date: isoDate(now),
    time: hhmm(now),
    weekday: now.toLocaleDateString("en-GB", { weekday: "long" }),
  };
}

/** The morning: when an all-day event reminds (worked out in Rust), and the time suggested
 * for a reminder on a later day. */
export const MORNING = "09:00";

/** A time to suggest for a reminder on `date`: the next full hour when that is today, else
 * the morning. */
export function suggestedTime(date: string, now: Date): string {
  if (date !== isoDate(now)) return MORNING;
  const next = now.getHours() + 1;
  return next > 23 ? "23:59" : `${pad(next)}:00`;
}

/** "Today", "Tomorrow", "Yesterday", or "Mon 2 Nov" (with the year when it is not this one). */
export function dayLabel(date: string, now: Date): string {
  const today = isoDate(now);
  if (date === today) return "Today";
  if (date === addDays(today, 1)) return "Tomorrow";
  if (date === addDays(today, -1)) return "Yesterday";
  const d = fromIsoDate(date);
  const part = (o: Intl.DateTimeFormatOptions) => d.toLocaleDateString("en-GB", o);
  const label = `${part({ weekday: "short" })} ${d.getDate()} ${part({ month: "short" })}`;
  return d.getFullYear() === now.getFullYear() ? label : `${label} ${d.getFullYear()}`;
}

/** The days a month's grid shows, first and last, as "YYYY-MM-DD". */
export function gridRange(year: number, month: number, today: Date): { from: string; to: string } {
  const cells = monthGrid(year, month, today);
  return { from: isoDate(cells[0].date), to: isoDate(cells[cells.length - 1].date) };
}

/** When something is, to go in a sentence: "tomorrow at 15:00", "on Mon 2 Nov, all day". */
export function whenPhrase(date: string, time: string | null, now: Date): string {
  const day = dayLabel(date, now);
  const on = /^(Today|Tomorrow|Yesterday)$/.test(day) ? day.toLowerCase() : `on ${day}`;
  return time ? `${on} at ${time}` : `${on}, all day`;
}
