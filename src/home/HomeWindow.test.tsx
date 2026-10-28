import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { HomeTabId, Settings } from "../lib/types";

vi.mock("../lib/ipc");

import * as ipc from "../lib/ipc";
import { monthTitle } from "./dates";
import { HomeWindow } from "./HomeWindow";

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

let tabCb: ((tab: HomeTabId) => void) | undefined;

function aiCard() {
  return screen.getByRole("heading", { level: 2, name: /gemma|llama|gpt|No model/ }).closest("section")!;
}

describe("HomeWindow", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    tabCb = undefined;
    vi.mocked(ipc.getSettings).mockResolvedValue(baseSettings);
    vi.mocked(ipc.updateSettings).mockImplementation(async (s) => s);
    vi.mocked(ipc.onSettingsChanged).mockResolvedValue(() => {});
    vi.mocked(ipc.onHomeTab).mockImplementation(async (cb) => {
      tabCb = cb;
      return () => {};
    });
    vi.mocked(ipc.ollamaStatus).mockResolvedValue({ state: "running", models: ["gemma3:4b"] });
    vi.mocked(ipc.hasApiKey).mockResolvedValue(false);
    vi.mocked(ipc.listModels).mockResolvedValue([]);
    vi.mocked(ipc.getAutostart).mockResolvedValue(false);
    vi.mocked(ipc.onPullProgress).mockResolvedValue(() => {});
  });

  it("Ask mouce opens the chat", async () => {
    vi.mocked(ipc.openChat).mockResolvedValue();
    render(<HomeWindow />);
    await userEvent.click(screen.getByRole("button", { name: "Ask mouce" }));
    expect(ipc.openChat).toHaveBeenCalled();
  });

  describe("tabs", () => {
    it("opens on Home with every tab listed", async () => {
      render(<HomeWindow />);
      const tabs = screen.getAllByRole("tab").map((t) => t.textContent);
      expect(tabs).toEqual(["Home", "Calendar", "Reminders", "Customize", "⚙Settings"]);
      expect(screen.getByRole("tab", { name: "Home" })).toHaveAttribute("aria-selected", "true");
      expect(await screen.findByText("Ready · runs on this laptop")).toBeInTheDocument();
    });

    it("starts on the tab Rust asked for", async () => {
      vi.mocked(ipc.initialHomeTab).mockReturnValue("settings");
      render(<HomeWindow />);
      expect(screen.getByRole("tab", { name: /Settings/ })).toHaveAttribute("aria-selected", "true");
      expect(await screen.findByLabelText("Provider")).toBeInTheDocument();
    });

    it("ignores an unknown starting tab", () => {
      vi.mocked(ipc.initialHomeTab).mockReturnValue("nope" as HomeTabId);
      render(<HomeWindow />);
      expect(screen.getByRole("tab", { name: "Home" })).toHaveAttribute("aria-selected", "true");
    });

    it("switches tab when the tray asks", async () => {
      render(<HomeWindow />);
      await waitFor(() => expect(tabCb).toBeDefined());
      act(() => tabCb!("customize"));
      expect(screen.getByRole("tab", { name: "Customize" })).toHaveAttribute("aria-selected", "true");
      expect(await screen.findByRole("checkbox", { name: "Sway" })).toBeInTheDocument();
    });

    it("moves between tabs with the arrow keys", async () => {
      const user = userEvent.setup();
      render(<HomeWindow />);
      await user.click(screen.getByRole("tab", { name: "Home" }));
      await user.keyboard("{ArrowRight}");
      expect(screen.getByRole("tab", { name: "Calendar" })).toHaveAttribute("aria-selected", "true");
      expect(screen.getByRole("tab", { name: "Calendar" })).toHaveFocus();
      await user.keyboard("{ArrowLeft}{ArrowLeft}");
      expect(screen.getByRole("tab", { name: /Settings/ })).toHaveAttribute("aria-selected", "true");
    });

    it("links the panel to its tab", async () => {
      const user = userEvent.setup();
      render(<HomeWindow />);
      await user.click(screen.getByRole("tab", { name: "Reminders" }));
      const panel = screen.getByRole("tabpanel");
      expect(panel).toHaveAttribute("aria-labelledby", "tab-reminders");
      expect(within(panel).getByText(/No reminders yet/)).toBeInTheDocument();
    });
  });

  describe("AI card", () => {
    it("Change AI opens Settings", async () => {
      const user = userEvent.setup();
      render(<HomeWindow />);
      await user.click(await screen.findByRole("button", { name: "Change AI" }));
      expect(screen.getByRole("tab", { name: /Settings/ })).toHaveAttribute("aria-selected", "true");
    });

    it("shows the default local model and where it runs", async () => {
      render(<HomeWindow />);
      await screen.findByText("Ready · runs on this laptop");
      expect(within(aiCard()).getByText("gemma3:4b")).toBeInTheDocument();
      expect(within(aiCard()).getByText("Local · Ollama")).toBeInTheDocument();
    });

    it("says when Ollama is not running", async () => {
      vi.mocked(ipc.ollamaStatus).mockResolvedValue({ state: "notRunning" });
      render(<HomeWindow />);
      expect(await screen.findByText(/Ollama isn't running/)).toBeInTheDocument();
    });

    it("says when Ollama is not installed", async () => {
      vi.mocked(ipc.ollamaStatus).mockResolvedValue({ state: "notInstalled" });
      render(<HomeWindow />);
      expect(await screen.findByText("Ollama isn't installed")).toBeInTheDocument();
    });

    it("says when the local model is not downloaded", async () => {
      vi.mocked(ipc.ollamaStatus).mockResolvedValue({ state: "running", models: ["llama3:8b"] });
      render(<HomeWindow />);
      expect(await screen.findByText("This model isn't downloaded yet")).toBeInTheDocument();
    });

    it("asks for a key for an online provider without one", async () => {
      vi.mocked(ipc.getSettings).mockResolvedValue({
        ...baseSettings,
        provider: "openai",
        models: { openai: "gpt-4o-mini" },
      });
      render(<HomeWindow />);
      expect(await screen.findByText("Add an API key in Settings")).toBeInTheDocument();
      expect(screen.getByText("Online · OpenAI")).toBeInTheDocument();
      expect(ipc.hasApiKey).toHaveBeenCalledWith("openai");
    });

    it("is ready for an online provider with a key and a model", async () => {
      vi.mocked(ipc.getSettings).mockResolvedValue({
        ...baseSettings,
        provider: "groq",
        models: { groq: "llama-3.1-8b-instant" },
      });
      vi.mocked(ipc.hasApiKey).mockResolvedValue(true);
      render(<HomeWindow />);
      expect(await screen.findByText("Ready · online with your key")).toBeInTheDocument();
      expect(screen.getByText("llama-3.1-8b-instant")).toBeInTheDocument();
    });

    it("asks for a model when an online provider has none", async () => {
      vi.mocked(ipc.getSettings).mockResolvedValue({ ...baseSettings, provider: "gemini" });
      vi.mocked(ipc.hasApiKey).mockResolvedValue(true);
      render(<HomeWindow />);
      expect(await screen.findByText("Choose a model in Settings")).toBeInTheDocument();
      expect(screen.getByText("No model chosen")).toBeInTheDocument();
    });

    it("shows a failed check word for word", async () => {
      vi.mocked(ipc.ollamaStatus).mockRejectedValue("connection reset");
      render(<HomeWindow />);
      expect(await screen.findByText("connection reset")).toBeInTheDocument();
    });

    it("checks again when the window regains focus", async () => {
      vi.mocked(ipc.ollamaStatus).mockResolvedValueOnce({ state: "notRunning" });
      render(<HomeWindow />);
      await screen.findByText(/Ollama isn't running/);
      act(() => {
        window.dispatchEvent(new Event("focus"));
      });
      expect(await screen.findByText("Ready · runs on this laptop")).toBeInTheDocument();
    });
  });

  describe("month and calendar", () => {
    it("highlights today on the dashboard", () => {
      render(<HomeWindow />);
      const today = new Date();
      expect(screen.getByRole("heading", { name: monthTitle(today.getFullYear(), today.getMonth()) }))
        .toBeInTheDocument();
      expect(document.querySelector('[aria-current="date"]')).toHaveTextContent(String(today.getDate()));
    });

    it("pages through months and comes back to today", async () => {
      const user = userEvent.setup();
      render(<HomeWindow />);
      await user.click(screen.getByRole("button", { name: "Open calendar" }));
      const now = new Date();
      const next = new Date(now.getFullYear(), now.getMonth() + 1, 1);
      const todayButton = screen.getByRole("button", { name: "Today" });
      expect(todayButton).toBeDisabled();
      await user.click(screen.getByRole("button", { name: "Next month" }));
      expect(screen.getByRole("heading", { name: monthTitle(next.getFullYear(), next.getMonth()) }))
        .toBeInTheDocument();
      expect(todayButton).toBeEnabled();
      await user.click(todayButton);
      expect(screen.getByRole("heading", { name: monthTitle(now.getFullYear(), now.getMonth()) }))
        .toBeInTheDocument();
    });
  });
});
