import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ChatReply, ChatTurn, Settings } from "../lib/types";

vi.mock("../lib/ipc");

import * as ipc from "../lib/ipc";
import { ChatWindow, LOOK_QUESTION, lookNote } from "./ChatWindow";

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

const done = (text: string, extra: Partial<ChatReply> = {}): ChatReply => ({
  text,
  stopped: false,
  used_local: false,
  ...extra,
});

// A stand-in for the Rust chat: `chatSend` records the question and answer the way
// `chat_send` does, so the window's refresh from `chatHistory` sees them.
let saved: ChatTurn[];

function answerWith(chunks: string[], extra: Partial<ChatReply> = {}) {
  vi.mocked(ipc.chatSend).mockImplementation(async (text, look, onDelta) => {
    chunks.forEach(onDelta);
    const answer = chunks.join("");
    saved.push({ role: "user", text: text || LOOK_QUESTION, looked: look });
    if (answer) saved.push({ role: "assistant", text: answer, looked: false });
    return done(answer, extra);
  });
}

describe("ChatWindow", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    saved = [];
    vi.mocked(ipc.getSettings).mockResolvedValue(baseSettings);
    vi.mocked(ipc.onSettingsChanged).mockResolvedValue(() => {});
    vi.mocked(ipc.chatHistory).mockImplementation(async () => [...saved]);
    vi.mocked(ipc.chatStop).mockResolvedValue();
    vi.mocked(ipc.chatClear).mockImplementation(async () => {
      saved = [];
    });
  });

  it("shows the saved conversation and the AI in use", async () => {
    saved = [
      { role: "user", text: "hi", looked: true },
      { role: "assistant", text: "Hello!", looked: false },
    ];
    render(<ChatWindow />);
    expect(await screen.findByText("Hello!")).toBeInTheDocument();
    expect(screen.getByText("hi")).toBeInTheDocument();
    expect(screen.getByText("Shared screen")).toBeInTheDocument();
    expect(await screen.findByText("Local · gemma3:4b")).toBeInTheDocument();
  });

  it("sends with Enter and streams the answer in", async () => {
    answerWith(["Hel", "lo there"]);
    render(<ChatWindow />);
    const input = screen.getByRole("textbox", { name: "Message" });
    await waitFor(() => expect(input).toHaveFocus());
    await userEvent.type(input, "hi mouce{Enter}");
    expect(ipc.chatSend).toHaveBeenCalledWith("hi mouce", false, expect.any(Function));
    expect(await screen.findByText("Hello there")).toBeInTheDocument();
    expect(screen.getByText("hi mouce")).toBeInTheDocument();
    expect(input).toHaveValue("");
  });

  it("Shift+Enter starts a new line instead of sending", async () => {
    render(<ChatWindow />);
    const input = screen.getByRole("textbox", { name: "Message" });
    await userEvent.type(input, "one{Shift>}{Enter}{/Shift}two");
    expect(input).toHaveValue("one\ntwo");
    expect(ipc.chatSend).not.toHaveBeenCalled();
  });

  it("Send is off until there is something to ask", async () => {
    render(<ChatWindow />);
    const send = screen.getByRole("button", { name: "Send" });
    expect(send).toBeDisabled();
    await userEvent.type(screen.getByRole("textbox", { name: "Message" }), "   ");
    expect(send).toBeDisabled();
    await userEvent.click(screen.getByRole("button", { name: "Look at my screen" }));
    expect(send).toBeEnabled();
  });

  it("Look at my screen says where the picture goes, then sends it", async () => {
    answerWith(["A spreadsheet"]);
    render(<ChatWindow />);
    await screen.findByText("Local · gemma3:4b");
    const lookButton = screen.getByRole("button", { name: "Look at my screen" });
    await userEvent.click(lookButton);
    expect(lookButton).toHaveAttribute("aria-pressed", "true");
    expect(screen.getByText(/It stays on this computer/)).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Send" }));
    expect(ipc.chatSend).toHaveBeenCalledWith("", true, expect.any(Function));
    expect(await screen.findByText("A spreadsheet")).toBeInTheDocument();
    expect(screen.getByText(LOOK_QUESTION)).toBeInTheDocument();
    expect(lookButton).toHaveAttribute("aria-pressed", "false");
  });

  it("the empty chat offers to look at the screen right away", async () => {
    answerWith(["Your desktop"]);
    render(<ChatWindow />);
    await userEvent.click(screen.getByRole("button", { name: LOOK_QUESTION }));
    expect(ipc.chatSend).toHaveBeenCalledWith("", true, expect.any(Function));
    expect(await screen.findByText("Your desktop")).toBeInTheDocument();
  });

  it("shows Stop while answering and stops when pressed", async () => {
    let finish: (r: ChatReply) => void = () => {};
    let write: (chunk: string) => void = () => {};
    vi.mocked(ipc.chatSend).mockImplementation((text, look, onDelta) => {
      write = onDelta;
      saved.push({ role: "user", text, looked: look });
      return new Promise((resolve) => {
        finish = resolve;
      });
    });
    render(<ChatWindow />);
    await userEvent.type(screen.getByRole("textbox", { name: "Message" }), "long one{Enter}");
    expect(screen.getByLabelText("mouce is writing")).toBeInTheDocument();
    act(() => write("Part"));
    expect(screen.getByText("Part")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Stop" }));
    expect(ipc.chatStop).toHaveBeenCalled();
    saved.push({ role: "assistant", text: "Part", looked: false });
    await act(async () => finish(done("Part", { stopped: true })));
    expect(await screen.findByText("Stopped")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Send" })).toBeInTheDocument();
  });

  it("an error is shown and the question goes back in the box", async () => {
    vi.mocked(ipc.chatSend).mockRejectedValue("Ollama isn't running");
    render(<ChatWindow />);
    const input = screen.getByRole("textbox", { name: "Message" });
    await userEvent.type(input, "hi{Enter}");
    expect(await screen.findByRole("alert")).toHaveTextContent("Ollama isn't running");
    expect(input).toHaveValue("hi");
    expect(screen.queryByText("hi", { selector: ".bubble-text" })).not.toBeInTheDocument();
  });

  it("says when the local AI answered instead of the online one", async () => {
    vi.mocked(ipc.getSettings).mockResolvedValue({
      ...baseSettings,
      provider: "anthropic",
      models: { anthropic: "claude-x" },
    });
    answerWith(["local answer"], { used_local: true });
    render(<ChatWindow />);
    expect(await screen.findByText("Claude · claude-x")).toBeInTheDocument();
    await userEvent.type(screen.getByRole("textbox", { name: "Message" }), "hi{Enter}");
    expect(
      await screen.findByText("Answered by the local AI because Claude didn't respond"),
    ).toBeInTheDocument();
  });

  it("New chat clears the conversation", async () => {
    saved = [
      { role: "user", text: "hi", looked: false },
      { role: "assistant", text: "Hello!", looked: false },
    ];
    render(<ChatWindow />);
    await screen.findByText("Hello!");
    await userEvent.click(screen.getByRole("button", { name: "New chat" }));
    expect(ipc.chatClear).toHaveBeenCalled();
    expect(screen.queryByText("Hello!")).not.toBeInTheDocument();
    expect(screen.getByText("Ask me anything, or let me look at your screen.")).toBeInTheDocument();
  });

  it("follows settings changes made in another window", async () => {
    let settingsCb: ((s: Settings) => void) | undefined;
    vi.mocked(ipc.onSettingsChanged).mockImplementation(async (cb) => {
      settingsCb = cb;
      return () => {};
    });
    render(<ChatWindow />);
    await screen.findByText("Local · gemma3:4b");
    act(() => settingsCb!({ ...baseSettings, provider: "groq", models: { groq: "llama" } }));
    expect(screen.getByText("Groq · llama")).toBeInTheDocument();
  });
});

describe("lookNote", () => {
  it("names the online provider the picture is sent to", () => {
    expect(lookNote({ ...baseSettings, provider: "openai" })).toBe(
      "mouce will take one picture of this screen and send it to OpenAI.",
    );
    expect(lookNote(baseSettings)).toMatch(/stays on this computer/);
    expect(lookNote(null)).toMatch(/one picture/);
  });
});
