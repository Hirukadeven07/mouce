import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Settings } from "../lib/types";

vi.mock("../lib/ipc");

import * as ipc from "../lib/ipc";
import { Settings as SettingsScreen } from "./Settings";

const baseSettings: Settings = {
  provider: "ollama",
  models: {},
  ollama_base_url: "http://localhost:11434",
  star_size: "medium",
  star_rotation: true,
  star_position: null,
  circle_hotkey: "Ctrl+Alt+Space",
  history_enabled: false,
  fallback_to_local: true,
  first_run_done: true,
};

async function renderSettings() {
  render(<SettingsScreen />);
  await screen.findByLabelText("Provider");
  // Let the startup checks (key status, model list, autostart) settle.
  await waitFor(() => expect(ipc.getAutostart).toHaveBeenCalled());
  await screen.findByRole("checkbox", { name: "Start Star when Windows starts" });
}

describe("Settings", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    vi.mocked(ipc.getSettings).mockResolvedValue(baseSettings);
    vi.mocked(ipc.updateSettings).mockImplementation(async (s) => s);
    vi.mocked(ipc.onSettingsChanged).mockResolvedValue(() => {});
    vi.mocked(ipc.hasApiKey).mockResolvedValue(false);
    vi.mocked(ipc.listModels).mockResolvedValue([]);
    vi.mocked(ipc.getAutostart).mockResolvedValue(false);
    vi.mocked(ipc.setApiKey).mockResolvedValue(undefined);
  });

  it("lists five providers", async () => {
    await renderSettings();
    const options = within(screen.getByLabelText("Provider")).getAllByRole("option");
    expect(options.map((o) => o.textContent)).toEqual([
      "Local (Ollama)",
      "OpenAI",
      "Groq",
      "Gemini",
      "Claude",
    ]);
  });

  it("saving a key trims and calls setApiKey", async () => {
    const user = userEvent.setup();
    await renderSettings();
    await user.selectOptions(screen.getByLabelText("Provider"), "Claude");
    await screen.findByText("No key yet");

    const input = screen.getByLabelText("API key") as HTMLInputElement;
    expect(input).toHaveAttribute("type", "password");
    await user.type(input, "  sk-ant-x  ");
    await user.click(screen.getByRole("button", { name: "Save key" }));

    await screen.findByText("Key saved");
    expect(ipc.setApiKey).toHaveBeenCalledWith("anthropic", "sk-ant-x");
    expect(input.value).toBe("");
  });

  it("never displays a stored key", async () => {
    vi.mocked(ipc.getSettings).mockResolvedValue({ ...baseSettings, provider: "openai" });
    vi.mocked(ipc.hasApiKey).mockResolvedValue(true);
    await renderSettings();
    await screen.findByText("Key saved");
    const fields = document.querySelectorAll<HTMLInputElement>(
      "input:not([type=checkbox]):not([type=radio])",
    );
    expect(fields.length).toBeGreaterThan(0);
    fields.forEach((f) => expect(f.value).toBe(""));
  });

  it("test connection shows error text", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.testProvider).mockRejectedValue("HTTP 401: invalid x-api-key");
    await renderSettings();
    await user.click(screen.getByRole("button", { name: "Test connection" }));
    expect(await screen.findByText("HTTP 401: invalid x-api-key")).toBeInTheDocument();
    expect(ipc.testProvider).toHaveBeenCalledWith("ollama");
  });

  it("toggling rotate calls updateSettings with star_rotation false", async () => {
    const user = userEvent.setup();
    await renderSettings();
    await user.click(screen.getByRole("checkbox", { name: "Rotate" }));
    await waitFor(() =>
      expect(ipc.updateSettings).toHaveBeenCalledWith(
        expect.objectContaining({ star_rotation: false }),
      ),
    );
    await waitFor(() => expect(screen.getByRole("checkbox", { name: "Rotate" })).not.toBeChecked());
  });

  it("shows a Choose a model placeholder for a provider with no model yet", async () => {
    vi.mocked(ipc.getSettings).mockResolvedValue({ ...baseSettings, provider: "groq" });
    vi.mocked(ipc.hasApiKey).mockResolvedValue(true);
    vi.mocked(ipc.listModels).mockResolvedValue(["llama-a", "llama-b"]);
    await renderSettings();
    await screen.findByRole("option", { name: "llama-a" });
    const select = screen.getByLabelText("Model") as HTMLSelectElement;
    expect(select.value).toBe("");
    expect(within(select).getByRole("option", { name: "Choose a model" })).toBeInTheDocument();
  });

  it("defaults the Ollama model to gemma3:4b", async () => {
    await renderSettings();
    expect((screen.getByLabelText("Model") as HTMLSelectElement).value).toBe("gemma3:4b");
  });
});
