import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Settings } from "../lib/types";

vi.mock("../lib/ipc");

import * as ipc from "../lib/ipc";
import { Customize } from "./Customize";

const baseSettings: Settings = {
  provider: "ollama",
  models: {},
  ollama_base_url: "http://localhost:11434",
  mouce_size: "medium",
  mouce_rotation: true,
  mouce_position: null,
  circle_hotkey: "Ctrl+Alt+Space",
  chat_hotkey: "Ctrl+Alt+M",
  history_enabled: false,
  fallback_to_local: true,
  first_run_done: true,
};

describe("Customize", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    vi.mocked(ipc.getSettings).mockResolvedValue(baseSettings);
    vi.mocked(ipc.updateSettings).mockImplementation(async (s) => s);
    vi.mocked(ipc.onSettingsChanged).mockResolvedValue(() => {});
  });

  it("changes the size", async () => {
    const user = userEvent.setup();
    render(<Customize />);
    await user.selectOptions(await screen.findByLabelText("Size"), "large");
    await waitFor(() =>
      expect(ipc.updateSettings).toHaveBeenCalledWith(expect.objectContaining({ mouce_size: "large" })),
    );
  });

  it("turns sway off", async () => {
    const user = userEvent.setup();
    render(<Customize />);
    const sway = await screen.findByRole("checkbox", { name: "Sway" });
    expect(sway).toBeChecked();
    await user.click(sway);
    await waitFor(() =>
      expect(ipc.updateSettings).toHaveBeenCalledWith(expect.objectContaining({ mouce_rotation: false })),
    );
    await waitFor(() => expect(sway).not.toBeChecked());
  });
});
