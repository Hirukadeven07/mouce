import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { CalendarItem } from "../lib/types";

vi.mock("../lib/ipc");

import * as ipc from "../lib/ipc";
import { RemindersTab } from "./RemindersTab";

const now = new Date(2026, 9, 30, 14, 5);

function reminder(over: Partial<CalendarItem>): CalendarItem {
  return {
    id: 1,
    kind: "reminder",
    title: "Submit the lab report",
    date: "2026-11-06",
    start_date: over.date ?? "2026-11-06",
    end_time: null,
    time: "17:00",
    remind_before: null,
    repeat: null,
    alert_at: new Date(2026, 10, 6, 17, 0).getTime(),
    done: false,
    ...over,
  };
}

beforeEach(() => {
  vi.resetAllMocks();
  vi.mocked(ipc.calendarReminders).mockResolvedValue([]);
  vi.mocked(ipc.onCalendarChanged).mockResolvedValue(() => {});
});

describe("RemindersTab", () => {
  it("says how to add one when there are none", async () => {
    render(<RemindersTab today={now} />);
    expect(await screen.findByText("No reminders yet")).toBeInTheDocument();
    expect(screen.getByRole("textbox", { name: "New reminder" })).toBeInTheDocument();
  });

  it("lists what is still to do, with overdue ones marked, and the done ones apart", async () => {
    vi.mocked(ipc.calendarReminders).mockResolvedValue([
      reminder({ id: 1, title: "Pay rent", date: "2026-10-29", time: "09:00" }),
      reminder({ id: 2 }),
      reminder({ id: 3, title: "Buy milk", done: true }),
    ]);
    render(<RemindersTab today={now} />);
    const todo = await screen.findByRole("list", { name: "To do" });
    const rows = within(todo).getAllByRole("listitem");
    expect(rows[0]).toHaveTextContent("Pay rentOverdue · Yesterday 09:00");
    expect(rows[1]).toHaveTextContent("Submit the lab reportFri 6 Nov 17:00");
    expect(within(screen.getByRole("list", { name: "Done" })).getByText("Buy milk")).toBeInTheDocument();
  });

  it("ticks one off and clears the done ones", async () => {
    vi.mocked(ipc.calendarReminders).mockResolvedValue([reminder({ id: 2 }), reminder({ id: 3, title: "Buy milk", done: true })]);
    vi.mocked(ipc.calendarSetDone).mockResolvedValue();
    vi.mocked(ipc.calendarClearDone).mockResolvedValue();
    const user = userEvent.setup();
    render(<RemindersTab today={now} />);
    await user.click(await screen.findByRole("checkbox", { name: "Done: Submit the lab report" }));
    expect(ipc.calendarSetDone).toHaveBeenCalledWith(2, true);
    await user.click(screen.getByRole("checkbox", { name: "Done: Buy milk" }));
    expect(ipc.calendarSetDone).toHaveBeenCalledWith(3, false);
    await user.click(screen.getByRole("button", { name: "Clear done" }));
    expect(ipc.calendarClearDone).toHaveBeenCalled();
  });

  it("reads a note into a reminder with a time to be reminded", async () => {
    vi.mocked(ipc.calendarUnderstand).mockResolvedValue({
      title: "Call mum",
      date: "2026-10-31",
      time: null,
      end_time: null,
      repeat: "daily",
      understood: true,
    });
    vi.mocked(ipc.calendarSave).mockResolvedValue(reminder({ title: "Call mum" }));
    const user = userEvent.setup();
    render(<RemindersTab today={now} />);
    await user.type(screen.getByRole("textbox", { name: "New reminder" }), "call mum tomorrow{Enter}");
    expect(ipc.calendarUnderstand).toHaveBeenCalledWith("reminder", "call mum tomorrow", expect.anything());
    const form = await screen.findByRole("form", { name: "New reminder" });
    expect(within(form).getByLabelText("Remind me to")).toHaveValue("Call mum");
    expect(within(form).getByLabelText("Time")).toHaveValue("09:00");
    expect(within(form).queryByLabelText("Reminder")).not.toBeInTheDocument();
    expect(within(form).getByLabelText("Repeats")).toHaveValue("daily");
    expect(within(form).getByLabelText("Starts")).toHaveValue("2026-10-31");
    await user.click(within(form).getByRole("button", { name: "Save reminder" }));
    expect(ipc.calendarSave).toHaveBeenCalledWith(null, {
      kind: "reminder",
      title: "Call mum",
      date: "2026-10-31",
      time: "09:00",
      end_time: null,
      remind_before: null,
      repeat: "daily",
    });
  });
});
