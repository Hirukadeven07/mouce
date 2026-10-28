import { describe, expect, it } from "vitest";
import { greeting, longDate, monthGrid, monthTitle } from "./dates";

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
