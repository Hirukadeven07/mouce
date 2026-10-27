//! Tauri commands for the Settings and first-run windows.
//!
//! Each command is a thin wrapper over a plain `*_impl` function so the logic can be
//! unit-tested without a running app. There is deliberately no command that returns an
//! API key: keys go in, and are only read inside Rust to talk to the chosen provider.

use crate::ai::{self, ChatMessage, ChatRequest, Role};
use crate::ollama_setup::{self, OllamaStatus, PullProgress};
use crate::settings::{self, ProviderId, Settings};
use crate::{star, AppState};
use std::path::{Path, PathBuf};
use std::sync::MutexGuard;
use tauri::{AppHandle, Emitter, Manager, State};
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

/// The star was clicked (not dragged). In M1 this opens Settings.
#[tauri::command]
pub fn star_clicked(app: AppHandle) {
    star::open_settings(&app);
}

// ---- settings ----

pub fn get_settings_impl(state: &AppState) -> Settings {
    lock(state).clone()
}

/// Saves `incoming` while holding the settings lock, so a concurrent writer (such as the
/// star-position save) can never be overwritten by a stale snapshot. The position and the
/// first-run flag belong to the backend; whatever the window sends for them is ignored.
/// Returns the saved settings and whether the star size changed.
pub fn update_settings_impl(
    state: &AppState,
    incoming: Settings,
) -> Result<(Settings, bool), String> {
    let mut current = lock(state);
    let mut next = incoming;
    next.star_position = current.star_position;
    next.first_run_done = current.first_run_done;
    settings::save(&state.store, &next).map_err(|e| e.to_string())?;
    let size_changed = current.star_size != next.star_size;
    *current = next.clone();
    Ok((next, size_changed))
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Settings {
    get_settings_impl(&state)
}

#[tauri::command]
pub fn update_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: Settings,
) -> Result<Settings, String> {
    let (saved, size_changed) = update_settings_impl(&state, settings)?;
    let _ = app.emit(star::SETTINGS_CHANGED_EVENT, &saved);
    if size_changed {
        star::resize(&app, saved.star_size).map_err(|e| e.to_string())?;
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

#[tauri::command]
pub fn set_api_key(
    state: State<'_, AppState>,
    provider: ProviderId,
    key: String,
) -> Result<(), String> {
    set_api_key_impl(&state, provider, &key)
}

#[tauri::command]
pub fn has_api_key(state: State<'_, AppState>, provider: ProviderId) -> bool {
    has_api_key_impl(&state, provider)
}

#[tauri::command]
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
    let _ = app.emit(star::STAR_STATE_EVENT, "thinking");
    let result = test_provider_impl(&state, provider).await;
    let _ = app.emit(star::STAR_STATE_EVENT, "idle");
    result
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

pub async fn pull_ollama_model_impl(
    state: &AppState,
    model: &str,
    on_progress: &mut (dyn FnMut(PullProgress) + Send),
) -> Result<(), String> {
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

#[tauri::command]
pub fn get_autostart(app: AppHandle) -> Result<bool, String> {
    app.autolaunch().is_enabled().map_err(|e| e.to_string())
}

#[tauri::command]
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

#[tauri::command]
pub fn finish_first_run(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let saved = finish_first_run_impl(&state)?;
    let _ = app.emit(star::SETTINGS_CHANGED_EVENT, &saved);
    if let Some(window) = app.get_webview_window(star::FIRSTRUN_LABEL) {
        window.close().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ollama_setup::OllamaStatus;
    use crate::settings::{ProviderId, Settings, StarSize};
    use crate::store::AppStore;
    use crate::AppState;
    use std::path::Path;
    use std::sync::{Arc, Mutex};
    use wiremock::matchers::{body_partial_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn state_with(settings: Settings, service: &str) -> (tempfile::TempDir, AppState) {
        let dir = tempfile::tempdir().unwrap();
        let store = AppStore::open(dir.path().to_path_buf()).unwrap();
        let state = AppState {
            store,
            settings: Mutex::new(settings),
            secrets: crate::secrets::Secrets::new(service),
        };
        (dir, state)
    }

    fn state(settings: Settings) -> (tempfile::TempDir, AppState) {
        state_with(settings, "com.desktopstar.app.test.commands.unused")
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
        let s = Settings { star_rotation: false, ..Default::default() };
        let (_d, st) = state(s.clone());
        assert_eq!(get_settings_impl(&st), s);
    }

    #[test]
    fn update_settings_persists_and_returns_new_settings() {
        let (_d, st) = state(Settings::default());
        let next = Settings {
            star_size: StarSize::Large,
            star_rotation: false,
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
            Settings { star_position: Some((5, 6)), first_run_done: true, ..Default::default() };
        let (_d, st) = state(current);
        let stale = Settings {
            star_position: Some((1, 1)),
            first_run_done: false,
            star_rotation: false,
            ..Default::default()
        };
        let (saved, _) = update_settings_impl(&st, stale).unwrap();
        assert_eq!(saved.star_position, Some((5, 6)));
        assert!(saved.first_run_done);
        assert!(!saved.star_rotation);
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
        let service = "com.desktopstar.app.test.commands.api_key";
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

    #[test]
    fn finish_first_run_sets_flag_and_persists() {
        let s = Settings { star_rotation: false, ..Default::default() };
        let (_d, st) = state(s);
        let done = finish_first_run_impl(&st).unwrap();
        assert!(done.first_run_done);
        assert!(!done.star_rotation);
        assert!(crate::settings::load(&st.store).first_run_done);
        assert!(get_settings_impl(&st).first_run_done);
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
