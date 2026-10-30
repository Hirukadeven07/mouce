import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { CalendarItem } from "../lib/types";

vi.mock("../lib/ipc");

import * as ipc from "../lib/ipc";
import { AlertCard, whenText } from "./AlertCard";

const now = new Date(2026, 9, 30, 14, 50);

function item(over: Partial<CalendarItem>): CalendarItem {
  return {
    id: 1,
    kind: "event",
    title: "Dentist",
    date: "2026-10-30",
    start_date: "2026-10-30",
    time: "15:00",
    remind_before: 10,
    repeat: null,
    alert_at: now.getTime(),
    done: false,
    ...over,
  };
}

describe("whenText", () => {
  it("says how soon an event starts", () => {
    expect(whenText(item({}), now)).toBe("Starts in 10 minutes · 15:00");
    expect(whenText(item({ time: "14:51" }), now)).toBe("Starting now · 14:51");
    expect(whenText(item({ time: "14:50" }), now)).toBe("Starting now · 14:50");
    expect(whenText(item({ time: "14:30" }), now)).toBe("Started at 14:30");
    expect(whenText(item({ date: "2026-10-31", time: "15:00" }), now)).toBe("Starts tomorrow at 15:00");
    expect(whenText(item({ time: null }), now)).toBe("Today, all day");
    expect(whenText(item({ date: "2026-10-31", time: null }), now)).toBe("Tomorrow, all day");
  });

  it("says when a reminder was due", () => {
    expect(whenText(item({ kind: "reminder", time: "14:50" }), now)).toBe("Due now");
    expect(whenText(item({ kind: "reminder", time: "09:00" }), now)).toBe("Was due at 09:00");
    expect(whenText(item({ kind: "reminder", date: "2026-10-29", time: "09:00" }), now)).toBe(
      "Was due yesterday at 09:00",
    );
    expect(whenText(item({ kind: "reminder", date: "2026-10-26", time: "09:00" }), now)).toBe(
      "Was due on Mon 26 Oct at 09:00",
    );
  });
});

describe("AlertCard", () => {
  let alertsCb: ((due: CalendarItem[]) => void) | undefined;

  beforeEach(() => {
    vi.resetAllMocks();
    alertsCb = undefined;
    vi.mocked(ipc.alertsCurrent).mockResolvedValue([]);
    vi.mocked(ipc.alertsFit).mockResolvedValue();
    vi.mocked(ipc.alertsAct).mockResolvedValue();
    vi.mocked(ipc.onAlerts).mockImplementation(async (cb) => {
      alertsCb = cb;
      return () => {};
    });
  });

  it("shows what was due before the page loaded and asks to be shown", async () => {
    vi.mocked(ipc.alertsCurrent).mockResolvedValue([item({})]);
    render(<AlertCard />);
    expect(await screen.findByRole("heading", { name: "Reminder" })).toBeInTheDocument();
    expect(screen.getByText("Dentist")).toBeInTheDocument();
    await waitFor(() => expect(ipc.alertsFit).toHaveBeenCalled());
  });

  it("shows nothing and stays hidden when nothing is due", async () => {
    const { container } = render(<AlertCard />);
    await waitFor(() => expect(alertsCb).toBeDefined());
    expect(container).toBeEmptyDOMElement();
    expect(ipc.alertsFit).not.toHaveBeenCalled();
  });

  it("snoozes for as long as chosen, puts away an event, and marks a reminder done", async () => {
    const user = userEvent.setup();
    render(<AlertCard />);
    await waitFor(() => expect(alertsCb).toBeDefined());
    act(() => alertsCb!([item({ id: 1 }), item({ id: 2, kind: "reminder", title: "Call mum", time: "14:50" })]));
    expect(screen.getByRole("heading", { name: "2 reminders" })).toBeInTheDocument();
    await user.click(screen.getAllByRole("button", { name: "Snooze…" })[0]);
    const choices = screen.getByRole("group", { name: "Snooze “Dentist” for" });
    await user.click(within(choices).getByRole("button", { name: "Back" }));
    expect(screen.queryByRole("group")).not.toBeInTheDocument();
    await user.click(screen.getAllByRole("button", { name: "Snooze…" })[0]);
    await user.click(screen.getByRole("button", { name: "1 hour" }));
    expect(ipc.alertsAct).toHaveBeenCalledWith(1, "snooze_hour");
    await user.click(screen.getByRole("button", { name: "Tomorrow" }));
    expect(ipc.alertsAct).toHaveBeenCalledWith(1, "snooze_tomorrow");
    await user.click(screen.getByRole("button", { name: "10 min" }));
    expect(ipc.alertsAct).toHaveBeenCalledWith(1, "snooze");
    // In the app a snoozed item leaves the card; here it stays, still showing the choices.
    await user.click(screen.getByRole("button", { name: "Back" }));
    await user.click(screen.getByRole("button", { name: "Got it" }));
    expect(ipc.alertsAct).toHaveBeenCalledWith(1, "dismiss");
    await user.click(screen.getByRole("button", { name: "Done" }));
    expect(ipc.alertsAct).toHaveBeenCalledWith(2, "done");
  });

  it("opens the item in mouce from its title", async () => {
    vi.mocked(ipc.alertsCurrent).mockResolvedValue([item({ id: 4 })]);
    vi.mocked(ipc.alertsOpen).mockResolvedValue();
    const user = userEvent.setup();
    render(<AlertCard />);
    await user.click(await screen.findByRole("button", { name: "Dentist" }));
    expect(ipc.alertsOpen).toHaveBeenCalledWith(4);
  });

  it("says when an action failed", async () => {
    vi.mocked(ipc.alertsCurrent).mockResolvedValue([item({})]);
    vi.mocked(ipc.alertsAct).mockRejectedValue("database error: locked");
    const user = userEvent.setup();
    render(<AlertCard />);
    await user.click(await screen.findByRole("button", { name: "Got it" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("database error: locked");
  });
});
