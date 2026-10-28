import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Settings, MouceState } from "../lib/types";

vi.mock("../lib/ipc");

import * as ipc from "../lib/ipc";
import { Mouce, MouceWindow } from "./Mouce";

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
};

describe("Mouce", () => {
  it("renders state class", () => {
    const { container } = render(<Mouce state="thinking" rotate sizePx={36} />);
    expect(container.querySelector("img")).toHaveClass("mouce", "mouce--thinking");
  });

  it("still when rotation off", () => {
    const { container } = render(<Mouce state="idle" rotate={false} sizePx={36} />);
    expect(container.querySelector("img")).toHaveClass("mouce--still");
  });

  it("size applied", () => {
    const { container } = render(<Mouce state="idle" rotate sizePx={28} />);
    const img = container.querySelector("img");
    expect(img).toHaveAttribute("width", "28");
    expect(img).toHaveAttribute("height", "28");
  });
});

describe("MouceWindow", () => {
  let stateCb: ((s: MouceState) => void) | undefined;
  let settingsCb: ((s: Settings) => void) | undefined;
  const unlistenState = vi.fn();
  const unlistenSettings = vi.fn();

  beforeEach(() => {
    vi.resetAllMocks();
    stateCb = undefined;
    settingsCb = undefined;
    vi.mocked(ipc.getSettings).mockResolvedValue(baseSettings);
    vi.mocked(ipc.mouceClicked).mockResolvedValue(undefined);
    vi.mocked(ipc.startDragging).mockResolvedValue(undefined);
    vi.mocked(ipc.onMouceState).mockImplementation((cb) => {
      stateCb = cb;
      return Promise.resolve(unlistenState);
    });
    vi.mocked(ipc.onSettingsChanged).mockImplementation((cb) => {
      settingsCb = cb;
      return Promise.resolve(unlistenSettings);
    });
  });

  async function mount() {
    const utils = render(<MouceWindow />);
    // Wait for settings to load and listeners to attach.
    await waitFor(() => expect(stateCb).toBeDefined());
    await waitFor(() => expect(settingsCb).toBeDefined());
    await waitFor(() => expect(utils.container.querySelector("img")).toHaveAttribute("width", "36"));
    return utils;
  }

  it("click without movement calls mouceClicked, not startDragging", async () => {
    const { container } = await mount();
    const root = container.firstElementChild as HTMLElement;
    fireEvent.pointerDown(root, { clientX: 20, clientY: 20, button: 0 });
    fireEvent.pointerUp(root, { clientX: 20, clientY: 20, button: 0 });
    expect(ipc.mouceClicked).toHaveBeenCalledTimes(1);
    expect(ipc.startDragging).not.toHaveBeenCalled();
  });

  it("moving more than 3px starts dragging once and suppresses the click", async () => {
    const { container } = await mount();
    const root = container.firstElementChild as HTMLElement;
    fireEvent.pointerDown(root, { clientX: 20, clientY: 20, button: 0 });
    fireEvent.pointerMove(root, { clientX: 30, clientY: 20, buttons: 1 });
    fireEvent.pointerMove(root, { clientX: 40, clientY: 25, buttons: 1 });
    fireEvent.pointerUp(root, { clientX: 40, clientY: 25, button: 0 });
    expect(ipc.startDragging).toHaveBeenCalledTimes(1);
    expect(ipc.mouceClicked).not.toHaveBeenCalled();
  });

  it("a tiny wobble within 3px is still a click", async () => {
    const { container } = await mount();
    const root = container.firstElementChild as HTMLElement;
    fireEvent.pointerDown(root, { clientX: 20, clientY: 20, button: 0 });
    fireEvent.pointerMove(root, { clientX: 22, clientY: 21, buttons: 1 });
    fireEvent.pointerUp(root, { clientX: 22, clientY: 21, button: 0 });
    expect(ipc.startDragging).not.toHaveBeenCalled();
    expect(ipc.mouceClicked).toHaveBeenCalledTimes(1);
  });

  it("hover movement with no button held never starts dragging", async () => {
    const { container } = await mount();
    const root = container.firstElementChild as HTMLElement;
    fireEvent.pointerDown(root, { clientX: 20, clientY: 20, button: 0, buttons: 1 });
    fireEvent.pointerMove(root, { clientX: 30, clientY: 20, buttons: 0 });
    expect(ipc.startDragging).not.toHaveBeenCalled();
    // The press was cleared, so a later release is not a click either.
    fireEvent.pointerUp(root, { clientX: 30, clientY: 20, button: 0 });
    expect(ipc.mouceClicked).not.toHaveBeenCalled();
  });

  it("pointercancel clears the press", async () => {
    const { container } = await mount();
    const root = container.firstElementChild as HTMLElement;
    fireEvent.pointerDown(root, { clientX: 20, clientY: 20, button: 0, buttons: 1 });
    fireEvent.pointerCancel(root);
    fireEvent.pointerMove(root, { clientX: 30, clientY: 20, buttons: 1 });
    expect(ipc.startDragging).not.toHaveBeenCalled();
  });

  it("lost pointer capture clears the press", async () => {
    const { container } = await mount();
    const root = container.firstElementChild as HTMLElement;
    fireEvent.pointerDown(root, { clientX: 20, clientY: 20, button: 0, buttons: 1 });
    fireEvent.lostPointerCapture(root);
    fireEvent.pointerMove(root, { clientX: 30, clientY: 20, buttons: 1 });
    expect(ipc.startDragging).not.toHaveBeenCalled();
  });

  it("suppresses the native context menu", async () => {
    const { container } = await mount();
    const root = container.firstElementChild as HTMLElement;
    // fireEvent returns false when the event was default-prevented.
    expect(fireEvent.contextMenu(root)).toBe(false);
  });

  it("is exposed as a button", async () => {
    await mount();
    expect(screen.getByRole("button", { name: /mouce/i })).toBeInTheDocument();
  });

  it("attaches a catch handler to each listen() promise immediately", async () => {
    const rejected = Promise.reject(new Error("no events"));
    const catchSpy = vi.spyOn(rejected, "catch");
    vi.mocked(ipc.onMouceState).mockImplementation(() => rejected);
    const { unmount } = render(<MouceWindow />);
    expect(catchSpy).toHaveBeenCalled();
    await waitFor(() => expect(ipc.getSettings).toHaveBeenCalled());
    unmount();
  });

  it("mouce-state event updates the class", async () => {
    const { container } = await mount();
    expect(container.querySelector("img")).toHaveClass("mouce--idle");
    act(() => stateCb!("thinking"));
    expect(container.querySelector("img")).toHaveClass("mouce--thinking");
  });

  it("settings-changed updates size and rotation", async () => {
    const { container } = await mount();
    act(() => settingsCb!({ ...baseSettings, mouce_size: "large", mouce_rotation: false }));
    const img = container.querySelector("img");
    expect(img).toHaveAttribute("width", "48");
    expect(img).toHaveClass("mouce--still");
  });

  it("removes listeners on unmount", async () => {
    const { unmount } = await mount();
    unmount();
    await waitFor(() => expect(unlistenState).toHaveBeenCalledTimes(1));
    expect(unlistenSettings).toHaveBeenCalledTimes(1);
  });
});
