import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Settings } from "../lib/types";

vi.mock("../lib/ipc");

import * as ipc from "../lib/ipc";
import { Shortcuts } from "./Shortcuts";

const baseSettings: Settings = {
  provider: "ollama",
  models: {},
  ollama_base_url: "http://localhost:11434",
  mouce_size: "medium",
  mouce_rotation: true,
  mouce_position: null,
  circle_hotkey: "Ctrl+Alt+O",
  chat_hotkey: "Ctrl+Alt+M",
  history_enabled: false,
  fallback_to_local: true,
  first_run_done: true,
  liquid_glass: true,
  dictionary_enabled: true,
  calendar_in_chat: true,
  quiet_hours: false,
  quiet_from: "22:00",
  quiet_to: "07:00",
  wait_for_full_screen: true,
};

/** The keys shown on the row labelled `label`, or "Off". */
function keysOf(label: string): string {
  const row = screen.getByText(label).closest(".form-row") as HTMLElement;
  const keys = within(row).queryAllByText((_, el) => el?.tagName === "KBD").map((k) => k.textContent);
  return keys.length ? keys.join("+") : "Off";
}

describe("Shortcuts", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    vi.mocked(ipc.onSettingsChanged).mockResolvedValue(() => {});
  });

  it("lists the shortcuts that work in any app, from the saved settings", async () => {
    vi.mocked(ipc.getSettings).mockResolvedValue(baseSettings);
    render(<Shortcuts onNavigate={() => {}} />);
    await screen.findByText("In any app");
    expect(keysOf("Open the chat")).toBe("Ctrl+Alt+M");
    expect(keysOf("Circle to look up")).toBe("Ctrl+Alt+O");
    expect(keysOf("Look up a word")).toBe("Ctrl+Right-click");
    expect(keysOf("Explain a phrase")).toBe("Ctrl+Right-click");
    expect(keysOf("New line")).toBe("Shift+Enter");
  });

  it("says Off for a shortcut that is turned off, and for the dictionary when it is off", async () => {
    vi.mocked(ipc.getSettings).mockResolvedValue({ ...baseSettings, circle_hotkey: "", dictionary_enabled: false });
    render(<Shortcuts onNavigate={() => {}} />);
    await screen.findByText("In any app");
    expect(keysOf("Circle to look up")).toBe("Off");
    expect(keysOf("Look up a word")).toBe("Off");
    expect(keysOf("Open the chat")).toBe("Ctrl+Alt+M");
  });

  it("goes to Settings to change them", async () => {
    vi.mocked(ipc.getSettings).mockResolvedValue(baseSettings);
    const onNavigate = vi.fn();
    render(<Shortcuts onNavigate={onNavigate} />);
    await userEvent.click(await screen.findByRole("button", { name: "Settings" }));
    expect(onNavigate).toHaveBeenCalledWith("settings");
  });
});
