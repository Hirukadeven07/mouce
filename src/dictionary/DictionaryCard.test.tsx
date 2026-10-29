import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { WordCard } from "../lib/types";

vi.mock("../lib/ipc");

import * as ipc from "../lib/ipc";
import { DictionaryCard, NOTICE_MS, SHOWN_SENSES } from "./DictionaryCard";

const senses = (n: number) =>
  Array.from({ length: n }, (_, i) => ({ meaning: `meaning ${i + 1}`, example: i === 0 ? "an example" : null }));

const ran: WordCard = {
  id: 1,
  word: "ran",
  context: "She ran to the station.",
  missing: null,
  entries: [
    { word: "run", part_of_speech: "verb", pronunciation: "ɹʌn", senses: senses(5) },
    { word: "run", part_of_speech: "noun", pronunciation: null, senses: senses(2) },
  ],
  explaining: "writing",
  explanation: "",
};

let cardCb: ((card: WordCard) => void) | undefined;

describe("DictionaryCard", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    cardCb = undefined;
    vi.mocked(ipc.dictionaryCurrent).mockResolvedValue(ran);
    vi.mocked(ipc.dictionaryFit).mockResolvedValue();
    vi.mocked(ipc.dictionaryClose).mockResolvedValue();
    vi.mocked(ipc.dictionaryChat).mockResolvedValue();
    vi.mocked(ipc.onDictionaryCard).mockImplementation(async (cb) => {
      cardCb = cb;
      return () => {};
    });
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("shows the dictionary form of the word, its pronunciation and where it came from", async () => {
    render(<DictionaryCard />);
    expect(await screen.findByRole("heading", { level: 1, name: "run" })).toBeInTheDocument();
    expect(screen.getByText("/ɹʌn/")).toBeInTheDocument();
    expect(screen.getByText("from “ran”")).toBeInTheDocument();
    expect(screen.getByText("Meanings from Open English WordNet")).toBeInTheDocument();
  });

  it("lists the first meanings of each part of speech, with more on request", async () => {
    const user = userEvent.setup();
    render(<DictionaryCard />);
    const verb = (await screen.findByRole("heading", { name: "verb" })).closest("section")!;
    expect(within(verb).getAllByRole("listitem")).toHaveLength(SHOWN_SENSES);
    expect(within(verb).getByText("an example")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "2 more meanings" }));
    expect(within(verb).getAllByRole("listitem")).toHaveLength(5);
    expect(screen.getByRole("button", { name: "Fewer meanings" })).toHaveAttribute("aria-expanded", "true");
  });

  it("asks Rust to fit the window to each card", async () => {
    render(<DictionaryCard />);
    await waitFor(() => expect(ipc.dictionaryFit).toHaveBeenCalledWith(1, expect.any(Number)));
    act(() => cardCb!({ ...ran, id: 2 }));
    await waitFor(() => expect(ipc.dictionaryFit).toHaveBeenCalledWith(2, expect.any(Number)));
  });

  it("streams what the AI says the word means in its sentence", async () => {
    render(<DictionaryCard />);
    expect(await screen.findByRole("status", { name: "mouce is thinking" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "In this sentence" })).toBeInTheDocument();
    act(() => cardCb!({ ...ran, explanation: "Moved quickly on foot." }));
    expect(await screen.findByText("Moved quickly on foot.")).toBeInTheDocument();
    expect(screen.queryByRole("status", { name: "mouce is thinking" })).not.toBeInTheDocument();
  });

  it("says so quietly when the AI could not explain", async () => {
    vi.mocked(ipc.dictionaryCurrent).mockResolvedValue({ ...ran, explaining: "failed" });
    render(<DictionaryCard />);
    expect(await screen.findByText(/The AI couldn't explain it this time/)).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "verb" })).toBeInTheDocument();
  });

  it("explains a word the dictionary does not know, or says neither could", async () => {
    const unknown: WordCard = { ...ran, word: "rizz", entries: [], explaining: "done", explanation: "Charm." };
    vi.mocked(ipc.dictionaryCurrent).mockResolvedValue(unknown);
    render(<DictionaryCard />);
    expect(await screen.findByRole("heading", { level: 1, name: "rizz" })).toBeInTheDocument();
    expect(screen.getByText("Charm.")).toBeInTheDocument();
    expect(screen.queryByText(/Open English WordNet/)).not.toBeInTheDocument();

    act(() => cardCb!({ ...unknown, explaining: "failed", explanation: "" }));
    expect(
      await screen.findByText("This word isn't in mouce's dictionary, and the AI couldn't be reached."),
    ).toBeInTheDocument();
  });

  it("a selected phrase is explained by the AI, and named as a phrase", async () => {
    const phrase: WordCard = { ...ran, word: "by and large", entries: [], explaining: "failed" };
    vi.mocked(ipc.dictionaryCurrent).mockResolvedValue(phrase);
    render(<DictionaryCard />);
    expect(await screen.findByRole("heading", { level: 1, name: "by and large" })).toBeInTheDocument();
    expect(
      screen.getByText("This phrase isn't in mouce's dictionary, and the AI couldn't be reached."),
    ).toBeInTheDocument();
  });

  it("closes, or hands the word to the chat", async () => {
    const user = userEvent.setup();
    render(<DictionaryCard />);
    await user.click(await screen.findByRole("button", { name: "Chat about this" }));
    expect(ipc.dictionaryChat).toHaveBeenCalledWith(1);
    await user.click(screen.getByRole("button", { name: "Close" }));
    expect(ipc.dictionaryClose).toHaveBeenCalledWith(1);
  });

  it("says the word aloud with a built-in voice", async () => {
    const speakFn = vi.fn();
    const utterances: { text: string; lang: string }[] = [];
    class Utterance {
      lang = "";
      constructor(public text: string) {
        utterances.push(this);
      }
    }
    vi.stubGlobal("SpeechSynthesisUtterance", Utterance);
    vi.stubGlobal("speechSynthesis", { speak: speakFn, cancel: vi.fn() });
    const user = userEvent.setup();
    render(<DictionaryCard />);
    await user.click(await screen.findByRole("button", { name: "Say “run”" }));
    expect(speakFn).toHaveBeenCalledTimes(1);
    expect(utterances[0]).toMatchObject({ text: "run", lang: "en-US" });
    vi.unstubAllGlobals();
  });

  it("an older card arriving late does not replace a newer one", async () => {
    let resolve: (c: WordCard) => void = () => {};
    vi.mocked(ipc.dictionaryCurrent).mockReturnValue(new Promise((r) => (resolve = r)));
    render(<DictionaryCard />);
    await waitFor(() => expect(cardCb).toBeDefined());
    act(() => cardCb!({ ...ran, id: 5, word: "goose", entries: [] , explaining: "off" }));
    await act(async () => resolve(ran));
    expect(screen.getByRole("heading", { level: 1, name: "goose" })).toBeInTheDocument();
  });

  it("a card with no word says why and closes itself", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    vi.mocked(ipc.dictionaryCurrent).mockResolvedValue({ ...ran, word: "", entries: [], missing: "word", explaining: "off" });
    render(<DictionaryCard />);
    expect(await screen.findByText("mouce couldn't find a word there.")).toBeInTheDocument();
    expect(ipc.dictionaryClose).not.toHaveBeenCalled();
    await act(async () => vi.advanceTimersByTime(NOTICE_MS));
    expect(ipc.dictionaryClose).toHaveBeenCalledWith(1);
  });

  it("leaves password boxes alone", async () => {
    vi.mocked(ipc.dictionaryCurrent).mockResolvedValue({ ...ran, word: "", entries: [], missing: "secret", explaining: "off" });
    render(<DictionaryCard />);
    expect(await screen.findByText("This is a password box, so mouce leaves it alone.")).toBeInTheDocument();
  });
});
