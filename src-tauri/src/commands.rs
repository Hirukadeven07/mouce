//! Tauri commands for the mouce, chat, home and first-run windows.
//!
//! Each command is a thin wrapper over a plain `*_impl` function so the logic can be
//! unit-tested without a running app. There is deliberately no command that returns an
//! API key: keys go in, and are only read inside Rust to talk to the chosen provider.

use crate::ai::{self, ChatMessage, ChatRequest, ImageInput, Role};
use crate::chat::{self, Conversation, Turn};
use crate::ollama_setup::{self, OllamaStatus, PullProgress};
use crate::settings::{self, ProviderId, Settings};
use crate::{hotkey, mouce, screen, AppState};
use serde::Serialize;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, MutexGuard};
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::sync::Notify;
use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_opener::OpenerExt as _;

/// Event to the webview while a model downloads: payload is a `PullProgress`.
pub const PULL_PROGRESS_EVENT: &str = "ollama-pull-progress";
/// The only address the app ever opens in the browser.
const OLLAMA_DOWNLOAD_URL: &str = "https://ollama.com/download";

const TEST_SYSTEM: &str = "You are a connection test.";
const TEST_USER: &str = "Reply with exactly: ready";
const TEST_MAX_TOKENS: u32 = 16;

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

/// The mouce was clicked (not dragged): open the chat beside it.
#[tauri::command]
pub fn mouce_clicked(app: AppHandle) {
    mouce::open_chat(&app);
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
    hotkey::parse(&next.chat_hotkey)?;
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
    let before = get_settings_impl(&state);
    let old_hotkey = before.chat_hotkey;
    let hotkey_changed = old_hotkey.trim() != settings.chat_hotkey.trim();
    if hotkey_changed {
        hotkey::replace(&app, &old_hotkey, &settings.chat_hotkey)?;
    }
    let new_hotkey = settings.chat_hotkey.clone();
    let (saved, size_changed) = update_settings_impl(&state, settings).inspect_err(|_| {
        if hotkey_changed {
            // Not saved, so the old shortcut stays the one that works.
            let _ = hotkey::replace(&app, &new_hotkey, &old_hotkey);
        }
    })?;
    let _ = app.emit(mouce::SETTINGS_CHANGED_EVENT, &saved);
    if size_changed {
        // The settings are already saved; a window that could not resize is not an error.
        let resized = mouce::resize(&app, saved.mouce_size);
        log_best_effort("resize the mouce after saving settings", resized, |m| eprintln!("{m}"));
    }
    if saved.liquid_glass != before.liquid_glass {
        mouce::set_glass(&app, saved.liquid_glass);
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
        None if provider == ProviderId::Ollama => {
            Ok(ollama_setup::DEFAULT_LOCAL_MODEL.to_string())
        }
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

/// Asks `provider` about the conversation so far, streaming the answer to `on_chunk`.
async fn ask(
    state: &AppState,
    settings: &Settings,
    provider: ProviderId,
    on_chunk: &mut (dyn for<'a> FnMut(&'a str) + Send),
) -> Result<String, String> {
    let model = model_for(settings, provider)?;
    let request = lock_chat(state).request(model);
    let provider = build_provider(state, settings, provider)?;
    provider.chat_stream(&request, on_chunk).await.map_err(|e| e.to_string())
}

/// Asks the chosen AI. If an online AI fails before writing anything and fallback is on,
/// asks the local one instead. Returns the answer and whether the local AI gave it.
async fn ask_with_fallback(
    state: &AppState,
    settings: &Settings,
    on_chunk: &mut (dyn for<'a> FnMut(&'a str) + Send),
) -> Result<(String, bool), String> {
    let mut wrote = false;
    let first = {
        let mut track = |d: &str| {
            wrote = true;
            on_chunk(d);
        };
        ask(state, settings, settings.provider, &mut track).await
    };
    match first {
        Ok(text) => Ok((text, false)),
        Err(e)
            if !wrote && settings.provider != ProviderId::Ollama && settings.fallback_to_local =>
        {
            match ask(state, settings, ProviderId::Ollama, on_chunk).await {
                Ok(text) => Ok((text, true)),
                Err(local) => Err(format!("{e} (the local AI could not answer either: {local})")),
            }
        }
        Err(e) => Err(e),
    }
}

/// Adds the user's question (with a screen picture when `screen` is given) and streams
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
    let Some(_slot) = BusySlot::try_take(&state.chat_active) else {
        return Err("mouce is still answering".to_string());
    };
    let text = match text.trim() {
        "" if screen.is_some() => chat::LOOK_QUESTION.to_string(),
        "" => return Err("Type a question first".to_string()),
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
    lock_chat(state).push_user(text, screen);

    let mut written = String::new();
    let outcome = {
        let mut on_chunk = |d: &str| {
            written.push_str(d);
            on_delta(d);
        };
        tokio::select! {
            result = ask_with_fallback(state, &settings, &mut on_chunk) => Some(result),
            _ = stop.notified() => None,
        }
    };

    let reply = {
        let mut chat = lock_chat(state);
        match outcome {
            Some(Ok((text, used_local))) => {
                chat.push_assistant(text.clone());
                ChatReply { text, stopped: false, used_local }
            }
            Some(Err(e)) => {
                chat.drop_unanswered();
                return Err(e);
            }
            None => {
                if written.trim().is_empty() {
                    chat.drop_unanswered();
                } else {
                    chat.push_assistant(written.clone());
                }
                ChatReply { text: written, stopped: true, used_local: false }
            }
        }
    };
    // The answer is already shown; a history that could not be saved is not an error.
    log_best_effort("save the chat history", persist_chat(state), |m| eprintln!("{m}"));
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

/// Asks mouce `text`, after looking at the screen when `look` is set. Each piece of the
/// answer is sent down `on_delta` as it arrives.
#[tauri::command]
pub async fn chat_send(
    app: AppHandle,
    state: State<'_, AppState>,
    text: String,
    look: bool,
    on_delta: Channel<String>,
) -> Result<ChatReply, String> {
    let _ = app.emit(mouce::MOUCE_STATE_EVENT, if look { "looking" } else { "thinking" });
    // Back to idle however this ends: finished, failed, stopped or cancelled.
    let _idle = OnDrop::new(|| {
        let _ = app.emit(mouce::MOUCE_STATE_EVENT, "idle");
    });
    let screen = look.then(|| look_at_screen(app.clone()));
    chat_send_impl(&state, &text, screen, &mut |d| {
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
        log_best_effort("close the first-run window", window.close(), |m| eprintln!("{m}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ollama_setup::OllamaStatus;
    use crate::settings::{ProviderId, Settings, MouceSize};
    use crate::store::AppStore;
    use crate::AppState;
    use std::path::Path;
    use std::panic::{catch_unwind, AssertUnwindSafe};
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
                        let s = Settings {
                            circle_hotkey: format!("Ctrl+{i}"),
                            ..Default::default()
                        };
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
            ResponseTemplate::new(200)
                .set_body_string(PULL_OK)
                .set_delay(Duration::from_secs(5)),
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
        let turns =
            vec![chat::Turn { role: chat::TurnRole::User, text: "hi".into(), looked: false }];
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
}
