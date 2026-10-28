// Date helpers for the home window. Weeks start on Monday.

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
