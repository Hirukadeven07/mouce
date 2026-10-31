//! Tauri commands for the mouce, chat, home and first-run windows.
//!
//! Each command is a thin wrapper over a plain `*_impl` function so the logic can be
//! unit-tested without a running app. There is deliberately no command that returns an
//! API key: keys go in, and are only read inside Rust to talk to the chosen provider.

use crate::ai::{self, ChatMessage, ChatRequest, ImageInput, Role};
use crate::calendar::{self, alerts, understand};
use crate::chat::{self, Conversation, Snip, Turn};
use crate::ollama_setup::{self, OllamaStatus, PullProgress};
use crate::settings::{self, ProviderId, Settings};
use crate::{circle, dictionary, hotkey, mouce, screen, AppState};
use serde::Serialize;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_opener::OpenerExt as _;
use tokio::sync::Notify;

/// Event to the webview while a model downloads: payload is a `PullProgress`.
pub const PULL_PROGRESS_EVENT: &str = "ollama-pull-progress";
/// The only address the app ever opens in the browser.
const OLLAMA_DOWNLOAD_URL: &str = "https://ollama.com/download";

const TEST_SYSTEM: &str = "You are a connection test.";
const TEST_USER: &str = "Reply with exactly: ready";
const TEST_MAX_TOKENS: u32 = 16;

/// The longest question the chat accepts, far more than anyone types; it keeps a window
/// from handing the AI (and the saved history) an endless text.
pub const MAX_QUESTION_CHARS: usize = 20_000;
/// The longest model name, Ollama address or shortcut a setting may hold.
pub const MAX_SETTING_CHARS: usize = 200;

fn lock(state: &AppState) -> MutexGuard<'_, Settings> {
    state.settings.lock().unwrap_or_else(|e| e.into_inner())
}

/// Lock order: when both are needed, take the settings lock first, then the chat lock.
fn lock_chat(state: &AppState) -> MutexGuard<'_, Conversation> {
    state.chat.lock().unwrap_or_else(|e| e.into_inner())
}

/// Runs a closure when dropped: on normal exit, on an early return, when a future holding
/// it is dropped, and while unwinding from a panic.
struct OnDrop<F: FnOnce()>(Option<F>);

impl<F: FnOnce()> OnDrop<F> {
    fn new(f: F) -> OnDrop<F> {
        OnDrop(Some(f))
    }
}

impl<F: FnOnce()> Drop for OnDrop<F> {
    fn drop(&mut self) {
        if let Some(f) = self.0.take() {
            f();
        }
    }
}

/// For work that happens after a command already succeeded (its result is saved): a
/// failure is only reported, never turned into an error for the caller.
fn log_best_effort<E: std::fmt::Display>(
    what: &str,
    result: Result<(), E>,
    log: impl FnOnce(String),
) {
    if let Err(e) = result {
        log(format!("could not {what}: {e}"));
    }
}

/// The mouce was clicked (not dragged): open the chat beside it, or, when the chat is
/// already showing, the home window.
#[tauri::command]
pub fn mouce_clicked(app: AppHandle) {
    match mouce::click_target(mouce::chat_on_screen(&app)) {
        mouce::ClickTarget::Chat => mouce::open_chat(&app),
        mouce::ClickTarget::Home => mouce::open_home(&app, mouce::HomeTab::Home),
    }
}

// ---- settings ----

pub fn get_settings_impl(state: &AppState) -> Settings {
    lock(state).clone()
}

/// Saves `incoming` while holding the settings lock, so a concurrent writer (such as the
/// mouce-position save) can never be overwritten by a stale snapshot. The position and the
/// first-run flag belong to the backend; whatever the window sends for them is ignored.
/// Turning chat history on saves the chat so far; turning it off deletes the saved copy.
/// Returns the saved settings and whether the mouce size changed.
pub fn update_settings_impl(
    state: &AppState,
    incoming: Settings,
) -> Result<(Settings, bool), String> {
    let mut current = lock(state);
    let mut next = incoming;
    next.mouce_position = current.mouce_position;
    next.first_run_done = current.first_run_done;
    next.chat_hotkey = next.chat_hotkey.trim().to_string();
    next.circle_hotkey = next.circle_hotkey.trim().to_string();
    next.ollama_base_url = next.ollama_base_url.trim().to_string();
    check_setting_lengths(&next)?;
    check_ollama_address(&next.ollama_base_url)?;
    hotkey::check_distinct(&next.chat_hotkey, &next.circle_hotkey)?;
    let quiet_time = |t: &str| calendar::normal_time(t).ok_or("Pick a time for quiet hours");
    next.quiet_from = quiet_time(&next.quiet_from)?;
    next.quiet_to = quiet_time(&next.quiet_to)?;
    if next.history_enabled != current.history_enabled {
        let chat = lock_chat(state);
        let result = if next.history_enabled {
            chat::save_history(&state.store, chat.turns())
        } else {
            chat::forget_history(&state.store)
        };
        result.map_err(|e| e.to_string())?;
    }
    settings::save(&state.store, &next).map_err(|e| e.to_string())?;
    let size_changed = current.mouce_size != next.mouce_size;
    *current = next.clone();
    Ok((next, size_changed))
}

fn check_setting_lengths(s: &Settings) -> Result<(), String> {
    let too_long = |t: &str| t.chars().count() > MAX_SETTING_CHARS;
    if too_long(&s.chat_hotkey) || too_long(&s.circle_hotkey) {
        return Err("That shortcut is too long".to_string());
    }
    if too_long(&s.ollama_base_url) {
        return Err("That Ollama address is too long".to_string());
    }
    if s.models.values().any(|m| too_long(m)) {
        return Err("That model name is too long".to_string());
    }
    Ok(())
}

/// Ollama runs on this computer, and the chat and screen pictures go to it, so its address
/// must be `localhost` or `127.0.0.1` (blank means the default one).
fn check_ollama_address(address: &str) -> Result<(), String> {
    if address.is_empty() {
        return Ok(());
    }
    let ok = url::Url::parse(address)
        .is_ok_and(|u| matches!(u.scheme(), "http" | "https") && ai::net::is_loopback(&u));
    if ok {
        Ok(())
    } else {
        Err("Ollama's address must be on this computer, like http://localhost:11434".to_string())
    }
}

/// Moves each shortcut that changed (`(old, new)`) to its new keys. If one cannot move, the
/// ones already moved go back, and the error says why.
fn move_hotkeys(app: &AppHandle, moves: &[(String, String)]) -> Result<(), String> {
    for (i, (old, new)) in moves.iter().enumerate() {
        if let Err(e) = hotkey::replace(app, old, new) {
            undo_hotkeys(app, &moves[..i]);
            return Err(e);
        }
    }
    Ok(())
}

fn undo_hotkeys(app: &AppHandle, moves: &[(String, String)]) {
    for (old, new) in moves.iter().rev() {
        let _ = hotkey::replace(app, new, old);
    }
}

#[tauri::command(async)]
pub fn get_settings(state: State<'_, AppState>) -> Settings {
    get_settings_impl(&state)
}

#[tauri::command(async)]
pub fn update_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: Settings,
) -> Result<Settings, String> {
    hotkey::check_distinct(&settings.chat_hotkey, &settings.circle_hotkey)?;
    let current = get_settings_impl(&state);
    let glass_before = current.liquid_glass;
    let dictionary_before = current.dictionary_enabled;
    let moves: Vec<(String, String)> = [
        (current.chat_hotkey, settings.chat_hotkey.clone()),
        (current.circle_hotkey, settings.circle_hotkey.clone()),
    ]
    .into_iter()
    .filter(|(old, new)| old.trim() != new.trim())
    .collect();
    move_hotkeys(&app, &moves)?;
    let (saved, size_changed) = update_settings_impl(&state, settings).inspect_err(|_| {
        // Not saved, so the old shortcuts stay the ones that work.
        undo_hotkeys(&app, &moves);
    })?;
    let _ = app.emit(mouce::SETTINGS_CHANGED_EVENT, &saved);
    if size_changed {
        // The settings are already saved; a window that could not resize is not an error.
        let resized = mouce::resize(&app, saved.mouce_size);
        log_best_effort("resize the mouce after saving settings", resized, |m| log::warn!("{m}"));
    }
    if saved.liquid_glass != glass_before {
        mouce::set_glass(&app, saved.liquid_glass);
    }
    if saved.dictionary_enabled != dictionary_before {
        dictionary::set_enabled(&app, saved.dictionary_enabled);
    }
    Ok(saved)
}

// ---- API keys (write-only from the webview) ----

pub fn set_api_key_impl(state: &AppState, provider: ProviderId, key: &str) -> Result<(), String> {
    state.secrets.set_key(provider, key).map_err(|e| e.to_string())
}

pub fn has_api_key_impl(state: &AppState, provider: ProviderId) -> bool {
    state.secrets.has_key(provider)
}

pub fn clear_api_key_impl(state: &AppState, provider: ProviderId) -> Result<(), String> {
    state.secrets.clear_key(provider).map_err(|e| e.to_string())
}

#[tauri::command(async)]
pub fn set_api_key(
    state: State<'_, AppState>,
    provider: ProviderId,
    key: String,
) -> Result<(), String> {
    set_api_key_impl(&state, provider, &key)
}

#[tauri::command(async)]
pub fn has_api_key(state: State<'_, AppState>, provider: ProviderId) -> bool {
    has_api_key_impl(&state, provider)
}

#[tauri::command(async)]
pub fn clear_api_key(state: State<'_, AppState>, provider: ProviderId) -> Result<(), String> {
    clear_api_key_impl(&state, provider)
}

// ---- providers ----

/// The key is read here, inside Rust, and handed straight to the provider.
fn build_provider(
    state: &AppState,
    settings: &Settings,
    provider: ProviderId,
) -> Result<Box<dyn ai::Provider>, String> {
    let (base_url, key) = if provider == ProviderId::Ollama {
        (Some(settings.ollama_base_url.clone()), None)
    } else {
        (None, state.secrets.get_key(provider).map_err(|e| e.to_string())?)
    };
    ai::provider_for(provider, base_url, key).map_err(|e| e.to_string())
}

fn model_for(settings: &Settings, provider: ProviderId) -> Result<String, String> {
    match settings.models.get(&provider).map(|m| m.trim()).filter(|m| !m.is_empty()) {
        Some(m) => Ok(m.to_string()),
        None if provider == ProviderId::Ollama => Ok(ollama_setup::DEFAULT_LOCAL_MODEL.to_string()),
        None => Err("Pick a model in Settings first".to_string()),
    }
}

pub async fn list_models_impl(
    state: &AppState,
    provider: ProviderId,
) -> Result<Vec<String>, String> {
    let settings = get_settings_impl(state);
    let provider = build_provider(state, &settings, provider)?;
    provider.list_models().await.map_err(|e| e.to_string())
}

fn connection_test_request(model: String) -> ChatRequest {
    ChatRequest {
        model,
        system: Some(TEST_SYSTEM.to_string()),
        messages: vec![ChatMessage {
            role: Role::User,
            text: TEST_USER.to_string(),
            images: Vec::new(),
        }],
        max_tokens: TEST_MAX_TOKENS,
    }
}

pub async fn test_provider_impl(state: &AppState, provider: ProviderId) -> Result<String, String> {
    let settings = get_settings_impl(state);
    let model = model_for(&settings, provider)?;
    let provider = build_provider(state, &settings, provider)?;
    let reply = provider
        .chat_stream(&connection_test_request(model), &mut |_: &str| {})
        .await
        .map_err(|e| e.to_string())?;
    Ok(reply.trim().to_string())
}

#[tauri::command]
pub async fn list_models(
    state: State<'_, AppState>,
    provider: ProviderId,
) -> Result<Vec<String>, String> {
    list_models_impl(&state, provider).await
}

#[tauri::command]
pub async fn test_provider(
    app: AppHandle,
    state: State<'_, AppState>,
    provider: ProviderId,
) -> Result<String, String> {
    let _ = app.emit(mouce::MOUCE_STATE_EVENT, "thinking");
    // Back to idle however this ends: finished, failed, cancelled or panicked.
    let _idle = OnDrop::new(|| {
        let _ = app.emit(mouce::MOUCE_STATE_EVENT, "idle");
    });
    test_provider_impl(&state, provider).await
}

// ---- chat ----

/// How an answer ended. `text` is the whole answer (or what was written before Stop).
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct ChatReply {
    pub text: String,
    /// The user pressed Stop before the answer was finished.
    pub stopped: bool,
    /// The chosen online AI failed, so the local one answered instead.
    pub used_local: bool,
}

/// Saves the chat when history is on. Holding the settings lock means turning history off
/// at the same moment cannot leave a saved copy behind.
fn persist_chat(state: &AppState) -> Result<(), String> {
    let settings = lock(state);
    if !settings.history_enabled {
        return Ok(());
    }
    let chat = lock_chat(state);
    chat::save_history(&state.store, chat.turns()).map_err(|e| e.to_string())
}

/// Builds a request for the model the user picked.
pub type RequestFor<'a> = &'a (dyn Fn(String) -> ChatRequest + Sync);

/// Asks `provider` the request `request` makes for its model, streaming the answer to
/// `on_chunk`.
async fn ask(
    state: &AppState,
    settings: &Settings,
    provider: ProviderId,
    request: RequestFor<'_>,
    on_chunk: &mut (dyn for<'a> FnMut(&'a str) + Send),
) -> Result<String, String> {
    let request = request(model_for(settings, provider)?);
    let provider = build_provider(state, settings, provider)?;
    provider.chat_stream(&request, on_chunk).await.map_err(|e| e.to_string())
}

/// Asks the chosen AI. If an online AI fails before writing anything and fallback is on,
/// asks the local one instead. Returns the answer and whether the local AI gave it.
pub async fn ask_with_fallback(
    state: &AppState,
    settings: &Settings,
    request: RequestFor<'_>,
    on_chunk: &mut (dyn for<'a> FnMut(&'a str) + Send),
) -> Result<(String, bool), String> {
    let mut wrote = false;
    let first = {
        let mut track = |d: &str| {
            wrote = true;
            on_chunk(d);
        };
        ask(state, settings, settings.provider, request, &mut track).await
    };
    match first {
        Ok(text) => Ok((text, false)),
        Err(e)
            if !wrote && settings.provider != ProviderId::Ollama && settings.fallback_to_local =>
        {
            log::warn!(
                "{:?} could not answer, asking the local AI instead: {e}",
                settings.provider
            );
            match ask(state, settings, ProviderId::Ollama, request, on_chunk).await {
                Ok(text) => Ok((text, true)),
                Err(local) => Err(format!("{e} (the local AI could not answer either: {local})")),
            }
        }
        Err(e) => Err(e),
    }
}

/// The circled area a question is about. If the question goes unanswered, the area goes
/// back to wait for the next question, unless the user has circled a new one meanwhile.
struct TakenSnip<'a> {
    slot: &'a Mutex<Option<Snip>>,
    snip: Option<Snip>,
}

impl<'a> TakenSnip<'a> {
    fn take(slot: &'a Mutex<Option<Snip>>) -> TakenSnip<'a> {
        let snip = slot.lock().unwrap_or_else(|e| e.into_inner()).take();
        TakenSnip { slot, snip }
    }

    fn none(slot: &'a Mutex<Option<Snip>>) -> TakenSnip<'a> {
        TakenSnip { slot, snip: None }
    }

    fn image(&self) -> Option<ImageInput> {
        self.snip.as_ref().map(|s| s.image.clone())
    }

    fn answered(&mut self) {
        self.snip = None;
    }
}

impl Drop for TakenSnip<'_> {
    fn drop(&mut self) {
        if let Some(snip) = self.snip.take() {
            self.slot.lock().unwrap_or_else(|e| e.into_inner()).get_or_insert(snip);
        }
    }
}

/// Adds the user's question (with a screen picture when `screen` is given, or else the
/// circled area waiting to be asked about) and streams
/// mouce's answer to `on_delta`. A failed question is taken back out of the chat so it
/// can be asked again; Stop keeps whatever was already written.
pub async fn chat_send_impl<F>(
    state: &AppState,
    text: &str,
    screen: Option<F>,
    on_delta: &mut (dyn FnMut(&str) + Send),
) -> Result<ChatReply, String>
where
    F: Future<Output = Result<ImageInput, String>> + Send,
{
    chat_send_with_plans(state, text, screen, None, on_delta).await
}

/// `chat_send_impl`, with the user's `plans` from the calendar going with a question about
/// them (unless the screen or a circled area goes instead).
pub async fn chat_send_with_plans<F>(
    state: &AppState,
    text: &str,
    screen: Option<F>,
    plans: Option<String>,
    on_delta: &mut (dyn FnMut(&str) + Send),
) -> Result<ChatReply, String>
where
    F: Future<Output = Result<ImageInput, String>> + Send,
{
    let Some(_slot) = BusySlot::try_take(&state.chat_active) else {
        return Err("mouce is still answering".to_string());
    };
    let mut circled = if screen.is_some() {
        TakenSnip::none(&state.circled)
    } else {
        TakenSnip::take(&state.circled)
    };
    let text = match text.trim() {
        "" if screen.is_some() => chat::LOOK_QUESTION.to_string(),
        "" if circled.snip.is_some() => chat::CIRCLE_QUESTION.to_string(),
        "" => return Err("Type a question first".to_string()),
        t if t.chars().count() > MAX_QUESTION_CHARS => {
            return Err(format!("Keep the question under {MAX_QUESTION_CHARS} characters"))
        }
        t => t.to_string(),
    };
    let stop = Arc::new(Notify::new());
    *state.chat_stop.lock().unwrap_or_else(|e| e.into_inner()) = Some(stop.clone());
    let _forget_stop = OnDrop::new(|| {
        *state.chat_stop.lock().unwrap_or_else(|e| e.into_inner()) = None;
    });

    let screen = match screen {
        Some(look) => Some(look.await?),
        None => None,
    };
    let settings = get_settings_impl(state);
    match (screen, circled.image(), plans) {
        (None, Some(image), _) => lock_chat(state).push_user_circled(text, image),
        (None, None, Some(plans)) => lock_chat(state).push_user_with_plans(text, plans),
        (screen, _, _) => lock_chat(state).push_user(text, screen),
    }

    let chat_request = |model: String| lock_chat(state).request(model);
    let mut written = String::new();
    let outcome = {
        let mut on_chunk = |d: &str| {
            written.push_str(d);
            on_delta(d);
        };
        tokio::select! {
            result = ask_with_fallback(state, &settings, &chat_request, &mut on_chunk) => Some(result),
            _ = stop.notified() => None,
        }
    };

    let reply = {
        let mut chat = lock_chat(state);
        match outcome {
            Some(Ok((text, used_local))) => {
                chat.push_assistant(text.clone());
                circled.answered();
                ChatReply { text, stopped: false, used_local }
            }
            Some(Err(e)) => {
                // The error only, never the question: chat text stays out of the log.
                log::warn!("the AI could not answer: {e}");
                chat.drop_unanswered();
                return Err(e);
            }
            None => {
                if written.trim().is_empty() {
                    chat.drop_unanswered();
                } else {
                    chat.push_assistant(written.clone());
                    circled.answered();
                }
                ChatReply { text: written, stopped: true, used_local: false }
            }
        }
    };
    // The answer is already shown; a history that could not be saved is not an error.
    log_best_effort("save the chat history", persist_chat(state), |m| log::warn!("{m}"));
    Ok(reply)
}

/// Stops the answer being written, if there is one.
pub fn chat_stop_impl(state: &AppState) {
    if let Some(stop) = state.chat_stop.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        stop.notify_one();
    }
}

/// Starts a new chat: forgets this one, and its saved copy when history is on.
pub fn chat_clear_impl(state: &AppState) -> Result<(), String> {
    if state.chat_active.load(Ordering::SeqCst) {
        return Err("Wait for mouce to finish answering".to_string());
    }
    let settings = lock(state);
    lock_chat(state).clear();
    if settings.history_enabled {
        chat::forget_history(&state.store).map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Takes a picture of the screen the mouce is on, for the AI to look at.
async fn look_at_screen(app: AppHandle) -> Result<ImageInput, String> {
    let area = mouce::screen_area(&app).ok_or("Could not find the screen to look at")?;
    // The home window can hold the chat full size. Leave it out of this one picture so the
    // AI sees what is behind it, as it does with the chat panel; without this it still works.
    let _home_hidden = match app.get_webview_window(mouce::HOME_LABEL) {
        Some(home) if home.is_visible().unwrap_or(false) => {
            let hidden = screen::CaptureExclusion::new(home);
            if hidden.is_ok() {
                // Windows applies the change on its next frame.
                tokio::time::sleep(std::time::Duration::from_millis(60)).await;
            }
            hidden.ok()
        }
        _ => None,
    };
    let image = tauri::async_runtime::spawn_blocking(move || screen::capture(area))
        .await
        .map_err(|e| e.to_string())??;
    let _ = app.emit(mouce::MOUCE_STATE_EVENT, "thinking");
    Ok(ImageInput { mime: "image/jpeg".to_string(), base64: image.base64 })
}

#[tauri::command(async)]
pub fn chat_history(state: State<'_, AppState>) -> Vec<Turn> {
    lock_chat(&state).turns().to_vec()
}

/// How many days of plans go with a question about them, today included.
const PLANS_DAYS: i64 = 14;

/// The user's plans for a question about them, when that is turned on in Settings.
fn plans_for_chat(state: &AppState) -> Option<String> {
    if !lock(state).calendar_in_chat {
        return None;
    }
    calendar::plans_text(&state.store, alerts::now_ms(), PLANS_DAYS)
        .inspect_err(|e| log::warn!("could not read the calendar for the chat: {e}"))
        .ok()
}

/// Asks mouce `text`, after looking at the screen when `look` is set, or with the user's
/// plans when `plans` is set (a question about them). Each piece of the answer is sent down
/// `on_delta` as it arrives.
#[tauri::command]
pub async fn chat_send(
    app: AppHandle,
    state: State<'_, AppState>,
    text: String,
    look: bool,
    plans: Option<bool>,
    on_delta: Channel<String>,
) -> Result<ChatReply, String> {
    let _ = app.emit(mouce::MOUCE_STATE_EVENT, if look { "looking" } else { "thinking" });
    // Back to idle however this ends: finished, failed, stopped or cancelled.
    let _idle = OnDrop::new(|| {
        let _ = app.emit(mouce::MOUCE_STATE_EVENT, "idle");
    });
    let screen = look.then(|| look_at_screen(app.clone()));
    let plans = if plans == Some(true) && !look { plans_for_chat(&state) } else { None };
    chat_send_with_plans(&state, &text, screen, plans, &mut |d| {
        let _ = on_delta.send(d.to_string());
    })
    .await
}

#[tauri::command]
pub fn chat_stop(state: State<'_, AppState>) {
    chat_stop_impl(&state);
}

#[tauri::command(async)]
pub fn chat_clear(state: State<'_, AppState>) -> Result<(), String> {
    chat_clear_impl(&state)
}

#[tauri::command]
pub fn open_chat(app: AppHandle) {
    mouce::open_chat(&app);
}

/// Opens the home window on its Home tab (the logo in the chat header).
#[tauri::command]
pub fn open_home(app: AppHandle) {
    mouce::open_home(&app, mouce::HomeTab::Home);
}

/// Moves the chat from the panel into the home window, full size.
#[tauri::command]
pub fn open_home_chat(app: AppHandle) {
    mouce::close_chat(&app);
    mouce::open_home(&app, mouce::HomeTab::Chat);
}

/// Closes the chat panel: the home window is showing the same chat full size.
#[tauri::command]
pub fn close_chat_panel(app: AppHandle) {
    mouce::close_chat(&app);
}

/// The preview of the circled area waiting to be asked about, if there is one.
#[tauri::command(async)]
pub fn chat_attachment(state: State<'_, AppState>) -> Option<String> {
    circle::attachment_impl(&state)
}

/// The question "Chat about this" left for a chat that was not open yet, if there is one.
#[tauri::command(async)]
pub fn chat_take_draft(state: State<'_, AppState>) -> Option<String> {
    state.chat_draft.lock().unwrap_or_else(|e| e.into_inner()).take()
}

#[tauri::command(async)]
pub fn chat_discard_attachment(app: AppHandle, state: State<'_, AppState>) {
    circle::discard_impl(&state);
    circle::announce_attachment(&app, None);
}

// ---- circle to look up ----

#[tauri::command]
pub fn circle_start(app: AppHandle) {
    circle::start(&app);
}

/// The frozen screen for the circling layer to show.
#[tauri::command(async)]
pub fn circle_frame(state: State<'_, AppState>) -> Result<String, String> {
    circle::frame_image_impl(&state)
}

/// The layer shows the frozen screen: bring it up over everything.
#[tauri::command]
pub fn circle_ready(app: AppHandle) -> Result<(), String> {
    let layer = app.get_webview_window(mouce::CIRCLE_LABEL).ok_or("The circle layer is gone")?;
    layer.show().map_err(|e| e.to_string())?;
    layer.set_focus().map_err(|e| e.to_string())
}

/// The user circled `selection`: keep that part, close the layer and open the chat to ask
/// about it.
#[tauri::command(async)]
pub fn circle_finish(
    app: AppHandle,
    state: State<'_, AppState>,
    selection: screen::Selection,
) -> Result<(), String> {
    let preview = circle::finish_impl(&state, selection)?;
    circle::close_layer(&app);
    mouce::open_chat(&app);
    circle::announce_attachment(&app, Some(preview));
    Ok(())
}

#[tauri::command]
pub fn circle_cancel(app: AppHandle) {
    circle::close_layer(&app);
}

// ---- dictionary ----

/// The word card on screen now, for its page when it loads.
#[tauri::command(async)]
pub fn dictionary_current(state: State<'_, AppState>) -> Option<dictionary::Card> {
    dictionary::current_impl(&state)
}

/// The card's page has laid out card `id` at `height` logical pixels: show it there.
#[tauri::command(async)]
pub fn dictionary_fit(app: AppHandle, id: u64, height: f64) -> Result<(), String> {
    dictionary::fit_card(&app, id, height)
}

#[tauri::command(async)]
pub fn dictionary_close(app: AppHandle, id: u64) {
    dictionary::close_card(&app, id);
}

/// "Chat about this": opens the chat with a question about the word on card `id`.
#[tauri::command(async)]
pub fn dictionary_chat(app: AppHandle, id: u64) -> Result<(), String> {
    dictionary::chat_about(&app, id)
}

// ---- calendar and reminders ----

/// Tells every window the saved events or reminders changed, so each shows them again.
fn calendar_changed(app: &AppHandle) {
    let _ = app.emit(alerts::CHANGED_EVENT, ());
}

pub fn calendar_between_impl(
    state: &AppState,
    from: &str,
    to: &str,
) -> Result<Vec<calendar::Item>, String> {
    if !calendar::valid_date(from) || !calendar::valid_date(to) {
        return Err("Those are not dates".to_string());
    }
    calendar::between(&state.store, from, to).map_err(|e| e.to_string())
}

/// Adds an event or reminder, or changes event or reminder `id`.
pub fn calendar_save_impl(
    state: &AppState,
    id: Option<i64>,
    input: calendar::Input,
    now: i64,
) -> Result<calendar::Item, String> {
    let input = calendar::check(input)?;
    let saved = match id {
        None => Some(calendar::add(&state.store, &input, now)),
        Some(id) => calendar::update(&state.store, id, &input, now).transpose(),
    };
    saved.ok_or("It is no longer in your calendar")?.map_err(|e| e.to_string())
}

/// Reads a note the user typed ("dentist tomorrow at 3pm") into a draft for them to check.
/// If the AI cannot be asked, or its answer cannot be read, the draft is the note itself.
pub async fn calendar_understand_impl(
    state: &AppState,
    kind: calendar::Kind,
    note: &str,
    today: understand::Today,
) -> Result<understand::Draft, String> {
    let note = understand::check_note(note)?;
    let today = today.check()?;
    let settings = get_settings_impl(state);
    let request = |model: String| understand::request(model, kind, &today, &note);
    match ask_with_fallback(state, &settings, &request, &mut |_: &str| {}).await {
        Ok((answer, _)) => Ok(understand::read_answer(&answer, &note)
            .unwrap_or_else(|| understand::Draft::as_typed(&note))),
        Err(e) => {
            // The error only, never the note: what the user plans stays out of the log.
            log::warn!("the AI could not read a calendar note: {e}");
            Ok(understand::Draft::as_typed(&note))
        }
    }
}

/// Events and reminders on the days `from` to `to` ("YYYY-MM-DD", both included).
#[tauri::command(async)]
pub fn calendar_between(
    state: State<'_, AppState>,
    from: String,
    to: String,
) -> Result<Vec<calendar::Item>, String> {
    calendar_between_impl(&state, &from, &to)
}

#[tauri::command(async)]
pub fn calendar_reminders(state: State<'_, AppState>) -> Result<Vec<calendar::Item>, String> {
    calendar::reminders(&state.store).map_err(|e| e.to_string())
}

/// Saves what the user checked: a new event or reminder, or a change to `id`.
#[tauri::command(async)]
pub fn calendar_save(
    app: AppHandle,
    state: State<'_, AppState>,
    id: Option<i64>,
    input: calendar::Input,
) -> Result<calendar::Item, String> {
    let saved = calendar_save_impl(&state, id, input, alerts::now_ms())?;
    // A changed reminder time reminds anew, so the card no longer shows the old one.
    alerts::forget(&app, saved.id);
    calendar_changed(&app);
    Ok(saved)
}

#[tauri::command(async)]
pub fn calendar_delete(app: AppHandle, state: State<'_, AppState>, id: i64) -> Result<(), String> {
    calendar::delete(&state.store, id).map_err(|e| e.to_string())?;
    alerts::forget(&app, id);
    calendar_changed(&app);
    Ok(())
}

#[tauri::command(async)]
pub fn calendar_set_done(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
    done: bool,
) -> Result<(), String> {
    calendar::set_done(&state.store, id, done, alerts::now_ms())
        .map_err(|e| e.to_string())?
        .ok_or("That reminder is gone")?;
    if done {
        alerts::forget(&app, id);
    }
    calendar_changed(&app);
    Ok(())
}

/// Leaves day `date` out of repeating event `id` ("just this day").
#[tauri::command(async)]
pub fn calendar_skip_day(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
    date: String,
) -> Result<(), String> {
    calendar::skip_day(&state.store, id, &date, alerts::now_ms())
        .map_err(|e| e.to_string())?
        .ok_or("That is no longer in your calendar")?;
    alerts::forget(&app, id);
    calendar_changed(&app);
    Ok(())
}

/// Changes day `date` of repeating event `id` on its own, to what `input` says.
pub fn calendar_change_day_impl(
    state: &AppState,
    id: i64,
    date: &str,
    input: calendar::Input,
    now: i64,
) -> Result<calendar::Item, String> {
    let input = calendar::check(input)?;
    calendar::change_day(&state.store, id, date, &input, now)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "That is no longer in your calendar".to_string())
}

#[tauri::command(async)]
pub fn calendar_change_day(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
    date: String,
    input: calendar::Input,
) -> Result<calendar::Item, String> {
    let changed = calendar_change_day_impl(&state, id, &date, input, alerts::now_ms())?;
    alerts::forget(&app, id);
    calendar_changed(&app);
    Ok(changed)
}

/// Removes every reminder that has been done.
#[tauri::command(async)]
pub fn calendar_clear_done(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    calendar::clear_done(&state.store).map_err(|e| e.to_string())?;
    calendar_changed(&app);
    Ok(())
}

#[tauri::command]
pub async fn calendar_understand(
    app: AppHandle,
    state: State<'_, AppState>,
    kind: calendar::Kind,
    note: String,
    today: understand::Today,
) -> Result<understand::Draft, String> {
    let _ = app.emit(mouce::MOUCE_STATE_EVENT, "thinking");
    // Back to idle however this ends: finished, failed or cancelled.
    let _idle = OnDrop::new(|| {
        let _ = app.emit(mouce::MOUCE_STATE_EVENT, "idle");
    });
    calendar_understand_impl(&state, kind, &note, today).await
}

/// What is due now, for the reminder card and the mouce when their pages load.
#[tauri::command(async)]
pub fn alerts_current(state: State<'_, AppState>) -> Vec<calendar::Item> {
    alerts::current_impl(&state)
}

/// The reminder card has laid out at `height` logical pixels: show it beside the mouce.
#[tauri::command(async)]
pub fn alerts_fit(app: AppHandle, height: f64) -> Result<(), String> {
    alerts::fit_card(&app, height)
}

/// Opens the home window where reminder `id` on the card is.
#[tauri::command(async)]
pub fn alerts_open(app: AppHandle, id: i64) {
    alerts::open(&app, id);
}

/// Snooze, done or "got it" for reminder `id` on the card.
#[tauri::command(async)]
pub fn alerts_act(app: AppHandle, id: i64, action: alerts::Action) -> Result<(), String> {
    alerts::act(&app, id, action)
}

// ---- Ollama ----

fn ollama_exe_hint_in(local_app_data: Option<String>) -> PathBuf {
    match local_app_data {
        Some(dir) => PathBuf::from(dir).join("Programs").join("Ollama").join("ollama.exe"),
        None => PathBuf::new(),
    }
}

pub async fn ollama_status_impl(state: &AppState, exe_hint: &Path) -> OllamaStatus {
    let base_url = get_settings_impl(state).ollama_base_url;
    ollama_setup::status(&base_url, exe_hint).await
}

/// Holds a "this is running" flag (a model download, a chat answer); releases it when
/// dropped.
struct BusySlot<'a>(&'a AtomicBool);

impl<'a> BusySlot<'a> {
    fn try_take(flag: &'a AtomicBool) -> Option<BusySlot<'a>> {
        flag.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .ok()
            .map(|_| BusySlot(flag))
    }
}

impl Drop for BusySlot<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

pub async fn pull_ollama_model_impl(
    state: &AppState,
    model: &str,
    on_progress: &mut (dyn FnMut(PullProgress) + Send),
) -> Result<(), String> {
    let model = model.trim();
    if model.is_empty() {
        return Err("Choose a model to download".to_string());
    }
    if model.chars().count() > MAX_SETTING_CHARS {
        return Err("That model name is too long".to_string());
    }
    let Some(_slot) = BusySlot::try_take(&state.pull_active) else {
        return Err("A download is already in progress".to_string());
    };
    let base_url = get_settings_impl(state).ollama_base_url;
    ollama_setup::pull(&base_url, model, on_progress).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn ollama_status(state: State<'_, AppState>) -> Result<OllamaStatus, String> {
    let hint = ollama_exe_hint_in(std::env::var("LOCALAPPDATA").ok());
    Ok(ollama_status_impl(&state, &hint).await)
}

#[tauri::command]
pub async fn pull_ollama_model(
    app: AppHandle,
    state: State<'_, AppState>,
    model: String,
) -> Result<(), String> {
    pull_ollama_model_impl(&state, &model, &mut |p| {
        let _ = app.emit(PULL_PROGRESS_EVENT, p);
    })
    .await
}

/// Opens the fixed Ollama download page in the default browser. Takes no URL on purpose.
#[tauri::command]
pub fn open_ollama_download(app: AppHandle) -> Result<(), String> {
    app.opener().open_url(OLLAMA_DOWNLOAD_URL, None::<&str>).map_err(|e| e.to_string())
}

// ---- startup and first run ----

#[tauri::command(async)]
pub fn get_autostart(app: AppHandle) -> Result<bool, String> {
    app.autolaunch().is_enabled().map_err(|e| e.to_string())
}

#[tauri::command(async)]
pub fn set_autostart(app: AppHandle, enabled: bool) -> Result<(), String> {
    let launch = app.autolaunch();
    if enabled { launch.enable() } else { launch.disable() }.map_err(|e| e.to_string())
}

pub fn finish_first_run_impl(state: &AppState) -> Result<Settings, String> {
    let mut current = lock(state);
    let mut next = current.clone();
    next.first_run_done = true;
    settings::save(&state.store, &next).map_err(|e| e.to_string())?;
    *current = next.clone();
    Ok(next)
}

#[tauri::command(async)]
pub fn finish_first_run(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let saved = finish_first_run_impl(&state)?;
    let _ = app.emit(mouce::SETTINGS_CHANGED_EVENT, &saved);
    if let Some(window) = app.get_webview_window(mouce::FIRSTRUN_LABEL) {
        // First run is already saved as done; a window that would not close is not an error.
        log_best_effort("close the first-run window", window.close(), |m| log::warn!("{m}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ollama_setup::OllamaStatus;
    use crate::settings::{MouceSize, ProviderId, Settings};
    use crate::store::AppStore;
    use crate::AppState;
    use std::panic::{catch_unwind, AssertUnwindSafe};
    use std::path::Path;
    use std::sync::atomic::Ordering;
    use std::sync::Arc;
    use std::time::Duration;
    use wiremock::matchers::{body_partial_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn state_with(settings: Settings, service: &str) -> (tempfile::TempDir, AppState) {
        let dir = tempfile::tempdir().unwrap();
        let store = AppStore::open(dir.path().to_path_buf()).unwrap();
        let state = AppState::new(store, settings, crate::secrets::Secrets::new(service));
        (dir, state)
    }

    fn state(settings: Settings) -> (tempfile::TempDir, AppState) {
        state_with(settings, "com.mouce.app.test.commands.unused")
    }

    fn state_for(server: &MockServer) -> (tempfile::TempDir, AppState) {
        state(Settings { ollama_base_url: server.uri(), ..Default::default() })
    }

    struct Cleanup(AppState);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = self.0.secrets.clear_key(ProviderId::Groq);
        }
    }

    #[test]
    fn get_settings_returns_current() {
        let s = Settings { mouce_rotation: false, ..Default::default() };
        let (_d, st) = state(s.clone());
        assert_eq!(get_settings_impl(&st), s);
    }

    #[test]
    fn update_settings_persists_and_returns_new_settings() {
        let (_d, st) = state(Settings::default());
        let next = Settings {
            mouce_size: MouceSize::Large,
            mouce_rotation: false,
            history_enabled: true,
            ..Default::default()
        };
        let (saved, size_changed) = update_settings_impl(&st, next.clone()).unwrap();
        assert_eq!(saved, next);
        assert!(size_changed);
        assert_eq!(crate::settings::load(&st.store), next);
        assert_eq!(get_settings_impl(&st), next);

        let (_, again) = update_settings_impl(&st, next).unwrap();
        assert!(!again, "same size is not a size change");
    }

    #[test]
    fn update_settings_keeps_backend_owned_fields() {
        let current =
            Settings { mouce_position: Some((5, 6)), first_run_done: true, ..Default::default() };
        let (_d, st) = state(current);
        let stale = Settings {
            mouce_position: Some((1, 1)),
            first_run_done: false,
            mouce_rotation: false,
            ..Default::default()
        };
        let (saved, _) = update_settings_impl(&st, stale).unwrap();
        assert_eq!(saved.mouce_position, Some((5, 6)));
        assert!(saved.first_run_done);
        assert!(!saved.mouce_rotation);
    }

    #[test]
    fn update_settings_trims_the_chat_shortcut_and_refuses_a_bad_one() {
        let (_d, st) = state(Settings::default());
        let spaced = Settings { chat_hotkey: "  Ctrl+Alt+K ".into(), ..Default::default() };
        let (saved, _) = update_settings_impl(&st, spaced).unwrap();
        assert_eq!(saved.chat_hotkey, "Ctrl+Alt+K");

        let bare = Settings { chat_hotkey: "K".into(), ..Default::default() };
        assert!(update_settings_impl(&st, bare).unwrap_err().contains("needs Ctrl, Alt"));
        assert_eq!(crate::settings::load(&st.store).chat_hotkey, "Ctrl+Alt+K", "not saved");
    }

    #[test]
    fn update_settings_saves_while_holding_the_lock() {
        // If saving happened after the lock was released, concurrent updates could
        // leave the database holding a different value than memory.
        for _ in 0..20 {
            let (_d, st) = state(Settings::default());
            let st = Arc::new(st);
            let handles: Vec<_> = (0..8)
                .map(|i| {
                    let st = st.clone();
                    std::thread::spawn(move || {
                        let s =
                            Settings { circle_hotkey: format!("Ctrl+{i}"), ..Default::default() };
                        update_settings_impl(&st, s).unwrap();
                    })
                })
                .collect();
            for h in handles {
                h.join().unwrap();
            }
            assert_eq!(crate::settings::load(&st.store), get_settings_impl(&st));
        }
    }

    #[test]
    fn api_key_commands_trim_reject_blank_and_clear() {
        let _lock = crate::secrets::test_support::lock_credential_store();
        let service = "com.mouce.app.test.commands.api_key";
        let (_d, st) = state_with(Settings::default(), service);
        let g = Cleanup(st);
        let st = &g.0;
        set_api_key_impl(st, ProviderId::Groq, "  gsk_abc \n").unwrap();
        assert!(has_api_key_impl(st, ProviderId::Groq));
        assert_eq!(st.secrets.get_key(ProviderId::Groq).unwrap().as_deref(), Some("gsk_abc"));
        let err = set_api_key_impl(st, ProviderId::Groq, "   ").unwrap_err();
        assert_eq!(err, "API key is blank");
        assert!(has_api_key_impl(st, ProviderId::Groq));
        clear_api_key_impl(st, ProviderId::Groq).unwrap();
        assert!(!has_api_key_impl(st, ProviderId::Groq));
    }

    #[test]
    fn connection_test_request_is_exact() {
        let req = connection_test_request("m1".into());
        assert_eq!(req.model, "m1");
        assert_eq!(req.system.as_deref(), Some("You are a connection test."));
        assert_eq!(req.max_tokens, 16);
        assert_eq!(req.messages.len(), 1);
        assert_eq!(req.messages[0].role, crate::ai::Role::User);
        assert_eq!(req.messages[0].text, "Reply with exactly: ready");
        assert!(req.messages[0].images.is_empty());
    }

    fn ndjson_reply(text: &str) -> String {
        format!("{{\"message\":{{\"content\":\"{text}\"}},\"done\":true}}\n")
    }

    #[tokio::test]
    async fn test_provider_sends_probe_and_returns_reply() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .and(body_partial_json(serde_json::json!({
                "model": "tiny:1b",
                "messages": [
                    { "role": "system", "content": "You are a connection test." },
                    { "role": "user", "content": "Reply with exactly: ready" }
                ]
            })))
            .respond_with(ResponseTemplate::new(200).set_body_string(ndjson_reply("ready")))
            .expect(1)
            .mount(&server)
            .await;
        let (_d, st) = state_for(&server);
        st.settings.lock().unwrap().models.insert(ProviderId::Ollama, "tiny:1b".into());
        assert_eq!(test_provider_impl(&st, ProviderId::Ollama).await.unwrap(), "ready");
    }

    #[tokio::test]
    async fn test_provider_ollama_falls_back_to_default_model() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .and(body_partial_json(serde_json::json!({ "model": "gemma3:4b" })))
            .respond_with(ResponseTemplate::new(200).set_body_string(ndjson_reply("ready")))
            .expect(1)
            .mount(&server)
            .await;
        let (_d, st) = state_for(&server);
        st.settings.lock().unwrap().models.clear();
        assert_eq!(test_provider_impl(&st, ProviderId::Ollama).await.unwrap(), "ready");
    }

    #[tokio::test]
    async fn test_provider_other_providers_need_a_model() {
        let (_d, st) = state(Settings::default());
        for id in [ProviderId::OpenAi, ProviderId::Groq, ProviderId::Gemini, ProviderId::Anthropic]
        {
            assert_eq!(
                test_provider_impl(&st, id).await.unwrap_err(),
                "Pick a model in Settings first",
                "{id:?}"
            );
        }
    }

    #[tokio::test]
    async fn test_provider_surfaces_http_error_text() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(ResponseTemplate::new(500).set_body_string(r#"{"error":"boom"}"#))
            .mount(&server)
            .await;
        let (_d, st) = state_for(&server);
        let err = test_provider_impl(&st, ProviderId::Ollama).await.unwrap_err();
        assert!(err.contains("500"), "{err}");
    }

    #[tokio::test]
    async fn list_models_uses_ollama_base_url() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/tags"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(r#"{"models":[{"name":"a:1"},{"name":"b:2"}]}"#),
            )
            .expect(1)
            .mount(&server)
            .await;
        let (_d, st) = state_for(&server);
        assert_eq!(list_models_impl(&st, ProviderId::Ollama).await.unwrap(), vec!["a:1", "b:2"]);
    }

    #[tokio::test]
    async fn ollama_status_reads_running_models() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/tags"))
            .respond_with(
                ResponseTemplate::new(200).set_body_string(r#"{"models":[{"name":"gemma3:4b"}]}"#),
            )
            .mount(&server)
            .await;
        let (_d, st) = state_for(&server);
        let s = ollama_status_impl(&st, Path::new("nope.exe")).await;
        assert_eq!(s, OllamaStatus::Running { models: vec!["gemma3:4b".into()] });
    }

    #[tokio::test]
    async fn pull_model_reports_progress_to_callback() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/pull"))
            .and(body_partial_json(serde_json::json!({ "model": "tiny:1b" })))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                "{\"status\":\"downloading\",\"completed\":1,\"total\":2}\n{\"status\":\"success\"}\n",
            ))
            .expect(1)
            .mount(&server)
            .await;
        let (_d, st) = state_for(&server);
        let mut seen = Vec::new();
        pull_ollama_model_impl(&st, "tiny:1b", &mut |p| seen.push(p)).await.unwrap();
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[0].completed, Some(1));
        assert_eq!(seen[1].status, "success");
    }

    const PULL_OK: &str = "{\"status\":\"success\"}\n";

    async fn mount_pull(server: &MockServer, response: ResponseTemplate, expected: u64) {
        Mock::given(method("POST"))
            .and(path("/api/pull"))
            .respond_with(response)
            .expect(expected)
            .mount(server)
            .await;
    }

    #[tokio::test]
    async fn second_pull_is_refused_while_one_is_running() {
        let server = MockServer::start().await;
        mount_pull(
            &server,
            ResponseTemplate::new(200)
                .set_body_string(PULL_OK)
                .set_delay(Duration::from_millis(600)),
            1,
        )
        .await;
        let (_d, st) = state_for(&server);
        let mut on_first = |_| {};
        let first = pull_ollama_model_impl(&st, "tiny:1b", &mut on_first);
        let second = async {
            tokio::time::sleep(Duration::from_millis(150)).await;
            pull_ollama_model_impl(&st, "other:1b", &mut |_| {}).await
        };
        let (first, second) = tokio::join!(first, second);
        assert_eq!(first, Ok(()));
        assert_eq!(second, Err("A download is already in progress".to_string()));
        // `expect(1)` on the mock proves the refused pull never sent a request.
    }

    #[tokio::test]
    async fn pull_flag_is_released_after_success_and_after_error() {
        let server = MockServer::start().await;
        mount_pull(&server, ResponseTemplate::new(200).set_body_string(PULL_OK), 1).await;
        let (_d, st) = state_for(&server);
        pull_ollama_model_impl(&st, "tiny:1b", &mut |_| {}).await.unwrap();
        assert!(!st.pull_active.load(Ordering::SeqCst), "released after success");
        server.reset().await;

        mount_pull(&server, ResponseTemplate::new(500).set_body_string("boom"), 1).await;
        let err = pull_ollama_model_impl(&st, "tiny:1b", &mut |_| {}).await.unwrap_err();
        assert!(err.contains("500"), "{err}");
        assert!(!st.pull_active.load(Ordering::SeqCst), "released after error");
        server.reset().await;

        mount_pull(&server, ResponseTemplate::new(200).set_body_string(PULL_OK), 1).await;
        pull_ollama_model_impl(&st, "tiny:1b", &mut |_| {}).await.unwrap();
    }

    #[tokio::test]
    async fn pull_flag_is_released_when_the_pull_is_dropped() {
        let server = MockServer::start().await;
        mount_pull(
            &server,
            ResponseTemplate::new(200).set_body_string(PULL_OK).set_delay(Duration::from_secs(5)),
            1,
        )
        .await;
        let (_d, st) = state_for(&server);
        let cut_short = tokio::time::timeout(
            Duration::from_millis(200),
            pull_ollama_model_impl(&st, "tiny:1b", &mut |_| {}),
        )
        .await;
        assert!(cut_short.is_err(), "the pull should still be running when dropped");
        assert!(!st.pull_active.load(Ordering::SeqCst), "released when dropped");
    }

    #[tokio::test]
    async fn blank_model_is_rejected_before_any_request() {
        let server = MockServer::start().await;
        mount_pull(&server, ResponseTemplate::new(200).set_body_string(PULL_OK), 0).await;
        let (_d, st) = state_for(&server);
        for blank in ["", "   ", "\t\n"] {
            let err = pull_ollama_model_impl(&st, blank, &mut |_| {}).await.unwrap_err();
            assert_eq!(err, "Choose a model to download", "{blank:?}");
        }
        assert!(!st.pull_active.load(Ordering::SeqCst));
        assert!(server.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn model_name_is_trimmed_before_it_is_sent() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/pull"))
            .and(body_partial_json(serde_json::json!({ "model": "tiny:1b" })))
            .respond_with(ResponseTemplate::new(200).set_body_string(PULL_OK))
            .expect(1)
            .mount(&server)
            .await;
        let (_d, st) = state_for(&server);
        pull_ollama_model_impl(&st, "  tiny:1b \n", &mut |_| {}).await.unwrap();
    }

    #[test]
    fn on_drop_runs_on_normal_drop() {
        let ran = std::cell::Cell::new(0);
        {
            let _g = OnDrop::new(|| ran.set(ran.get() + 1));
            assert_eq!(ran.get(), 0, "not before the drop");
        }
        assert_eq!(ran.get(), 1);
    }

    #[test]
    fn on_drop_runs_while_unwinding_from_a_panic() {
        let ran = std::sync::atomic::AtomicBool::new(false);
        let result = catch_unwind(AssertUnwindSafe(|| {
            let _g = OnDrop::new(|| ran.store(true, Ordering::SeqCst));
            panic!("boom");
        }));
        assert!(result.is_err());
        assert!(ran.load(Ordering::SeqCst), "guard ran during the unwind");
    }

    #[test]
    fn failure_after_a_successful_save_is_logged_not_returned() {
        let mut logged = Vec::new();
        log_best_effort("resize the mouce", Err::<(), _>("window is gone"), |m| logged.push(m));
        assert_eq!(logged, vec!["could not resize the mouce: window is gone"]);

        let mut quiet = Vec::new();
        log_best_effort("resize the mouce", Ok::<(), String>(()), |m| quiet.push(m));
        assert!(quiet.is_empty());
    }

    #[test]
    fn finish_first_run_sets_flag_and_persists() {
        let s = Settings { mouce_rotation: false, ..Default::default() };
        let (_d, st) = state(s);
        let done = finish_first_run_impl(&st).unwrap();
        assert!(done.first_run_done);
        assert!(!done.mouce_rotation);
        assert!(crate::settings::load(&st.store).first_run_done);
        assert!(get_settings_impl(&st).first_run_done);
    }

    // ---- chat ----

    type Look = std::future::Ready<Result<ImageInput, String>>;

    fn no_look() -> Option<Look> {
        None
    }

    fn look() -> Option<Look> {
        Some(std::future::ready(Ok(ImageInput {
            mime: "image/jpeg".into(),
            base64: "U0NSRUVO".into(),
        })))
    }

    async fn mount_chat(server: &MockServer, response: ResponseTemplate) {
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(response)
            .mount(server)
            .await;
    }

    async fn sent_bodies(server: &MockServer) -> Vec<serde_json::Value> {
        server
            .received_requests()
            .await
            .unwrap()
            .iter()
            .map(|r| serde_json::from_slice(&r.body).unwrap())
            .collect()
    }

    fn two_chunks(a: &str, b: &str) -> String {
        let line = |text: &str, done: bool| {
            format!("{{\"message\":{{\"content\":\"{text}\"}},\"done\":{done}}}\n")
        };
        line(a, false) + &line(b, true)
    }

    #[tokio::test]
    async fn chat_streams_the_answer_and_remembers_both_sides() {
        let server = MockServer::start().await;
        let body = two_chunks("Hel", "lo");
        mount_chat(&server, ResponseTemplate::new(200).set_body_string(body)).await;
        let (_d, st) = state_for(&server);
        let mut chunks = Vec::new();
        let reply = chat_send_impl(&st, "  hi  ", no_look(), &mut |d| chunks.push(d.to_string()))
            .await
            .unwrap();
        assert_eq!(reply, ChatReply { text: "Hello".into(), stopped: false, used_local: false });
        assert_eq!(chunks, vec!["Hel", "lo"]);
        let turns = lock_chat(&st).turns().to_vec();
        assert_eq!(turns.iter().map(|t| t.text.as_str()).collect::<Vec<_>>(), vec!["hi", "Hello"]);

        let body = &sent_bodies(&server).await[0];
        assert_eq!(body["model"], "gemma3:4b");
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][0]["content"], chat::SYSTEM_PROMPT);
        assert_eq!(body["messages"][1]["content"], "hi");
    }

    #[tokio::test]
    async fn looking_sends_the_screen_with_a_default_question() {
        let server = MockServer::start().await;
        let body = ndjson_reply("A browser");
        mount_chat(&server, ResponseTemplate::new(200).set_body_string(body)).await;
        let (_d, st) = state_for(&server);
        chat_send_impl(&st, "", look(), &mut |_| {}).await.unwrap();
        let body = &sent_bodies(&server).await[0];
        assert_eq!(body["messages"][1]["content"], chat::LOOK_QUESTION);
        assert_eq!(body["messages"][1]["images"], serde_json::json!(["U0NSRUVO"]));
        assert!(lock_chat(&st).turns()[0].looked);
    }

    fn circle_something(st: &AppState) {
        *st.circled.lock().unwrap() = Some(Snip {
            image: ImageInput { mime: "image/jpeg".into(), base64: "Q0lSQ0xF".into() },
            thumbnail: "data:image/jpeg;base64,VEhVTUI=".into(),
        });
    }

    #[tokio::test]
    async fn a_circled_area_is_asked_about_once_with_a_default_question() {
        let server = MockServer::start().await;
        let body = ndjson_reply("A logo");
        mount_chat(&server, ResponseTemplate::new(200).set_body_string(body)).await;
        let (_d, st) = state_for(&server);
        circle_something(&st);
        chat_send_impl(&st, "", no_look(), &mut |_| {}).await.unwrap();
        let body = &sent_bodies(&server).await[0];
        assert_eq!(body["messages"][1]["content"], chat::CIRCLE_QUESTION);
        assert_eq!(body["messages"][1]["images"], serde_json::json!(["Q0lSQ0xF"]));
        assert!(lock_chat(&st).turns()[0].circled);
        assert!(st.circled.lock().unwrap().is_none(), "used up by the answer");
    }

    #[tokio::test]
    async fn a_circled_area_comes_back_when_the_question_fails() {
        let server = MockServer::start().await;
        mount_chat(&server, ResponseTemplate::new(500).set_body_string("boom")).await;
        let (_d, st) = state_for(&server);
        circle_something(&st);
        chat_send_impl(&st, "what is it?", no_look(), &mut |_| {}).await.unwrap_err();
        assert!(st.circled.lock().unwrap().is_some(), "still waiting to be asked about");
        assert!(lock_chat(&st).turns().is_empty());
    }

    #[tokio::test]
    async fn looking_at_the_whole_screen_leaves_a_circled_area_waiting() {
        let server = MockServer::start().await;
        let body = ndjson_reply("A desktop");
        mount_chat(&server, ResponseTemplate::new(200).set_body_string(body)).await;
        let (_d, st) = state_for(&server);
        circle_something(&st);
        chat_send_impl(&st, "", look(), &mut |_| {}).await.unwrap();
        let body = &sent_bodies(&server).await[0];
        assert_eq!(body["messages"][1]["images"], serde_json::json!(["U0NSRUVO"]));
        assert!(st.circled.lock().unwrap().is_some());
    }

    #[test]
    fn update_settings_refuses_one_shortcut_for_both_features() {
        let (_d, st) = state(Settings::default());
        let same = Settings {
            chat_hotkey: "Ctrl+Alt+M".into(),
            circle_hotkey: " ctrl+alt+m ".into(),
            ..Default::default()
        };
        assert!(update_settings_impl(&st, same).unwrap_err().contains("different shortcuts"));
        let spaced = Settings { circle_hotkey: " Ctrl+Alt+L ".into(), ..Default::default() };
        assert_eq!(update_settings_impl(&st, spaced).unwrap().0.circle_hotkey, "Ctrl+Alt+L");
    }

    #[tokio::test]
    async fn a_blank_question_is_refused_before_any_request() {
        let server = MockServer::start().await;
        let (_d, st) = state_for(&server);
        let err = chat_send_impl(&st, " \n ", no_look(), &mut |_| {}).await.unwrap_err();
        assert_eq!(err, "Type a question first");
        assert!(server.received_requests().await.unwrap().is_empty());
        assert!(!st.chat_active.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn an_endless_question_is_refused_before_any_request() {
        let server = MockServer::start().await;
        let (_d, st) = state_for(&server);
        let long = "a".repeat(MAX_QUESTION_CHARS + 1);
        let err = chat_send_impl(&st, &long, no_look(), &mut |_| {}).await.unwrap_err();
        assert!(err.contains("Keep the question under"), "{err}");
        assert!(server.received_requests().await.unwrap().is_empty());
        assert!(lock_chat(&st).turns().is_empty());
    }

    #[test]
    fn ollama_must_stay_on_this_computer() {
        let (_d, st) = state(Settings::default());
        for ok in ["", "http://localhost:11434", " http://127.0.0.1:9999 ", "https://localhost"] {
            let next = Settings { ollama_base_url: ok.into(), ..Default::default() };
            assert!(update_settings_impl(&st, next).is_ok(), "{ok:?} should be allowed");
        }
        for bad in [
            "http://192.168.1.5:11434",
            "https://evil.com",
            "http://localhost.evil.com",
            "file:///C:/x",
            "localhost:11434",
        ] {
            let next = Settings { ollama_base_url: bad.into(), ..Default::default() };
            let err = update_settings_impl(&st, next).unwrap_err();
            assert!(err.contains("on this computer"), "{bad:?}: {err}");
        }
        assert_eq!(lock(&st).ollama_base_url, "https://localhost", "refusals saved nothing");
    }

    #[test]
    fn overlong_settings_are_refused() {
        let (_d, st) = state(Settings::default());
        let long = "x".repeat(MAX_SETTING_CHARS + 1);
        let hotkey = Settings { chat_hotkey: long.clone(), ..Default::default() };
        assert!(update_settings_impl(&st, hotkey).unwrap_err().contains("shortcut"));
        let mut models = Settings::default();
        models.models.insert(ProviderId::OpenAi, long.clone());
        assert!(update_settings_impl(&st, models).unwrap_err().contains("model name"));
        let address =
            Settings { ollama_base_url: format!("http://localhost/{long}"), ..Default::default() };
        assert!(update_settings_impl(&st, address).unwrap_err().contains("Ollama address"));
    }

    #[tokio::test]
    async fn a_failed_question_is_taken_back_so_it_can_be_asked_again() {
        let server = MockServer::start().await;
        mount_chat(&server, ResponseTemplate::new(500).set_body_string("boom")).await;
        let (_d, st) = state_for(&server);
        let err = chat_send_impl(&st, "hi", look(), &mut |_| {}).await.unwrap_err();
        assert!(err.contains("500"), "{err}");
        assert!(lock_chat(&st).turns().is_empty());
        assert!(!st.chat_active.load(Ordering::SeqCst), "released after an error");
    }

    #[tokio::test]
    async fn a_failing_online_ai_falls_back_to_the_local_one() {
        let server = MockServer::start().await;
        let body = ndjson_reply("local hi");
        mount_chat(&server, ResponseTemplate::new(200).set_body_string(body)).await;
        let (_d, st) = state(Settings {
            provider: ProviderId::OpenAi,
            fallback_to_local: true,
            ollama_base_url: server.uri(),
            ..Default::default()
        });
        // No OpenAI model is chosen, so the online AI fails before writing anything.
        let reply = chat_send_impl(&st, "hi", no_look(), &mut |_| {}).await.unwrap();
        assert_eq!(reply, ChatReply { text: "local hi".into(), stopped: false, used_local: true });
    }

    #[tokio::test]
    async fn without_fallback_the_online_error_is_shown() {
        let server = MockServer::start().await;
        let (_d, st) = state(Settings {
            provider: ProviderId::OpenAi,
            fallback_to_local: false,
            ollama_base_url: server.uri(),
            ..Default::default()
        });
        let err = chat_send_impl(&st, "hi", no_look(), &mut |_| {}).await.unwrap_err();
        assert_eq!(err, "Pick a model in Settings first");
        assert!(server.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn when_both_fail_the_error_names_both() {
        let server = MockServer::start().await;
        mount_chat(&server, ResponseTemplate::new(500).set_body_string("down")).await;
        let (_d, st) = state(Settings {
            provider: ProviderId::OpenAi,
            fallback_to_local: true,
            ollama_base_url: server.uri(),
            ..Default::default()
        });
        let err = chat_send_impl(&st, "hi", no_look(), &mut |_| {}).await.unwrap_err();
        assert!(err.starts_with("Pick a model in Settings first"), "{err}");
        assert!(err.contains("the local AI could not answer either"), "{err}");
    }

    #[tokio::test]
    async fn stop_ends_the_answer_and_takes_back_an_unanswered_question() {
        let server = MockServer::start().await;
        mount_chat(
            &server,
            ResponseTemplate::new(200)
                .set_body_string(ndjson_reply("late"))
                .set_delay(Duration::from_secs(5)),
        )
        .await;
        let (_d, st) = state_for(&server);
        let stopper = async {
            tokio::time::sleep(Duration::from_millis(200)).await;
            chat_stop_impl(&st);
        };
        let mut quiet = |_: &str| {};
        let (reply, ()) = tokio::join!(chat_send_impl(&st, "hi", no_look(), &mut quiet), stopper);
        let stopped = ChatReply { text: String::new(), stopped: true, used_local: false };
        assert_eq!(reply.unwrap(), stopped);
        assert!(lock_chat(&st).turns().is_empty());
        assert!(st.chat_stop.lock().unwrap().is_none(), "nothing left to stop");
        assert!(!st.chat_active.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn a_second_question_waits_for_the_first_answer() {
        let server = MockServer::start().await;
        mount_chat(
            &server,
            ResponseTemplate::new(200)
                .set_body_string(ndjson_reply("one"))
                .set_delay(Duration::from_millis(500)),
        )
        .await;
        let (_d, st) = state_for(&server);
        let second = async {
            tokio::time::sleep(Duration::from_millis(100)).await;
            chat_send_impl(&st, "two", no_look(), &mut |_| {}).await
        };
        let mut quiet = |_: &str| {};
        let first = chat_send_impl(&st, "one", no_look(), &mut quiet);
        let (first, second) = tokio::join!(first, second);
        assert_eq!(first.unwrap().text, "one");
        assert_eq!(second.unwrap_err(), "mouce is still answering");
        assert_eq!(lock_chat(&st).turns().len(), 2);
    }

    #[tokio::test]
    async fn history_is_saved_only_while_it_is_turned_on() {
        let server = MockServer::start().await;
        mount_chat(&server, ResponseTemplate::new(200).set_body_string(ndjson_reply("hey"))).await;
        let (_d, st) = state_for(&server);
        chat_send_impl(&st, "hi", look(), &mut |_| {}).await.unwrap();
        assert!(chat::load_history(&st.store).is_empty(), "off by default");

        let on = Settings { history_enabled: true, ..get_settings_impl(&st) };
        update_settings_impl(&st, on.clone()).unwrap();
        assert_eq!(chat::load_history(&st.store).len(), 2, "turning it on saves the chat so far");

        chat_send_impl(&st, "again", no_look(), &mut |_| {}).await.unwrap();
        let saved = chat::load_history(&st.store);
        assert_eq!(saved.len(), 4);
        assert!(saved[0].looked);

        update_settings_impl(&st, Settings { history_enabled: false, ..on }).unwrap();
        assert!(chat::load_history(&st.store).is_empty(), "turning it off deletes the copy");
        assert_eq!(lock_chat(&st).turns().len(), 4, "the open chat stays");
    }

    #[test]
    fn a_new_chat_forgets_the_saved_copy_too() {
        let (_d, st) = state(Settings { history_enabled: true, ..Default::default() });
        lock_chat(&st).push_user("hi".into(), None);
        persist_chat(&st).unwrap();
        assert_eq!(chat::load_history(&st.store).len(), 1);
        chat_clear_impl(&st).unwrap();
        assert!(lock_chat(&st).turns().is_empty());
        assert!(chat::load_history(&st.store).is_empty());
    }

    #[test]
    fn a_new_chat_waits_for_the_answer_to_finish() {
        let (_d, st) = state(Settings::default());
        lock_chat(&st).push_user("hi".into(), None);
        st.chat_active.store(true, Ordering::SeqCst);
        assert_eq!(chat_clear_impl(&st).unwrap_err(), "Wait for mouce to finish answering");
        assert_eq!(lock_chat(&st).turns().len(), 1);
    }

    #[test]
    fn saved_history_is_loaded_when_the_app_starts() {
        let dir = tempfile::tempdir().unwrap();
        let store = AppStore::open(dir.path().to_path_buf()).unwrap();
        let turns = vec![chat::Turn {
            role: chat::TurnRole::User,
            text: "hi".into(),
            looked: false,
            circled: false,
            calendar: false,
        }];
        chat::save_history(&store, &turns).unwrap();
        let on = AppState::new(
            store,
            Settings { history_enabled: true, ..Default::default() },
            crate::secrets::Secrets::new("com.mouce.app.test.commands.unused"),
        );
        assert_eq!(lock_chat(&on).turns(), turns.as_slice());

        let store = AppStore::open(dir.path().to_path_buf()).unwrap();
        let off = AppState::new(
            store,
            Settings::default(),
            crate::secrets::Secrets::new("com.mouce.app.test.commands.unused"),
        );
        assert!(lock_chat(&off).turns().is_empty(), "history off: start empty");
    }

    #[test]
    fn ollama_download_url_is_fixed() {
        assert_eq!(OLLAMA_DOWNLOAD_URL, "https://ollama.com/download");
        // Compile-time check: the command takes only the app handle, never a URL.
        let _: fn(AppHandle) -> Result<(), String> = open_ollama_download;
    }

    #[test]
    fn ollama_exe_hint_points_into_local_app_data() {
        let hint = ollama_exe_hint_in(Some("C:\\Users\\x\\AppData\\Local".into()));
        assert!(hint.ends_with(Path::new("Programs").join("Ollama").join("ollama.exe")));
        assert!(!ollama_exe_hint_in(None).exists());
    }

    fn friday() -> understand::Today {
        understand::Today {
            date: "2026-10-30".into(),
            time: "14:05".into(),
            weekday: "Friday".into(),
        }
    }

    #[tokio::test]
    async fn a_calendar_note_is_read_by_the_ai_into_a_draft() {
        let server = MockServer::start().await;
        let answer = r#"{\"title\": \"Dentist\", \"date\": \"2026-10-31\", \"time\": \"15:00\"}"#;
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .and(body_partial_json(serde_json::json!({
                "messages": [{ "role": "system" }, { "role": "user", "content": "dentist tomorrow at 3pm" }]
            })))
            .respond_with(ResponseTemplate::new(200).set_body_string(ndjson_reply(answer)))
            .expect(1)
            .mount(&server)
            .await;
        let (_d, st) = state_for(&server);
        let draft = calendar_understand_impl(
            &st,
            calendar::Kind::Event,
            "  dentist tomorrow at 3pm ",
            friday(),
        )
        .await
        .unwrap();
        assert_eq!(
            draft,
            understand::Draft {
                title: "Dentist".into(),
                date: Some("2026-10-31".into()),
                time: Some("15:00".into()),
                end_time: None,
                repeat: None,
                understood: true,
            }
        );
    }

    #[tokio::test]
    async fn without_an_ai_the_note_itself_is_the_draft() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&server)
            .await;
        let (_d, st) = state_for(&server);
        let draft = calendar_understand_impl(&st, calendar::Kind::Reminder, "call mum", friday())
            .await
            .unwrap();
        assert_eq!(draft, understand::Draft::as_typed("call mum"));
    }

    #[tokio::test]
    async fn an_empty_note_or_a_bad_day_is_refused_before_asking() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(500))
            .expect(0)
            .mount(&server)
            .await;
        let (_d, st) = state_for(&server);
        let kind = calendar::Kind::Event;
        assert!(calendar_understand_impl(&st, kind, "  ", friday()).await.is_err());
        let bad = understand::Today { date: "soon".into(), ..friday() };
        assert!(calendar_understand_impl(&st, kind, "x", bad).await.is_err());
    }

    fn event_input(title: &str) -> calendar::Input {
        calendar::Input {
            kind: calendar::Kind::Event,
            title: title.into(),
            date: "2026-10-30".into(),
            time: None,
            end_time: None,
            remind_before: None,
            repeat: None,
        }
    }

    #[test]
    fn calendar_save_adds_changes_and_refuses_bad_input() {
        let (_d, st) = state(Settings::default());
        let added = calendar_save_impl(&st, None, event_input(" Dentist "), 0).unwrap();
        assert_eq!(added.title, "Dentist");
        let changed = calendar_save_impl(&st, Some(added.id), event_input("Doctor"), 0).unwrap();
        assert_eq!((changed.id, changed.title.as_str()), (added.id, "Doctor"));
        assert!(calendar_save_impl(&st, Some(added.id + 100), event_input("Gone"), 0).is_err());
        assert!(calendar_save_impl(&st, None, event_input("  "), 0).is_err());
        let found = calendar_between_impl(&st, "2026-10-01", "2026-10-31").unwrap();
        assert_eq!(found, vec![changed]);
        assert!(calendar_between_impl(&st, "October", "2026-10-31").is_err());
    }

    #[tokio::test]
    async fn a_question_about_plans_takes_them_to_the_ai() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(ResponseTemplate::new(200).set_body_string(ndjson_reply("Lunch at 1.")))
            .expect(1)
            .mount(&server)
            .await;
        let (_d, st) = state_for(&server);
        let plans = Some("- Team lunch".to_string());
        let reply = chat_send_with_plans(&st, "what's on?", no_look(), plans, &mut |_| {}).await;
        assert_eq!(reply.unwrap().text, "Lunch at 1.");
        assert!(lock_chat(&st).turns()[0].calendar);
        let requests = server.received_requests().await.unwrap();
        let body: serde_json::Value = serde_json::from_slice(&requests[0].body).unwrap();
        let system = body["messages"][0]["content"].as_str().unwrap();
        assert!(system.contains(chat::PLANS_PROMPT) && system.ends_with("- Team lunch"));
    }

    #[test]
    fn plans_only_go_to_the_chat_when_settings_allow() {
        let (_d, st) = state(Settings::default());
        assert!(plans_for_chat(&st).unwrap().contains("Nothing else is planned"));
        st.settings.lock().unwrap().calendar_in_chat = false;
        assert_eq!(plans_for_chat(&st), None);
    }

    #[test]
    fn one_day_of_a_series_changes_on_its_own() {
        let (_d, st) = state(Settings::default());
        let weekly = calendar::Input {
            repeat: Some(calendar::date::Repeat::Weekly),
            time: Some("07:00".into()),
            ..event_input("Gym")
        };
        let series = calendar_save_impl(&st, None, weekly, 0).unwrap();
        let moved = calendar::Input { time: Some("18:00".into()), ..event_input("Gym") };
        let alone =
            calendar_change_day_impl(&st, series.id, "2026-11-06", moved.clone(), 0).unwrap();
        assert_eq!((alone.repeat, alone.time.as_deref()), (None, Some("18:00")));
        assert!(calendar_change_day_impl(&st, 999, "2026-11-06", moved.clone(), 0).is_err());
        let bad = calendar::Input { title: " ".into(), ..moved };
        assert!(calendar_change_day_impl(&st, series.id, "2026-11-13", bad, 0).is_err());
    }

    #[test]
    fn quiet_hour_times_are_checked_when_settings_are_saved() {
        let (_d, st) = state(Settings::default());
        let next = Settings { quiet_from: "9:30".into(), ..Default::default() };
        assert_eq!(update_settings_impl(&st, next).unwrap().0.quiet_from, "09:30");
        let bad = Settings { quiet_to: "late".into(), ..Default::default() };
        assert!(update_settings_impl(&st, bad).is_err());
    }
}
