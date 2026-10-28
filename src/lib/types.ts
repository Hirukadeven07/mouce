// Mirrors the Rust types in src-tauri/src (settings.rs, ollama_setup.rs, chat.rs, commands.rs).

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
  circle_hotkey: string;
  /** Opens the chat from any app; empty turns it off. */
  chat_hotkey: string;
  history_enabled: boolean;
  fallback_to_local: boolean;
  first_run_done: boolean;
  /** Floating controls are drawn as Liquid Glass; off makes every surface solid. */
  liquid_glass: boolean;
}

export type MouceState = "idle" | "thinking" | "looking" | "reminder";

export type HomeTabId = "home" | "calendar" | "reminders" | "customize" | "settings";

export type OllamaStatus =
  | { state: "notInstalled" }
  | { state: "notRunning" }
  | { state: "running"; models: string[] };

export interface ChatTurn {
  role: "user" | "assistant";
  text: string;
  /** The user shared their screen with this message. */
  looked: boolean;
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
