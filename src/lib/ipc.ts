// The only module that talks to @tauri-apps/api.
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { OllamaStatus, ProviderId, PullProgress, Settings, StarState } from "./types";

export type { UnlistenFn };

export function windowLabel(): string {
  return getCurrentWebviewWindow().label;
}

export function getSettings(): Promise<Settings> {
  return invoke<Settings>("get_settings");
}

export function starClicked(): Promise<void> {
  return invoke<void>("star_clicked");
}

export function startDragging(): Promise<void> {
  return getCurrentWindow().startDragging();
}

export function onStarState(cb: (state: StarState) => void): Promise<UnlistenFn> {
  return listen<StarState>("star-state", (e) => cb(e.payload));
}

export function onSettingsChanged(cb: (settings: Settings) => void): Promise<UnlistenFn> {
  return listen<Settings>("settings-changed", (e) => cb(e.payload));
}

export function updateSettings(settings: Settings): Promise<Settings> {
  return invoke<Settings>("update_settings", { settings });
}

// Write-only on purpose: the webview can save or remove a key, never read one back.
export function setApiKey(provider: ProviderId, key: string): Promise<void> {
  return invoke<void>("set_api_key", { provider, key });
}

export function hasApiKey(provider: ProviderId): Promise<boolean> {
  return invoke<boolean>("has_api_key", { provider });
}

export function clearApiKey(provider: ProviderId): Promise<void> {
  return invoke<void>("clear_api_key", { provider });
}

export function listModels(provider: ProviderId): Promise<string[]> {
  return invoke<string[]>("list_models", { provider });
}

export function testProvider(provider: ProviderId): Promise<string> {
  return invoke<string>("test_provider", { provider });
}

export function ollamaStatus(): Promise<OllamaStatus> {
  return invoke<OllamaStatus>("ollama_status");
}

export function pullOllamaModel(model: string): Promise<void> {
  return invoke<void>("pull_ollama_model", { model });
}

export function openOllamaDownload(): Promise<void> {
  return invoke<void>("open_ollama_download");
}

export function getAutostart(): Promise<boolean> {
  return invoke<boolean>("get_autostart");
}

export function setAutostart(enabled: boolean): Promise<void> {
  return invoke<void>("set_autostart", { enabled });
}

export function finishFirstRun(): Promise<void> {
  return invoke<void>("finish_first_run");
}

export function onPullProgress(cb: (progress: PullProgress) => void): Promise<UnlistenFn> {
  return listen<PullProgress>("ollama-pull-progress", (e) => cb(e.payload));
}
