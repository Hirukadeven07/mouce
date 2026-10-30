import { describe, expect, it } from "vitest";
import {
  addDays,
  dayLabel,
  fromIsoDate,
  greeting,
  gridRange,
  isoDate,
  localMs,
  longDate,
  monthGrid,
  monthTitle,
  suggestedTime,
  todayFor,
  whenPhrase,
} from "./dates";

describe("monthGrid", () => {
  const today = new Date(2026, 9, 28, 15, 0);

  it("covers whole Monday-first weeks", () => {
    const cells = monthGrid(2026, 9, today);
    expect(cells).toHaveLength(35);
    // 1 October 2026 is a Thursday, so the grid opens on Monday 28 September.
    expect(cells[0].date).toEqual(new Date(2026, 8, 28));
    expect(cells[0].inMonth).toBe(false);
    expect(cells[3].date).toEqual(new Date(2026, 9, 1));
    expect(cells[3].inMonth).toBe(true);
    expect(cells[34].date).toEqual(new Date(2026, 10, 1));
  });

  it("marks only today", () => {
    const marked = monthGrid(2026, 9, today).filter((c) => c.isToday);
    expect(marked.map((c) => c.date.getDate())).toEqual([28]);
  });

  it("has no padding when the month starts on a Monday and fills four weeks", () => {
    // February 2027 starts on a Monday and has 28 days.
    const cells = monthGrid(2027, 1, today);
    expect(cells).toHaveLength(28);
    expect(cells.every((c) => c.inMonth)).toBe(true);
  });

  it("handles a month that needs six rows", () => {
    // August 2026 starts on a Saturday and has 31 days.
    expect(monthGrid(2026, 7, today)).toHaveLength(42);
  });
});

describe("text helpers", () => {
  it("greets by time of day", () => {
    expect(greeting(new Date(2026, 9, 28, 9))).toBe("Good morning");
    expect(greeting(new Date(2026, 9, 28, 12))).toBe("Good afternoon");
    expect(greeting(new Date(2026, 9, 28, 18))).toBe("Good evening");
  });

  it("formats dates in day-month order", () => {
    expect(longDate(new Date(2026, 9, 28))).toBe("Wednesday 28 October");
    expect(monthTitle(2026, 9)).toBe("October 2026");
  });
});

describe("calendar dates", () => {
  it("writes and reads days on the computer's clock", () => {
    expect(isoDate(new Date(2026, 0, 5, 23, 59))).toBe("2026-01-05");
    expect(fromIsoDate("2026-01-05")).toEqual(new Date(2026, 0, 5));
    expect(localMs("2026-10-30", "15:30")).toBe(new Date(2026, 9, 30, 15, 30).getTime());
  });

  it("adds days across months and years", () => {
    expect(addDays("2026-10-31", 1)).toBe("2026-11-01");
    expect(addDays("2026-01-01", -1)).toBe("2025-12-31");
    expect(addDays("2028-02-28", 1)).toBe("2028-02-29");
  });

  it("gives the AI today as it needs it", () => {
    expect(todayFor(new Date(2026, 9, 30, 9, 5))).toEqual({
      date: "2026-10-30",
      time: "09:05",
      weekday: "Friday",
    });
  });

  it("suggests the next hour today and the morning on other days", () => {
    const now = new Date(2026, 9, 30, 14, 5);
    expect(suggestedTime("2026-10-30", now)).toBe("15:00");
    expect(suggestedTime("2026-10-31", now)).toBe("09:00");
    expect(suggestedTime("2026-10-30", new Date(2026, 9, 30, 23, 30))).toBe("23:59");
  });

  it("names nearby days and dates the rest", () => {
    const now = new Date(2026, 9, 30, 12);
    expect(dayLabel("2026-10-30", now)).toBe("Today");
    expect(dayLabel("2026-10-31", now)).toBe("Tomorrow");
    expect(dayLabel("2026-10-29", now)).toBe("Yesterday");
    expect(dayLabel("2026-11-02", now)).toBe("Mon 2 Nov");
    expect(dayLabel("2027-01-04", now)).toBe("Mon 4 Jan 2027");
  });

  it("covers the whole grid a month shows", () => {
    expect(gridRange(2026, 9, new Date(2026, 9, 1))).toEqual({ from: "2026-09-28", to: "2026-11-01" });
  });

  it("puts a day and time into a sentence", () => {
    const now = new Date(2026, 9, 30, 12);
    expect(whenPhrase("2026-10-31", "15:00", now)).toBe("tomorrow at 15:00");
    expect(whenPhrase("2026-10-30", null, now)).toBe("today, all day");
    expect(whenPhrase("2026-11-02", "09:00", now)).toBe("on Mon 2 Nov at 09:00");
  });
});
