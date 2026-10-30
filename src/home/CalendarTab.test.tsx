import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { CalendarItem } from "../lib/types";

vi.mock("../lib/ipc");

import * as ipc from "../lib/ipc";
import { CalendarTab } from "./CalendarTab";
import { clashes, clashText, inputFrom, valuesFromDraft, valuesFromItem } from "./ItemForm";

// Friday 30 October 2026, just after two in the afternoon.
const now = new Date(2026, 9, 30, 14, 5);

function item(over: Partial<CalendarItem>): CalendarItem {
  return {
    id: 1,
    kind: "event",
    title: "Dentist",
    date: "2026-10-30",
    start_date: over.date ?? "2026-10-30",
    end_time: null,
    time: "15:00",
    remind_before: 10,
    repeat: null,
    alert_at: new Date(2026, 9, 30, 14, 50).getTime(),
    done: false,
    ...over,
  };
}

let changed: (() => void) | undefined;

beforeEach(() => {
  vi.resetAllMocks();
  changed = undefined;
  vi.mocked(ipc.calendarBetween).mockResolvedValue([]);
  vi.mocked(ipc.onCalendarChanged).mockImplementation(async (cb) => {
    changed = cb;
    return () => {};
  });
});

describe("the form's values", () => {
  it("fills in what the note left out", () => {
    const draft = { title: "Gym", date: null, time: null, end_time: null, repeat: null, understood: true };
    const event = valuesFromDraft("event", draft, now);
    expect(event).toEqual({ title: "Gym", date: "2026-10-30", time: "", end: "", allDay: true, remind: "", repeat: "" });
    const timed = valuesFromDraft("event", { ...draft, date: "2026-11-02", time: "07:30", repeat: "weekly" }, now);
    expect(timed).toMatchObject({ allDay: false, time: "07:30", remind: "10", repeat: "weekly" });
    const reminder = valuesFromDraft("reminder", { ...draft, title: "Call" }, now);
    expect(reminder).toMatchObject({ date: "2026-10-30", time: "15:00" });
  });

  it("sends what to save, leaving when to remind to Rust", () => {
    const values = { title: "Dentist", date: "2026-10-31", time: "15:00", end: "", allDay: false, remind: "30", repeat: "" as const };
    expect(inputFrom("event", values)).toEqual({
      kind: "event",
      title: "Dentist",
      date: "2026-10-31",
      time: "15:00",
      end_time: null,
      remind_before: 30,
      repeat: null,
    });
    expect(inputFrom("event", { ...values, allDay: true, remind: "", repeat: "yearly" })).toMatchObject({
      time: null,
      remind_before: null,
      repeat: "yearly",
    });
    expect(inputFrom("reminder", values)).toMatchObject({ remind_before: null, time: "15:00" });
  });

  it("finds clashes, taking an event with no end to last an hour", () => {
    const v = { title: "Call", date: "2026-10-30", time: "14:30", end: "", allDay: false, remind: "", repeat: "" as const };
    const dentist = item({ id: 1, time: "15:00", end_time: null });
    const lunch = item({ id: 2, title: "Lunch", time: "12:00", end_time: "13:00" });
    const holiday = item({ id: 3, title: "Holiday", time: null });
    expect(clashes(v, [dentist, lunch, holiday], null)).toEqual([dentist]);
    expect(clashes({ ...v, end: "14:45" }, [dentist], null)).toEqual([]);
    expect(clashes(v, [dentist], 1)).toEqual([]);
    expect(clashes({ ...v, time: "" }, [dentist], null)).toEqual([]);
    expect(clashText([dentist, lunch])).toBe("Overlaps “Dentist” (15:00) and “Lunch” (12:00–13:00).");
    expect(clashText([])).toBeNull();
  });

  it("changes a repeating item from where it starts", () => {
    const weekly = item({ date: "2026-10-30", start_date: "2026-10-02", repeat: "weekly" });
    expect(valuesFromItem(weekly)).toMatchObject({ date: "2026-10-02", repeat: "weekly" });
    expect(valuesFromItem(item({}))).toMatchObject({ date: "2026-10-30", repeat: "" });
  });
});

describe("CalendarTab", () => {
  it("reads a note, lets the user check it, and saves only on Save", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.calendarUnderstand).mockResolvedValue({
      title: "Dentist",
      date: "2026-10-31",
      time: "15:00",
      end_time: null,
      repeat: null,
      understood: true,
    });
    const saved = item({ id: 7, date: "2026-10-31" });
    vi.mocked(ipc.calendarSave).mockResolvedValue(saved);
    render(<CalendarTab today={now} />);

    await user.type(screen.getByRole("textbox", { name: "New event" }), "dentist tomorrow at 3pm{Enter}");
    expect(ipc.calendarUnderstand).toHaveBeenCalledWith("event", "dentist tomorrow at 3pm", {
      date: "2026-10-30",
      time: "14:05",
      weekday: "Friday",
    });
    const form = await screen.findByRole("form", { name: "New event" });
    expect(within(form).getByLabelText("What")).toHaveValue("Dentist");
    expect(within(form).getByLabelText("Date")).toHaveValue("2026-10-31");
    expect(within(form).getByLabelText("Starts at")).toHaveValue("15:00");
    expect(within(form).getByLabelText("Reminder")).toHaveValue("10");
    expect(ipc.calendarSave).not.toHaveBeenCalled();

    await user.selectOptions(within(form).getByLabelText("Reminder"), "1 hour before");
    await user.click(within(form).getByRole("button", { name: "Save event" }));
    expect(ipc.calendarSave).toHaveBeenCalledWith(null, {
      kind: "event",
      title: "Dentist",
      date: "2026-10-31",
      time: "15:00",
      end_time: null,
      remind_before: 60,
      repeat: null,
    });
    // The form closes and the saved day is shown.
    expect(await screen.findByRole("textbox", { name: "New event" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Saturday 31 October" })).toBeInTheDocument();
  });

  it("opens the note as typed when the AI cannot read it", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.calendarUnderstand).mockRejectedValue("no AI");
    render(<CalendarTab today={now} />);
    await user.type(screen.getByRole("textbox", { name: "New event" }), "team lunch{Enter}");
    const form = await screen.findByRole("form", { name: "New event" });
    expect(within(form).getByText(/couldn't read that/)).toBeInTheDocument();
    expect(within(form).getByLabelText("What")).toHaveValue("team lunch");
    expect(within(form).getByLabelText("Date")).toHaveValue("2026-10-30");
  });

  it("opens an empty form for the selected day without asking the AI", async () => {
    const user = userEvent.setup();
    render(<CalendarTab today={now} />);
    await user.click(screen.getByRole("button", { name: /Monday 26 October/ }));
    await user.click(screen.getByRole("button", { name: "Add" }));
    const form = screen.getByRole("form", { name: "New event" });
    expect(within(form).getByLabelText("Date")).toHaveValue("2026-10-26");
    expect(ipc.calendarUnderstand).not.toHaveBeenCalled();
  });

  it("offers the all-day reminders for an all-day event", async () => {
    const user = userEvent.setup();
    render(<CalendarTab today={now} />);
    await user.click(screen.getByRole("button", { name: "Add" }));
    const form = screen.getByRole("form", { name: "New event" });
    expect(within(form).queryByLabelText("Starts at")).not.toBeInTheDocument();
    expect(within(form).getByRole("option", { name: "On the day, at 9:00" })).toBeInTheDocument();
    await user.click(within(form).getByRole("checkbox", { name: "All day" }));
    expect(within(form).getByLabelText("Starts at")).toBeInTheDocument();
    expect(within(form).getByLabelText("Reminder")).toHaveValue("10");
  });

  it("shows why a save failed and keeps the form", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.calendarSave).mockRejectedValue("Give it a title");
    render(<CalendarTab today={now} />);
    await user.click(screen.getByRole("button", { name: "Add" }));
    await user.type(screen.getByLabelText("What"), "  x");
    await user.click(screen.getByRole("button", { name: "Save event" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Give it a title");
    expect(screen.getByRole("form", { name: "New event" })).toBeInTheDocument();
  });

  it("cancels with Escape", async () => {
    const user = userEvent.setup();
    render(<CalendarTab today={now} />);
    await user.click(screen.getByRole("button", { name: "Add" }));
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("form", { name: "New event" })).not.toBeInTheDocument();
  });

  it("lists the selected day's plans and marks days that have some", async () => {
    vi.mocked(ipc.calendarBetween).mockResolvedValue([
      item({ id: 1, title: "Holiday", time: null, remind_before: null, alert_at: null }),
      item({ id: 2 }),
      item({ id: 3, title: "Gym", date: "2026-10-26", time: "07:30" }),
    ]);
    const user = userEvent.setup();
    render(<CalendarTab today={now} />);
    expect(ipc.calendarBetween).toHaveBeenCalledWith("2026-09-28", "2026-11-01");
    const list = await screen.findByRole("list", { name: "Plans for Friday 30 October" });
    const rows = within(list).getAllByRole("listitem");
    expect(rows.map((r) => r.querySelector(".item-title")?.textContent)).toEqual(["Holiday", "Dentist"]);
    expect(rows[0]).toHaveTextContent("All day");
    expect(rows[1]).toHaveTextContent("15:00 · Reminds 10 minutes before");
    expect(screen.getByRole("button", { name: "Monday 26 October, has plans" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Tuesday 27 October" })).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /Monday 26 October/ }));
    expect(screen.getByRole("list", { name: "Plans for Monday 26 October" })).toHaveTextContent("Gym");
  });

  it("moves the selected day with the arrow keys, into the next month too", async () => {
    const user = userEvent.setup();
    render(<CalendarTab today={now} />);
    const today = screen.getByRole("button", { name: /Friday 30 October/ });
    today.focus();
    await user.keyboard("{ArrowRight}");
    expect(screen.getByRole("button", { name: /Saturday 31 October/ })).toHaveFocus();
    await user.keyboard("{ArrowDown}");
    expect(screen.getByRole("heading", { name: "November 2026" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Saturday 7 November/ })).toHaveFocus();
  });

  it("changes an event in the same form", async () => {
    vi.mocked(ipc.calendarBetween).mockResolvedValue([item({ id: 4 })]);
    vi.mocked(ipc.calendarSave).mockResolvedValue(item({ id: 4, title: "Doctor" }));
    const user = userEvent.setup();
    render(<CalendarTab today={now} />);
    await user.click(await screen.findByRole("button", { name: "Change “Dentist”" }));
    const form = screen.getByRole("form", { name: "Change event" });
    await user.clear(within(form).getByLabelText("What"));
    await user.type(within(form).getByLabelText("What"), "Doctor");
    await user.click(within(form).getByRole("button", { name: "Save" }));
    expect(ipc.calendarSave).toHaveBeenCalledWith(4, expect.objectContaining({ title: "Doctor" }));
  });

  it("asks once more before deleting", async () => {
    vi.mocked(ipc.calendarBetween).mockResolvedValue([item({ id: 5 })]);
    vi.mocked(ipc.calendarDelete).mockResolvedValue();
    const user = userEvent.setup();
    render(<CalendarTab today={now} />);
    await user.click(await screen.findByRole("button", { name: "Delete “Dentist”" }));
    expect(ipc.calendarDelete).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "Keep" }));
    await user.click(screen.getByRole("button", { name: "Delete “Dentist”" }));
    await user.click(screen.getByRole("button", { name: "Delete" }));
    expect(ipc.calendarDelete).toHaveBeenCalledWith(5);
  });

  it("says how a repeating event repeats, and deletes one day or every time", async () => {
    vi.mocked(ipc.calendarBetween).mockResolvedValue([
      item({ id: 6, title: "Gym", start_date: "2026-10-02", repeat: "weekly", remind_before: null }),
    ]);
    vi.mocked(ipc.calendarDelete).mockResolvedValue();
    const user = userEvent.setup();
    render(<CalendarTab today={now} />);
    expect(await screen.findByText("Gym")).toBeInTheDocument();
    expect(screen.getByRole("list", { name: /Plans for/ })).toHaveTextContent("15:00 · Every week");
    vi.mocked(ipc.calendarSkipDay).mockResolvedValue();
    await user.click(screen.getByRole("button", { name: "Delete “Gym”" }));
    await user.click(screen.getByRole("button", { name: "This day" }));
    expect(ipc.calendarSkipDay).toHaveBeenCalledWith(6, "2026-10-30");
    expect(ipc.calendarDelete).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "Every time" }));
    expect(ipc.calendarDelete).toHaveBeenCalledWith(6);
  });

  it("asks how often it repeats, and calls the date where it starts", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.calendarSave).mockResolvedValue(item({ repeat: "monthly" }));
    render(<CalendarTab today={now} />);
    await user.click(screen.getByRole("button", { name: "Add" }));
    const form = screen.getByRole("form", { name: "New event" });
    expect(within(form).getByLabelText("Repeats")).toHaveValue("");
    await user.selectOptions(within(form).getByLabelText("Repeats"), "Every month");
    expect(within(form).getByLabelText("Starts")).toHaveValue("2026-10-30");
    await user.type(within(form).getByLabelText("What"), "Rent");
    await user.click(within(form).getByRole("button", { name: "Save event" }));
    expect(ipc.calendarSave).toHaveBeenCalledWith(null, expect.objectContaining({ title: "Rent", repeat: "monthly" }));
  });

  it("keeps an end time and warns about a clash without stopping the save", async () => {
    const lunch = item({ id: 9, title: "Team lunch", time: "12:30", end_time: "13:30" });
    vi.mocked(ipc.calendarBetween).mockResolvedValue([lunch]);
    vi.mocked(ipc.calendarSave).mockResolvedValue(item({ id: 10, title: "Call" }));
    const user = userEvent.setup();
    render(<CalendarTab today={now} />);
    await screen.findByText("Team lunch");
    expect(screen.getByRole("list", { name: /Plans for/ })).toHaveTextContent("12:30–13:30");
    await user.click(screen.getByRole("button", { name: "Add" }));
    const form = screen.getByRole("form", { name: "New event" });
    await user.click(within(form).getByRole("checkbox", { name: "All day" }));
    await user.type(within(form).getByLabelText("What"), "Call");
    await user.type(within(form).getByLabelText("Starts at"), "13:00");
    await user.type(within(form).getByLabelText("Ends at"), "13:45");
    expect(await within(form).findByRole("status")).toHaveTextContent("Overlaps “Team lunch” (12:30–13:30).");
    await user.click(within(form).getByRole("button", { name: "Save event" }));
    expect(ipc.calendarSave).toHaveBeenCalledWith(null, expect.objectContaining({ time: "13:00", end_time: "13:45" }));
  });

  it("changes one day of a series on its own, or every time", async () => {
    const gym = item({ id: 6, title: "Gym", date: "2026-10-30", start_date: "2026-10-02", repeat: "weekly" });
    vi.mocked(ipc.calendarBetween).mockResolvedValue([gym]);
    vi.mocked(ipc.calendarChangeDay).mockResolvedValue(item({ id: 11, title: "Gym" }));
    vi.mocked(ipc.calendarSave).mockResolvedValue(gym);
    const user = userEvent.setup();
    render(<CalendarTab today={now} />);
    await user.click(await screen.findByRole("button", { name: "Change “Gym”" }));
    let form = screen.getByRole("form", { name: "Change event" });
    expect(within(form).getByRole("radio", { name: "Only Friday 30 October" })).toBeChecked();
    expect(within(form).getByLabelText("Date")).toHaveValue("2026-10-30");
    expect(within(form).queryByLabelText("Repeats")).not.toBeInTheDocument();
    await user.click(within(form).getByRole("button", { name: "Save" }));
    expect(ipc.calendarChangeDay).toHaveBeenCalledWith(6, "2026-10-30", expect.objectContaining({ repeat: null }));
    expect(ipc.calendarSave).not.toHaveBeenCalled();

    await user.click(await screen.findByRole("button", { name: "Change “Gym”" }));
    form = screen.getByRole("form", { name: "Change event" });
    await user.click(within(form).getByRole("radio", { name: "Every time" }));
    expect(within(form).getByLabelText("Starts")).toHaveValue("2026-10-02");
    expect(within(form).getByLabelText("Repeats")).toHaveValue("weekly");
    await user.click(within(form).getByRole("button", { name: "Save" }));
    expect(ipc.calendarSave).toHaveBeenCalledWith(6, expect.objectContaining({ date: "2026-10-02", repeat: "weekly" }));
  });

  it("loads again when the calendar changes in another window", async () => {
    render(<CalendarTab today={now} />);
    await waitFor(() => expect(changed).toBeDefined());
    vi.mocked(ipc.calendarBetween).mockResolvedValue([item({ title: "From the card" })]);
    act(() => changed!());
    expect(await screen.findByText("From the card")).toBeInTheDocument();
  });
});
