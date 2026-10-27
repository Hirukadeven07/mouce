import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { PullProgress, Settings } from "../lib/types";

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

let progressCb: ((p: PullProgress) => void) | undefined;

async function renderSettings() {
  render(<SettingsScreen />);
  await screen.findByLabelText("Provider");
  // Let the startup checks (key status, model list, autostart) settle.
  await waitFor(() => expect(ipc.getAutostart).toHaveBeenCalled());
  await screen.findByRole("checkbox", { name: "Start Mouce when Windows starts" });
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
    vi.mocked(ipc.clearApiKey).mockResolvedValue(undefined);
    vi.mocked(ipc.ollamaStatus).mockResolvedValue({ state: "running", models: [] });
    vi.mocked(ipc.openOllamaDownload).mockResolvedValue(undefined);
    progressCb = undefined;
    vi.mocked(ipc.onPullProgress).mockImplementation(async (cb) => {
      progressCb = cb;
      return () => {};
    });
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
    await user.click(screen.getByRole("checkbox", { name: "Sway" }));
    await waitFor(() =>
      expect(ipc.updateSettings).toHaveBeenCalledWith(
        expect.objectContaining({ star_rotation: false }),
      ),
    );
    await waitFor(() => expect(screen.getByRole("checkbox", { name: "Sway" })).not.toBeChecked());
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

  it("a slow model list for the old provider never fills the new provider", async () => {
    const user = userEvent.setup();
    let resolveOld: (m: string[]) => void = () => {};
    vi.mocked(ipc.listModels).mockImplementation(
      () => new Promise<string[]>((resolve) => (resolveOld = resolve)),
    );
    await renderSettings();
    await user.selectOptions(screen.getByLabelText("Provider"), "Claude");
    await screen.findByText("No key yet");

    await act(async () => resolveOld(["gemma3:4b", "llama3"]));

    const select = screen.getByLabelText("Model") as HTMLSelectElement;
    expect(within(select).getAllByRole("option").map((o) => o.textContent)).toEqual(["Choose a model"]);
    expect(screen.getByRole("button", { name: "Refresh" })).toBeDisabled();
  });

  it("explains that Ollama is not running when the model list fails", async () => {
    vi.mocked(ipc.listModels).mockRejectedValue("connection refused");
    vi.mocked(ipc.ollamaStatus).mockResolvedValue({ state: "notRunning" });
    await renderSettings();
    expect(
      await screen.findByText("Ollama isn't running — start it from the Start menu, then press Refresh"),
    ).toBeInTheDocument();
    expect(screen.queryByText("connection refused")).not.toBeInTheDocument();
  });

  it("explains that Ollama is not installed and offers the download", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.testProvider).mockRejectedValue("connection refused");
    vi.mocked(ipc.ollamaStatus).mockResolvedValue({ state: "notInstalled" });
    await renderSettings();
    await user.click(screen.getByRole("button", { name: "Test connection" }));
    await screen.findByText("Ollama isn't installed yet");
    await user.click(screen.getByRole("button", { name: "Get Ollama (free, runs on this laptop)" }));
    expect(ipc.openOllamaDownload).toHaveBeenCalledTimes(1);
  });

  it("shows the original Ollama error when Ollama is running", async () => {
    vi.mocked(ipc.listModels).mockRejectedValue("something odd");
    await renderSettings();
    expect(await screen.findByText("something odd")).toBeInTheDocument();
  });

  it("an older settings-changed event cannot flip an optimistic control back", async () => {
    const user = userEvent.setup();
    const listeners: Array<(s: Settings) => void> = [];
    vi.mocked(ipc.onSettingsChanged).mockImplementation(async (cb) => {
      listeners.push(cb);
      return () => {};
    });
    let finish: (s: Settings) => void = () => {};
    vi.mocked(ipc.updateSettings).mockImplementation(
      () => new Promise<Settings>((resolve) => (finish = resolve)),
    );
    await renderSettings();
    await user.click(screen.getByRole("checkbox", { name: "Sway" }));
    expect(screen.getByRole("checkbox", { name: "Sway" })).not.toBeChecked();

    expect(listeners.length).toBeGreaterThan(0);
    act(() => listeners.forEach((cb) => cb({ ...baseSettings, star_rotation: true })));
    expect(screen.getByRole("checkbox", { name: "Sway" })).not.toBeChecked();

    await act(async () => finish({ ...baseSettings, star_rotation: false }));
    expect(screen.getByRole("checkbox", { name: "Sway" })).not.toBeChecked();
  });

  it("shows the privacy sentence", async () => {
    await renderSettings();
    expect(
      screen.getByText(
        "Screen captures are only sent to the AI you pick above. Mouce cannot open, change, or delete your files.",
      ),
    ).toBeInTheDocument();
  });

  it("disables the startup checkbox while a change is pending", async () => {
    const user = userEvent.setup();
    let finish: () => void = () => {};
    vi.mocked(ipc.setAutostart).mockImplementation(() => new Promise<void>((r) => (finish = r)));
    await renderSettings();
    const box = screen.getByRole("checkbox", { name: "Start Mouce when Windows starts" });
    await user.click(box);
    expect(box).toBeDisabled();
    expect(box).toBeChecked();
    await act(async () => finish());
    expect(box).toBeEnabled();
    expect(box).toBeChecked();
    expect(ipc.setAutostart).toHaveBeenCalledTimes(1);
  });

  it("reverts the startup checkbox to the real value when the change fails", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.getAutostart).mockReset();
    vi.mocked(ipc.getAutostart).mockResolvedValueOnce(false).mockResolvedValueOnce(true);
    vi.mocked(ipc.setAutostart).mockRejectedValue("Access is denied");
    await renderSettings();
    const box = screen.getByRole("checkbox", { name: "Start Mouce when Windows starts" });
    await user.click(box);
    expect(await screen.findByText("Access is denied")).toBeInTheDocument();
    await waitFor(() => expect(box).toBeEnabled());
    expect(box).toBeChecked();
    expect(ipc.getAutostart).toHaveBeenCalledTimes(2);
  });

  it("removing the key clears the connection test result", async () => {
    const user = userEvent.setup();
    vi.mocked(ipc.getSettings).mockResolvedValue({ ...baseSettings, provider: "openai" });
    vi.mocked(ipc.hasApiKey).mockResolvedValue(true);
    vi.mocked(ipc.testProvider).mockResolvedValue("ready");
    await renderSettings();
    await screen.findByText("Key saved");
    await user.click(screen.getByRole("button", { name: "Test connection" }));
    await screen.findByText("Connected: ready");
    await user.click(screen.getByRole("button", { name: "Remove key" }));
    await screen.findByText("No key yet");
    expect(screen.queryByText("Connected: ready")).not.toBeInTheDocument();
  });

  it("labels an Ollama model that is not installed", async () => {
    vi.mocked(ipc.listModels).mockResolvedValue(["llama3"]);
    await renderSettings();
    expect(await screen.findByRole("option", { name: "gemma3:4b (not downloaded)" })).toBeInTheDocument();
    expect(screen.getByRole("option", { name: "llama3" })).toBeInTheDocument();
  });

  it("does not label an installed Ollama model", async () => {
    vi.mocked(ipc.listModels).mockResolvedValue(["gemma3:4b"]);
    await renderSettings();
    await screen.findByRole("option", { name: "gemma3:4b" });
    expect(screen.queryByText(/not downloaded/)).not.toBeInTheDocument();
  });
  describe("downloading a local model", () => {
    const downloadName = "Download gemma3:4b";

    it("offers the download only for a not-downloaded Ollama model", async () => {
      vi.mocked(ipc.listModels).mockResolvedValue(["llama3"]);
      await renderSettings();
      expect(await screen.findByRole("button", { name: downloadName })).toBeEnabled();
    });

    it("does not offer it when the model is installed", async () => {
      vi.mocked(ipc.listModels).mockResolvedValue(["gemma3:4b"]);
      await renderSettings();
      await screen.findByRole("option", { name: "gemma3:4b" });
      expect(screen.queryByRole("button", { name: /^Download/ })).not.toBeInTheDocument();
    });

    it("does not offer it before the model list is known", async () => {
      vi.mocked(ipc.listModels).mockRejectedValue("Ollama is down");
      await renderSettings();
      await screen.findByText("Ollama is down");
      expect(screen.queryByRole("button", { name: /^Download/ })).not.toBeInTheDocument();
    });

    it("does not offer it for an online provider", async () => {
      vi.mocked(ipc.getSettings).mockResolvedValue({ ...baseSettings, provider: "openai" });
      vi.mocked(ipc.hasApiKey).mockResolvedValue(true);
      vi.mocked(ipc.listModels).mockResolvedValue(["gpt-4o"]);
      await renderSettings();
      await screen.findByRole("option", { name: "gpt-4o" });
      expect(screen.queryByRole("button", { name: /^Download/ })).not.toBeInTheDocument();
    });

    it("disables the button while pulling, shows progress, then reloads the models", async () => {
      const user = userEvent.setup();
      vi.mocked(ipc.listModels).mockResolvedValueOnce([]);
      let finish: () => void = () => {};
      vi.mocked(ipc.pullOllamaModel).mockImplementation(
        () => new Promise<void>((resolve) => (finish = resolve)),
      );
      await renderSettings();
      const button = await screen.findByRole("button", { name: downloadName });
      await user.click(button);
      expect(ipc.pullOllamaModel).toHaveBeenCalledWith("gemma3:4b");
      expect(button).toBeDisabled();

      await waitFor(() => expect(progressCb).toBeDefined());
      act(() => progressCb?.({ status: "pulling", completed: 25, total: 100 }));
      expect(screen.getByText("25%")).toBeInTheDocument();
      act(() => progressCb?.({ status: "verifying", completed: null, total: null }));
      expect(screen.getByText("verifying")).toBeInTheDocument();
      expect(screen.queryByText("25%")).not.toBeInTheDocument();

      const before = vi.mocked(ipc.listModels).mock.calls.length;
      vi.mocked(ipc.listModels).mockResolvedValue(["gemma3:4b"]);
      await act(async () => finish());
      await waitFor(() => expect(vi.mocked(ipc.listModels).mock.calls.length).toBeGreaterThan(before));
      await screen.findByRole("option", { name: "gemma3:4b" });
      expect(screen.queryByText(/not downloaded/)).not.toBeInTheDocument();
      expect(screen.queryByRole("button", { name: /^Download/ })).not.toBeInTheDocument();
    });

    it("shows a download error verbatim and lets you retry", async () => {
      const user = userEvent.setup();
      vi.mocked(ipc.pullOllamaModel).mockRejectedValue("A download is already in progress");
      await renderSettings();
      await user.click(await screen.findByRole("button", { name: downloadName }));
      expect(await screen.findByText("A download is already in progress")).toBeInTheDocument();
      expect(screen.getByRole("button", { name: downloadName })).toBeEnabled();
    });
  });
});
