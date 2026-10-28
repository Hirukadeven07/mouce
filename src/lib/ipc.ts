// The only module that talks to @tauri-apps/api.
import { Channel, invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type {
  ChatReply,
  ChatTurn,
  HomeTabId,
  OllamaStatus,
  ProviderId,
  PullProgress,
  Selection,
  Settings,
  MouceState,
} from "./types";

export type { UnlistenFn };

export function windowLabel(): string {
  return getCurrentWebviewWindow().label;
}

export function getSettings(): Promise<Settings> {
  return invoke<Settings>("get_settings");
}

export function mouceClicked(): Promise<void> {
  return invoke<void>("mouce_clicked");
}

export function startDragging(): Promise<void> {
  return getCurrentWindow().startDragging();
}

export function onMouceState(cb: (state: MouceState) => void): Promise<UnlistenFn> {
  return listen<MouceState>("mouce-state", (e) => cb(e.payload));
}

// The tab the home window should open on, set by Rust before the page loads (see
// `mouce::open_home`). Anything unexpected falls back to Home.
export function initialHomeTab(): HomeTabId | undefined {
  return (window as { __MOUCE_HOME_TAB?: HomeTabId }).__MOUCE_HOME_TAB;
}

export function onHomeTab(cb: (tab: HomeTabId) => void): Promise<UnlistenFn> {
  return listen<HomeTabId>("home-tab", (e) => cb(e.payload));
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

export function openChat(): Promise<void> {
  return invoke<void>("open_chat");
}

export function openHome(): Promise<void> {
  return invoke<void>("open_home");
}

/** Moves the chat into the home window, full size, closing the panel. */
export function openHomeChat(): Promise<void> {
  return invoke<void>("open_home_chat");
}

export function closeChatPanel(): Promise<void> {
  return invoke<void>("close_chat_panel");
}

export function chatHistory(): Promise<ChatTurn[]> {
  return invoke<ChatTurn[]>("chat_history");
}

// Asks mouce `text`, taking a picture of the screen first when `look` is set. Each piece
// of the answer arrives in `onDelta` as it is written.
export function chatSend(
  text: string,
  look: boolean,
  onDelta: (chunk: string) => void,
): Promise<ChatReply> {
  const channel = new Channel<string>();
  channel.onmessage = onDelta;
  return invoke<ChatReply>("chat_send", { text, look, onDelta: channel });
}

export function chatStop(): Promise<void> {
  return invoke<void>("chat_stop");
}

export function chatClear(): Promise<void> {
  return invoke<void>("chat_clear");
}

// The circled part of the screen waiting to be asked about, as a small preview image.
export function chatAttachment(): Promise<string | null> {
  return invoke<string | null>("chat_attachment");
}

export function chatDiscardAttachment(): Promise<void> {
  return invoke<void>("chat_discard_attachment");
}

export function onChatAttachment(cb: (preview: string | null) => void): Promise<UnlistenFn> {
  return listen<string | null>("chat-attachment", (e) => cb(e.payload));
}

export function circleStart(): Promise<void> {
  return invoke<void>("circle_start");
}

// The frozen screen the circle layer shows, as an image URL.
export function circleFrame(): Promise<string> {
  return invoke<string>("circle_frame");
}

export function circleReady(): Promise<void> {
  return invoke<void>("circle_ready");
}

export function circleFinish(selection: Selection): Promise<void> {
  return invoke<void>("circle_finish", { selection });
}

export function circleCancel(): Promise<void> {
  return invoke<void>("circle_cancel");
}

export function onPullProgress(cb: (progress: PullProgress) => void): Promise<UnlistenFn> {
  return listen<PullProgress>("ollama-pull-progress", (e) => cb(e.payload));
}
