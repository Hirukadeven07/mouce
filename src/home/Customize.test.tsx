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
  liquid_glass: true,
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
    const sway = await screen.findByRole("switch", { name: "Sway" });
    expect(sway).toBeChecked();
    await user.click(sway);
    await waitFor(() =>
      expect(ipc.updateSettings).toHaveBeenCalledWith(expect.objectContaining({ mouce_rotation: false })),
    );
    await waitFor(() => expect(sway).not.toBeChecked());
  });

  it("turns Liquid Glass off", async () => {
    const user = userEvent.setup();
    render(<Customize />);
    const glass = await screen.findByRole("switch", { name: "Liquid Glass" });
    expect(glass).toBeChecked();
    await user.click(glass);
    await waitFor(() =>
      expect(ipc.updateSettings).toHaveBeenCalledWith(expect.objectContaining({ liquid_glass: false })),
    );
  });

  it("shows Liquid Glass as off while Windows has transparency effects off", async () => {
    const matchMedia = vi.fn((media: string) => ({
      matches: media === "(prefers-reduced-transparency: reduce)",
      media,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
    }));
    vi.stubGlobal("matchMedia", matchMedia);
    try {
      render(<Customize />);
      const glass = await screen.findByRole("switch", { name: "Liquid Glass" });
      expect(glass).not.toBeChecked();
      expect(glass).toBeDisabled();
      expect(glass).toHaveAccessibleDescription(/Transparency effects are off in Windows settings/);
    } finally {
      vi.unstubAllGlobals();
    }
  });
});
