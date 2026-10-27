import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { OllamaStatus, PullProgress, Settings } from "../lib/types";

vi.mock("../lib/ipc");

import * as ipc from "../lib/ipc";
import { FirstRun } from "./FirstRun";

const baseSettings: Settings = {
  provider: "ollama",
  models: {},
  ollama_base_url: "http://localhost:11434",
  mouce_size: "medium",
  mouce_rotation: true,
  mouce_position: null,
  circle_hotkey: "Ctrl+Alt+Space",
  history_enabled: false,
  fallback_to_local: true,
  first_run_done: false,
};

let progressCb: ((p: PullProgress) => void) | undefined;

function setStatus(status: OllamaStatus) {
  vi.mocked(ipc.ollamaStatus).mockResolvedValue(status);
}

async function toStepTwo(user: ReturnType<typeof userEvent.setup>) {
  await user.click(await screen.findByRole("button", { name: "Next" }));
  await screen.findByRole("heading", { name: "Choose your AI" });
}

describe("FirstRun", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    progressCb = undefined;
    vi.mocked(ipc.getSettings).mockResolvedValue(baseSettings);
    vi.mocked(ipc.updateSettings).mockImplementation(async (s) => s);
    vi.mocked(ipc.onSettingsChanged).mockResolvedValue(() => {});
    vi.mocked(ipc.onPullProgress).mockImplementation(async (cb) => {
      progressCb = cb;
      return () => {};
    });
    vi.mocked(ipc.hasApiKey).mockResolvedValue(false);
    vi.mocked(ipc.listModels).mockResolvedValue([]);
    vi.mocked(ipc.openOllamaDownload).mockResolvedValue(undefined);
    vi.mocked(ipc.setAutostart).mockResolvedValue(undefined);
    vi.mocked(ipc.finishFirstRun).mockResolvedValue(undefined);
  });

  it("not installed offers download and online", async () => {
    const user = userEvent.setup();
    setStatus({ state: "notInstalled" });
    render(<FirstRun />);
    await toStepTwo(user);

    const get = await screen.findByRole("button", { name: "Get Ollama (free, runs on this laptop)" });
    expect(screen.getByRole("button", { name: "Use an online AI" })).toBeInTheDocument();
    await user.click(get);
    expect(ipc.openOllamaDownload).toHaveBeenCalledTimes(1);
  });

  it("not installed can check again after Ollama is installed", async () => {
    const user = userEvent.setup();
    setStatus({ state: "notInstalled" });
    render(<FirstRun />);
    await toStepTwo(user);

    const check = await screen.findByRole("button", { name: "Check again" });
    expect(screen.getByRole("button", { name: "Get Ollama (free, runs on this laptop)" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Use an online AI" })).toBeInTheDocument();

    setStatus({ state: "running", models: [] });
    await user.click(check);
    expect(await screen.findByRole("button", { name: "Download gemma3:4b (about 3 GB)" })).toBeEnabled();
    expect(ipc.ollamaStatus).toHaveBeenCalledTimes(2);
  });

  it("not running asks to start it and can check again", async () => {
    const user = userEvent.setup();
    setStatus({ state: "notRunning" });
    render(<FirstRun />);
    await toStepTwo(user);

    await screen.findByText(
      "Ollama is installed but not running — start it from the Start menu, then press Check again",
    );
    setStatus({ state: "running", models: ["gemma3:4b"] });
    await user.click(screen.getByRole("button", { name: "Check again" }));
    await screen.findByText(/ready/i);
    expect(ipc.ollamaStatus).toHaveBeenCalledTimes(2);
  });

  it("running without model offers gemma download", async () => {
    const user = userEvent.setup();
    setStatus({ state: "running", models: [] });
    render(<FirstRun />);
    await toStepTwo(user);

    expect(await screen.findByRole("button", { name: "Download gemma3:4b (about 3 GB)" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Skip" })).toBeInTheDocument();
  });

  it("running with model skips download", async () => {
    const user = userEvent.setup();
    setStatus({ state: "running", models: ["gemma3:4b"] });
    render(<FirstRun />);
    await toStepTwo(user);

    await screen.findByText(/ready/i);
    expect(screen.queryByRole("button", { name: /Download gemma3:4b/ })).not.toBeInTheDocument();
  });

  it("use an online AI reveals the provider and key fields", async () => {
    const user = userEvent.setup();
    setStatus({ state: "notInstalled" });
    render(<FirstRun />);
    await toStepTwo(user);

    await user.click(await screen.findByRole("button", { name: "Use an online AI" }));
    await user.selectOptions(await screen.findByLabelText("Provider"), "Claude");
    expect(await screen.findByLabelText("API key")).toBeInTheDocument();
    await screen.findByText("No key yet");
  });

  it("disables download while pulling and shows progress", async () => {
    const user = userEvent.setup();
    setStatus({ state: "running", models: [] });
    let finish: () => void = () => {};
    vi.mocked(ipc.pullOllamaModel).mockImplementation(
      () => new Promise<void>((resolve) => (finish = resolve)),
    );
    render(<FirstRun />);
    await toStepTwo(user);

    const button = await screen.findByRole("button", { name: "Download gemma3:4b (about 3 GB)" });
    await user.click(button);
    expect(ipc.pullOllamaModel).toHaveBeenCalledWith("gemma3:4b");
    expect(button).toBeDisabled();

    await waitFor(() => expect(progressCb).toBeDefined());
    act(() => progressCb?.({ status: "pulling", completed: 50, total: 100 }));
    expect(screen.getByText("50%")).toBeInTheDocument();
    act(() => progressCb?.({ status: "pulling", completed: 75, total: 100 }));
    expect(screen.getByText("75%")).toBeInTheDocument();
    expect(button).toBeDisabled();

    setStatus({ state: "running", models: ["gemma3:4b"] });
    await act(async () => finish());
    await screen.findByText(/ready/i);
  });

  it("shows a download error verbatim and lets you retry", async () => {
    const user = userEvent.setup();
    setStatus({ state: "running", models: [] });
    vi.mocked(ipc.pullOllamaModel).mockRejectedValue("A download is already in progress");
    render(<FirstRun />);
    await toStepTwo(user);

    await user.click(await screen.findByRole("button", { name: "Download gemma3:4b (about 3 GB)" }));
    expect(await screen.findByText("A download is already in progress")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Download gemma3:4b (about 3 GB)" })).toBeEnabled();
  });

  it("done applies autostart and finishes", async () => {
    const user = userEvent.setup();
    setStatus({ state: "running", models: ["gemma3:4b"] });
    const order: string[] = [];
    vi.mocked(ipc.setAutostart).mockImplementation(async (enabled) => {
      order.push(`autostart:${enabled}`);
    });
    vi.mocked(ipc.finishFirstRun).mockImplementation(async () => {
      order.push("finish");
    });
    render(<FirstRun />);

    expect(await screen.findByText("Hi, I'm mouce.")).toBeInTheDocument();
    expect(screen.getByRole("checkbox", { name: "Start mouce when Windows starts" })).toBeChecked();
    await toStepTwo(user);
    await screen.findByText(/ready/i);
    await user.click(screen.getByRole("button", { name: "Next" }));
    await screen.findByText("All set");
    await user.click(screen.getByRole("button", { name: "Done" }));

    await waitFor(() => expect(ipc.finishFirstRun).toHaveBeenCalledTimes(1));
    expect(ipc.setAutostart).toHaveBeenCalledWith(true);
    expect(order).toEqual(["autostart:true", "finish"]);
  });

  it("unchecking the startup box turns autostart off", async () => {
    const user = userEvent.setup();
    setStatus({ state: "running", models: ["gemma3:4b"] });
    render(<FirstRun />);
    await user.click(await screen.findByRole("checkbox", { name: "Start mouce when Windows starts" }));
    await toStepTwo(user);
    await screen.findByText(/ready/i);
    await user.click(screen.getByRole("button", { name: "Next" }));
    await user.click(await screen.findByRole("button", { name: "Done" }));
    await waitFor(() => expect(ipc.finishFirstRun).toHaveBeenCalled());
    expect(ipc.setAutostart).toHaveBeenCalledWith(false);
  });

  it("a failed autostart change is shown and a second Done finishes without it", async () => {
    const user = userEvent.setup();
    setStatus({ state: "running", models: ["gemma3:4b"] });
    vi.mocked(ipc.setAutostart).mockRejectedValue("Access is denied");
    render(<FirstRun />);
    await toStepTwo(user);
    await screen.findByText(/ready/i);
    await user.click(screen.getByRole("button", { name: "Next" }));
    await user.click(await screen.findByRole("button", { name: "Done" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("Access is denied");
    expect(ipc.finishFirstRun).not.toHaveBeenCalled();

    await user.click(screen.getByRole("button", { name: "Done" }));
    await waitFor(() => expect(ipc.finishFirstRun).toHaveBeenCalledTimes(1));
    expect(ipc.setAutostart).toHaveBeenCalledTimes(1);
  });

  it("cannot leave step two while a download runs", async () => {
    const user = userEvent.setup();
    setStatus({ state: "running", models: [] });
    let finish: () => void = () => {};
    vi.mocked(ipc.pullOllamaModel).mockImplementation(() => new Promise<void>((r) => (finish = r)));
    render(<FirstRun />);
    await toStepTwo(user);

    await user.click(await screen.findByRole("button", { name: "Download gemma3:4b (about 3 GB)" }));
    expect(screen.getByRole("button", { name: "Skip" })).toBeDisabled();

    setStatus({ state: "running", models: ["gemma3:4b"] });
    await act(async () => finish());
    expect(await screen.findByRole("button", { name: "Next" })).toBeEnabled();
  });

  it("a failed re-check after a successful download still shows success", async () => {
    const user = userEvent.setup();
    const logged = vi.spyOn(console, "error").mockImplementation(() => {});
    vi.mocked(ipc.ollamaStatus)
      .mockResolvedValueOnce({ state: "running", models: [] })
      .mockRejectedValueOnce("status check failed");
    vi.mocked(ipc.pullOllamaModel).mockResolvedValue(undefined);
    render(<FirstRun />);
    await toStepTwo(user);

    await user.click(await screen.findByRole("button", { name: "Download gemma3:4b (about 3 GB)" }));
    await screen.findByText("gemma3:4b is ready.");
    await waitFor(() => expect(logged).toHaveBeenCalled());
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Download gemma3:4b/ })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Next" })).toBeEnabled();
    logged.mockRestore();
  });
});
