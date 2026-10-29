// Mirrors the Rust types in src-tauri/src (settings.rs, ollama_setup.rs, chat.rs, commands.rs,
// dictionary/).

export type ProviderId = "ollama" | "openai" | "groq" | "gemini" | "anthropic";

export type MouceSize = "small" | "medium" | "large";

export function mouceSizePx(size: MouceSize): number {
  switch (size) {
    case "small":
      return 28;
    case "medium":
      return 36;
    case "large":
      return 48;
  }
}

export interface Settings {
  provider: ProviderId;
  models: Partial<Record<ProviderId, string>>;
  ollama_base_url: string;
  mouce_size: MouceSize;
  mouce_rotation: boolean;
  mouce_position: [number, number] | null;
  /** Starts "Circle to look up" from any app; empty turns it off. */
  circle_hotkey: string;
  /** Opens the chat from any app; empty turns it off. */
  chat_hotkey: string;
  history_enabled: boolean;
  fallback_to_local: boolean;
  first_run_done: boolean;
  /** Floating controls are drawn as Liquid Glass; off makes every surface solid. */
  liquid_glass: boolean;
  /** Ctrl + right-click a word in any app to look it up. */
  dictionary_enabled: boolean;
}

export type MouceState = "idle" | "thinking" | "looking" | "reminder";

export type HomeTabId = "home" | "chat" | "calendar" | "reminders" | "customize" | "settings";

export type OllamaStatus =
  | { state: "notInstalled" }
  | { state: "notRunning" }
  | { state: "running"; models: string[] };

export interface ChatTurn {
  role: "user" | "assistant";
  text: string;
  /** The user shared their screen with this message. */
  looked: boolean;
  /** The user circled part of their screen for this message. */
  circled: boolean;
}

/** Part of the screen as fractions (0 to 1) of its width and height. */
export interface Selection {
  x: number;
  y: number;
  w: number;
  h: number;
}

export interface ChatReply {
  text: string;
  /** The user pressed Stop before the answer was finished. */
  stopped: boolean;
  /** The chosen online AI failed, so the local one answered instead. */
  used_local: boolean;
}

export interface PullProgress {
  status: string;
  completed: number | null;
  total: number | null;
}

/** One meaning of a word, with an example of it in use when the dictionary has one. */
export interface Sense {
  meaning: string;
  example: string | null;
}

/** A word as one part of speech, from the dictionary built into mouce. */
export interface DictionaryEntry {
  word: string;
  part_of_speech: "noun" | "verb" | "adjective" | "adverb" | "";
  /** In the International Phonetic Alphabet. */
  pronunciation: string | null;
  senses: Sense[];
}

/** What the word card shows (`dictionary::Card`). */
export interface WordCard {
  /** New for each look-up. */
  id: number;
  /** The word as it was on screen; empty when `missing` says why. */
  word: string;
  /** The sentence it was in; may be empty. */
  context: string;
  /** "word": there was no word there; "secret": a password box, which mouce leaves alone. */
  missing: "word" | "secret" | null;
  entries: DictionaryEntry[];
  /** Whether the AI is saying what the word means in its sentence. */
  explaining: "off" | "writing" | "done" | "failed";
  explanation: string;
}
