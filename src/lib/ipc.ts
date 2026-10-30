// The only module that talks to @tauri-apps/api.
import { Channel, invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type {
  AlertAction,
  CalendarDraft,
  CalendarInput,
  CalendarItem,
  CalendarKind,
  ChatReply,
  ChatTurn,
  HomeTabId,
  OllamaStatus,
  ProviderId,
  PullProgress,
  Selection,
  Settings,
  MouceState,
  Today,
  WordCard,
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

// Asks mouce `text`, taking a picture of the screen first when `look` is set, or with the
// user's plans from the calendar when `plans` is set. Each piece of the answer arrives in
// `onDelta` as it is written.
export function chatSend(
  text: string,
  look: boolean,
  plans: boolean,
  onDelta: (chunk: string) => void,
): Promise<ChatReply> {
  const channel = new Channel<string>();
  channel.onmessage = onDelta;
  return invoke<ChatReply>("chat_send", { text, look, plans, onDelta: channel });
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

// A question left by "Chat about this" on the word card while the chat was closed.
export function chatTakeDraft(): Promise<string | null> {
  return invoke<string | null>("chat_take_draft");
}

export function onChatDraft(cb: (text: string) => void): Promise<UnlistenFn> {
  return listen<string>("chat-draft", (e) => cb(e.payload));
}

export function dictionaryCurrent(): Promise<WordCard | null> {
  return invoke<WordCard | null>("dictionary_current");
}

export function onDictionaryCard(cb: (card: WordCard) => void): Promise<UnlistenFn> {
  return listen<WordCard>("dictionary-card", (e) => cb(e.payload));
}

// The card has laid out card `id` at `height` CSS pixels; Rust sizes and shows its window.
export function dictionaryFit(id: number, height: number): Promise<void> {
  return invoke<void>("dictionary_fit", { id, height });
}

export function dictionaryClose(id: number): Promise<void> {
  return invoke<void>("dictionary_close", { id });
}

export function dictionaryChat(id: number): Promise<void> {
  return invoke<void>("dictionary_chat", { id });
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

// Events and reminders on the days `from` to `to` ("YYYY-MM-DD", both included).
export function calendarBetween(from: string, to: string): Promise<CalendarItem[]> {
  return invoke<CalendarItem[]>("calendar_between", { from, to });
}

export function calendarReminders(): Promise<CalendarItem[]> {
  return invoke<CalendarItem[]>("calendar_reminders");
}

// Adds an event or reminder, or changes `id` when it is given.
export function calendarSave(id: number | null, input: CalendarInput): Promise<CalendarItem> {
  return invoke<CalendarItem>("calendar_save", { id, input });
}

// Leaves day `date` out of repeating event `id` ("just this day").
export function calendarSkipDay(id: number, date: string): Promise<void> {
  return invoke<void>("calendar_skip_day", { id, date });
}

// Changes day `date` of repeating event `id` on its own; returns the new event for that day.
export function calendarChangeDay(id: number, date: string, input: CalendarInput): Promise<CalendarItem> {
  return invoke<CalendarItem>("calendar_change_day", { id, date, input });
}

export function calendarDelete(id: number): Promise<void> {
  return invoke<void>("calendar_delete", { id });
}

export function calendarSetDone(id: number, done: boolean): Promise<void> {
  return invoke<void>("calendar_set_done", { id, done });
}

export function calendarClearDone(): Promise<void> {
  return invoke<void>("calendar_clear_done");
}

// Asks the AI to read a note ("dentist tomorrow at 3pm") into a draft for the user to check.
export function calendarUnderstand(kind: CalendarKind, note: string, today: Today): Promise<CalendarDraft> {
  return invoke<CalendarDraft>("calendar_understand", { kind, note, today });
}

// Saved events or reminders changed, in this window or another.
export function onCalendarChanged(cb: () => void): Promise<UnlistenFn> {
  return listen<null>("calendar-changed", () => cb());
}

// What is due now, shown on the card beside the mouce.
export function alertsCurrent(): Promise<CalendarItem[]> {
  return invoke<CalendarItem[]>("alerts_current");
}

export function onAlerts(cb: (due: CalendarItem[]) => void): Promise<UnlistenFn> {
  return listen<CalendarItem[]>("calendar-alerts", (e) => cb(e.payload));
}

// The reminder card has laid out at `height` CSS pixels; Rust sizes and shows its window.
export function alertsFit(height: number): Promise<void> {
  return invoke<void>("alerts_fit", { height });
}

export function alertsAct(id: number, action: AlertAction): Promise<void> {
  return invoke<void>("alerts_act", { id, action });
}

// Opens the home window on the Calendar or Reminders tab, where card item `id` is.
export function alertsOpen(id: number): Promise<void> {
  return invoke<void>("alerts_open", { id });
}
