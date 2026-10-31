import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { CalendarItem, ChatReply, ChatTurn, Settings } from "../lib/types";

vi.mock("../lib/ipc");

import * as ipc from "../lib/ipc";
import { CIRCLE_QUESTION, ChatWindow, LOOK_QUESTION, circleNote, lookNote } from "./ChatWindow";

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

const done = (text: string, extra: Partial<ChatReply> = {}): ChatReply => ({
  text,
  stopped: false,
  used_local: false,
  ...extra,
});

// A stand-in for the Rust chat: `chatSend` records the question and answer the way
// `chat_send` does, so the window's refresh from `chatHistory` sees them.
let saved: ChatTurn[];
// A stand-in for the circled area waiting in Rust, and the event that announces it.
let attachment: string | null;
let attachmentCb: ((preview: string | null) => void) | undefined;
// The event "Chat about this" on the word card sends to an open chat.
let draftCb: ((text: string) => void) | undefined;

function answerWith(chunks: string[], extra: Partial<ChatReply> = {}) {
  vi.mocked(ipc.chatSend).mockImplementation(async (text, look, _plans, onDelta) => {
    chunks.forEach(onDelta);
    const answer = chunks.join("");
    // Like Rust: a waiting circled area goes with the question unless the screen is shared.
    const circled = !look && attachment !== null;
    if (circled) attachment = null;
    const question = text || (look ? LOOK_QUESTION : CIRCLE_QUESTION);
    saved.push({ role: "user", text: question, looked: look, circled, calendar: _plans });
    if (answer) saved.push({ role: "assistant", text: answer, looked: false, circled: false });
    return done(answer, extra);
  });
}

describe("ChatWindow", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    saved = [];
    attachment = null;
    attachmentCb = undefined;
    vi.mocked(ipc.getSettings).mockResolvedValue(baseSettings);
    vi.mocked(ipc.onSettingsChanged).mockResolvedValue(() => {});
    vi.mocked(ipc.chatHistory).mockImplementation(async () => [...saved]);
    vi.mocked(ipc.chatStop).mockResolvedValue();
    vi.mocked(ipc.chatAttachment).mockImplementation(async () => attachment);
    vi.mocked(ipc.chatDiscardAttachment).mockImplementation(async () => {
      attachment = null;
    });
    vi.mocked(ipc.onChatAttachment).mockImplementation(async (cb) => {
      attachmentCb = cb;
      return () => {};
    });
    vi.mocked(ipc.circleStart).mockResolvedValue();
    vi.mocked(ipc.chatTakeDraft).mockResolvedValue(null);
    vi.mocked(ipc.onChatDraft).mockImplementation(async (cb) => {
      draftCb = cb;
      return () => {};
    });
    vi.mocked(ipc.chatClear).mockImplementation(async () => {
      saved = [];
    });
  });

  it("shows the saved conversation and the AI in use", async () => {
    saved = [
      { role: "user", text: "hi", looked: true, circled: false },
      { role: "assistant", text: "Hello!", looked: false, circled: false },
    ];
    render(<ChatWindow />);
    expect(await screen.findByText("Hello!")).toBeInTheDocument();
    expect(screen.getByText("hi")).toBeInTheDocument();
    expect(screen.getByText("Shared screen")).toBeInTheDocument();
    expect(await screen.findByText("Local · gemma3:4b")).toBeInTheDocument();
  });

  it("a question from the word card waits in the message box, ready to send", async () => {
    vi.mocked(ipc.chatTakeDraft).mockResolvedValue("What does “bank” mean?");
    render(<ChatWindow />);
    const box = screen.getByRole("textbox", { name: "Message" });
    await waitFor(() => expect(box).toHaveValue("What does “bank” mean?"));
    expect(ipc.chatSend).not.toHaveBeenCalled();

    act(() => draftCb!("What does “goose” mean?"));
    expect(box).toHaveValue("What does “goose” mean?");
    expect(box).toHaveFocus();
  });

  it("the logo in the header opens the home window", async () => {
    vi.mocked(ipc.openHome).mockResolvedValue();
    render(<ChatWindow />);
    await userEvent.click(screen.getByRole("button", { name: "Open mouce home" }));
    expect(ipc.openHome).toHaveBeenCalledTimes(1);
  });

  it("moves the chat full size into the home window", async () => {
    vi.mocked(ipc.openHomeChat).mockResolvedValue();
    render(<ChatWindow />);
    await userEvent.click(screen.getByRole("button", { name: "Open full size in mouce home" }));
    expect(ipc.openHomeChat).toHaveBeenCalledTimes(1);
  });

  it("picks up questions asked in the other view when it gets focus", async () => {
    render(<ChatWindow />);
    await screen.findByText("Ask me anything");
    saved = [
      { role: "user", text: "asked in the home window", looked: false, circled: false },
      { role: "assistant", text: "Answered there.", looked: false, circled: false },
    ];
    act(() => {
      window.dispatchEvent(new Event("focus"));
    });
    expect(await screen.findByText("Answered there.")).toBeInTheDocument();
  });

  it("sends with Enter and streams the answer in", async () => {
    answerWith(["Hel", "lo there"]);
    render(<ChatWindow />);
    const input = screen.getByRole("textbox", { name: "Message" });
    await waitFor(() => expect(input).toHaveFocus());
    await userEvent.type(input, "hi mouce{Enter}");
    expect(ipc.chatSend).toHaveBeenCalledWith("hi mouce", false, false, expect.any(Function));
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
    expect(ipc.chatSend).toHaveBeenCalledWith("", true, false, expect.any(Function));
    expect(await screen.findByText("A spreadsheet")).toBeInTheDocument();
    expect(screen.getByText(LOOK_QUESTION)).toBeInTheDocument();
    expect(lookButton).toHaveAttribute("aria-pressed", "false");
  });

  it("the empty chat offers to look at the screen right away", async () => {
    answerWith(["Your desktop"]);
    render(<ChatWindow />);
    await userEvent.click(screen.getByRole("button", { name: LOOK_QUESTION }));
    expect(ipc.chatSend).toHaveBeenCalledWith("", true, false, expect.any(Function));
    expect(await screen.findByText("Your desktop")).toBeInTheDocument();
  });

  it("shows Stop while answering and stops when pressed", async () => {
    let finish: (r: ChatReply) => void = () => {};
    let write: (chunk: string) => void = () => {};
    vi.mocked(ipc.chatSend).mockImplementation((text, look, _plans, onDelta) => {
      write = onDelta;
      saved.push({ role: "user", text, looked: look, circled: false });
      return new Promise((resolve) => {
        finish = resolve;
      });
    });
    render(<ChatWindow />);
    await userEvent.type(screen.getByRole("textbox", { name: "Message" }), "long one{Enter}");
    expect(screen.getByLabelText("mouce is writing")).toBeInTheDocument();
    act(() => write("Part"));
    expect(screen.getByText("Part")).toBeInTheDocument();
    // The answer streams to this view, so it cannot move to the other one until it is done.
    expect(screen.getByRole("button", { name: "Open full size in mouce home" })).toBeDisabled();
    await userEvent.click(screen.getByRole("button", { name: "Stop" }));
    expect(ipc.chatStop).toHaveBeenCalled();
    saved.push({ role: "assistant", text: "Part", looked: false, circled: false });
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

  describe("remind me…", () => {
    const callMum: CalendarItem = {
      id: 3,
      kind: "reminder",
      title: "Call mum",
      date: "2026-10-31",
      start_date: "2026-10-31",
      end_time: null,
      time: "18:00",
      remind_before: null,
      repeat: "weekly",
      alert_at: 1,
      done: false,
    };

    beforeEach(() => {
      vi.mocked(ipc.calendarUnderstand).mockResolvedValue({
        title: "Call mum",
        date: "2026-10-31",
        time: "18:00",
        end_time: null,
        repeat: "weekly",
        understood: true,
      });
      vi.mocked(ipc.calendarSave).mockResolvedValue(callMum);
    });

    it("opens a form to check instead of asking the AI, and says what was saved", async () => {
      const user = userEvent.setup();
      render(<ChatWindow />);
      await user.type(screen.getByRole("textbox", { name: "Message" }), "remind me to call mum every Saturday at 6pm{Enter}");
      const form = await screen.findByRole("form", { name: "New reminder" });
      expect(ipc.chatSend).not.toHaveBeenCalled();
      expect(ipc.calendarUnderstand).toHaveBeenCalledWith(
        "reminder",
        "remind me to call mum every Saturday at 6pm",
        expect.anything(),
      );
      expect(screen.getByText("remind me to call mum every Saturday at 6pm")).toBeInTheDocument();
      expect(within(form).getByLabelText("Repeats")).toHaveValue("weekly");
      await user.click(within(form).getByRole("button", { name: "Save reminder" }));
      expect(ipc.calendarSave).toHaveBeenCalledWith(null, expect.objectContaining({ kind: "reminder", repeat: "weekly" }));
      expect(await screen.findByRole("status")).toHaveTextContent(/Reminder set: “Call mum”, .* at 18:00, every week\./);
    });

    it("says nothing was saved when cancelled", async () => {
      const user = userEvent.setup();
      render(<ChatWindow />);
      await user.type(screen.getByRole("textbox", { name: "Message" }), "add dentist on Friday to my calendar{Enter}");
      const form = await screen.findByRole("form", { name: "New event" });
      await user.click(within(form).getByRole("button", { name: "Cancel" }));
      expect(screen.getByText("Okay, nothing was saved.")).toBeInTheDocument();
      expect(ipc.calendarSave).not.toHaveBeenCalled();
    });

    it("can ask the AI about it instead", async () => {
      answerWith(["Sure, here's how."]);
      const user = userEvent.setup();
      render(<ChatWindow />);
      await user.type(screen.getByRole("textbox", { name: "Message" }), "remind me how loops work{Enter}");
      await screen.findByRole("form", { name: "New reminder" });
      await user.click(screen.getByRole("button", { name: "Ask mouce about it instead" }));
      expect(ipc.chatSend).toHaveBeenCalledWith("remind me how loops work", false, false, expect.any(Function));
      expect(await screen.findByText("Sure, here's how.")).toBeInTheDocument();
      expect(screen.queryByRole("form", { name: "New reminder" })).not.toBeInTheDocument();
    });

    it("takes the user's plans to the AI for a question about them, and says so", async () => {
      answerWith(["The dentist at 3."]);
      const user = userEvent.setup();
      render(<ChatWindow />);
      await user.type(screen.getByRole("textbox", { name: "Message" }), "what's on tomorrow?{Enter}");
      expect(ipc.chatSend).toHaveBeenCalledWith("what's on tomorrow?", false, true, expect.any(Function));
      expect(await screen.findByText("Your plans")).toBeInTheDocument();
    });

    it("keeps the plans back when Settings say so", async () => {
      vi.mocked(ipc.getSettings).mockResolvedValue({ ...baseSettings, calendar_in_chat: false });
      answerWith(["I can't see your calendar."]);
      const user = userEvent.setup();
      render(<ChatWindow />);
      await screen.findByText(/Local/);
      await user.type(screen.getByRole("textbox", { name: "Message" }), "what's on tomorrow?{Enter}");
      expect(ipc.chatSend).toHaveBeenCalledWith("what's on tomorrow?", false, false, expect.any(Function));
    });

    it("leaves a question about the screen alone", async () => {
      answerWith(["That is a calendar."]);
      const user = userEvent.setup();
      render(<ChatWindow />);
      await user.click(screen.getByRole("button", { name: "Look at my screen" }));
      await user.type(screen.getByRole("textbox", { name: "Message" }), "remind me what this is{Enter}");
      expect(ipc.chatSend).toHaveBeenCalledWith("remind me what this is", true, false, expect.any(Function));
      expect(ipc.calendarUnderstand).not.toHaveBeenCalled();
    });
  });

  it("New chat clears the conversation", async () => {
    saved = [
      { role: "user", text: "hi", looked: false, circled: false },
      { role: "assistant", text: "Hello!", looked: false, circled: false },
    ];
    render(<ChatWindow />);
    await screen.findByText("Hello!");
    await userEvent.click(screen.getByRole("button", { name: "New chat" }));
    expect(ipc.chatClear).toHaveBeenCalled();
    expect(screen.queryByText("Hello!")).not.toBeInTheDocument();
    expect(screen.getByText("Ask me anything")).toBeInTheDocument();
    expect(screen.getByText("Or let me look at your screen.")).toBeInTheDocument();
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

describe("ChatWindow with Circle to look up", () => {
  const PREVIEW = "data:image/jpeg;base64,UFJFVklFVw==";

  beforeEach(() => {
    vi.resetAllMocks();
    saved = [];
    attachment = null;
    attachmentCb = undefined;
    vi.mocked(ipc.getSettings).mockResolvedValue(baseSettings);
    vi.mocked(ipc.onSettingsChanged).mockResolvedValue(() => {});
    vi.mocked(ipc.chatHistory).mockImplementation(async () => [...saved]);
    vi.mocked(ipc.chatAttachment).mockImplementation(async () => attachment);
    vi.mocked(ipc.chatDiscardAttachment).mockImplementation(async () => {
      attachment = null;
    });
    vi.mocked(ipc.onChatAttachment).mockImplementation(async (cb) => {
      attachmentCb = cb;
      return () => {};
    });
    vi.mocked(ipc.circleStart).mockResolvedValue();
    vi.mocked(ipc.chatTakeDraft).mockResolvedValue(null);
    vi.mocked(ipc.onChatDraft).mockImplementation(async (cb) => {
      draftCb = cb;
      return () => {};
    });
  });

  it("the Circle button starts circling", async () => {
    render(<ChatWindow />);
    await userEvent.click(screen.getByRole("button", { name: "Circle" }));
    expect(ipc.circleStart).toHaveBeenCalled();
  });

  it("the Circle button names the circle shortcut, and the empty chat does not", async () => {
    render(<ChatWindow />);
    const circle = screen.getByRole("button", { name: "Circle" });
    await waitFor(() => expect(circle).toHaveAttribute("title", "Circle to look up (Ctrl+Alt+O)"));
    expect(screen.queryByText(/to circle something/)).not.toBeInTheDocument();
  });

  it("a circled area arrives, is shown with where it goes, and is asked about", async () => {
    answerWith(["The mouce logo"]);
    render(<ChatWindow />);
    await waitFor(() => expect(attachmentCb).toBeDefined());
    attachment = PREVIEW;
    act(() => attachmentCb!(PREVIEW));
    const preview = screen.getByRole("img", { name: "The part of your screen you circled" });
    expect(preview).toHaveAttribute("src", PREVIEW);
    expect(await screen.findByText(/stays on this computer/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Look at my screen" })).toBeDisabled();
    const input = screen.getByRole("textbox", { name: "Message" });
    expect(input).toHaveFocus();
    await userEvent.click(screen.getByRole("button", { name: "Send" }));
    expect(ipc.chatSend).toHaveBeenCalledWith("", false, false, expect.any(Function));
    expect(await screen.findByText("The mouce logo")).toBeInTheDocument();
    expect(screen.getByText(CIRCLE_QUESTION)).toBeInTheDocument();
    expect(screen.getByText("Circled", { selector: ".bubble-chip" })).toBeInTheDocument();
    expect(screen.queryByRole("img", { name: /you circled/ })).not.toBeInTheDocument();
  });

  it("a circled area already waiting shows when the chat opens", async () => {
    attachment = PREVIEW;
    render(<ChatWindow />);
    expect(await screen.findByRole("img", { name: /you circled/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Send" })).toBeEnabled();
  });

  it("the circled area can be removed", async () => {
    attachment = PREVIEW;
    render(<ChatWindow />);
    await screen.findByRole("img", { name: /you circled/ });
    await userEvent.click(screen.getByRole("button", { name: "Remove the circled area" }));
    expect(ipc.chatDiscardAttachment).toHaveBeenCalled();
    expect(screen.queryByRole("img", { name: /you circled/ })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Send" })).toBeDisabled();
  });

  it("the circled area comes back when its question fails", async () => {
    attachment = PREVIEW;
    vi.mocked(ipc.chatSend).mockRejectedValue("Ollama isn't running");
    render(<ChatWindow />);
    await screen.findByRole("img", { name: /you circled/ });
    await userEvent.type(screen.getByRole("textbox", { name: "Message" }), "what is it?{Enter}");
    expect(await screen.findByRole("alert")).toHaveTextContent("Ollama isn't running");
    expect(await screen.findByRole("img", { name: /you circled/ })).toBeInTheDocument();
  });
});

describe("circleNote", () => {
  it("says where the circled area goes", () => {
    expect(circleNote(baseSettings)).toMatch(/stays on this computer/);
    expect(circleNote({ ...baseSettings, provider: "gemini" })).toBe(
      "Only this part is sent to Gemini with your question.",
    );
    expect(circleNote(null)).toMatch(/Only this part/);
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
