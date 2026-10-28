// Mirrors the Rust types in src-tauri/src (settings.rs, ollama_setup.rs).

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
  history_enabled: boolean;
  fallback_to_local: boolean;
  first_run_done: boolean;
}

export type MouceState = "idle" | "thinking" | "looking" | "reminder";

export type HomeTabId = "home" | "calendar" | "reminders" | "customize" | "settings";

export type OllamaStatus =
  | { state: "notInstalled" }
  | { state: "notRunning" }
  | { state: "running"; models: string[] };

export interface PullProgress {
  status: string;
  completed: number | null;
  total: number | null;
}
