// The only module that talks to @tauri-apps/api.
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { Settings, StarState } from "./types";

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
