// Mirrors the Rust types in src-tauri/src (settings.rs, ollama_setup.rs).

export type ProviderId = "ollama" | "openai" | "groq" | "gemini" | "anthropic";

export type StarSize = "small" | "medium" | "large";

export function starSizePx(size: StarSize): number {
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
  star_size: StarSize;
  star_rotation: boolean;
  star_position: [number, number] | null;
  circle_hotkey: string;
  history_enabled: boolean;
  fallback_to_local: boolean;
  first_run_done: boolean;
}

export type StarState = "idle" | "thinking" | "looking" | "reminder";

export type OllamaStatus =
  | { state: "notInstalled" }
  | { state: "notRunning" }
  | { state: "running"; models: string[] };

export interface PullProgress {
  status: string;
  completed: number | null;
  total: number | null;
}
