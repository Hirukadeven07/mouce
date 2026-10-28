pub mod ai;
pub mod chat;
pub mod circle;
pub mod commands;
pub mod hotkey;
pub mod ollama_setup;
pub mod secrets;
pub mod settings;
pub mod mouce;
pub mod screen;
pub mod startup;
pub mod store;
pub mod tray;

use secrets::Secrets;
use settings::Settings;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use store::AppStore;
use tauri::Manager;

pub struct AppState {
    pub store: AppStore,
    pub settings: Mutex<Settings>,
    pub secrets: Secrets,
    /// Set while a model download is running; only one runs at a time.
    pub pull_active: AtomicBool,
    /// Windows (Settings, first run) that are being built right now.
    pub opening: mouce::OpeningWindows,
    /// The chat with mouce, loaded from the saved history when that is turned on.
    pub chat: Mutex<chat::Conversation>,
    /// Set while mouce is answering; only one answer is written at a time.
    pub chat_active: AtomicBool,
    /// Stops the answer being written, if there is one.
    pub chat_stop: Mutex<Option<Arc<tokio::sync::Notify>>>,
    /// Set from the moment circling starts until its layer closes.
    pub circle_active: AtomicBool,
    /// The frozen screen while the user circles part of it.
    pub circle_frame: Mutex<Option<screen::Frame>>,
    /// The circled part waiting to be asked about in the chat.
    pub circled: Mutex<Option<chat::Snip>>,
}

impl AppState {
    pub fn new(store: AppStore, settings: Settings, secrets: Secrets) -> AppState {
        let history =
            if settings.history_enabled { chat::load_history(&store) } else { Vec::new() };
        AppState {
            store,
            settings: Mutex::new(settings),
            secrets,
            pull_active: AtomicBool::new(false),
            opening: mouce::OpeningWindows::default(),
            chat: Mutex::new(chat::Conversation::from_turns(history)),
            chat_active: AtomicBool::new(false),
            chat_stop: Mutex::new(None),
            circle_active: AtomicBool::new(false),
            circle_frame: Mutex::new(None),
            circled: Mutex::new(None),
        }
    }
}

/// Which window a second launch should bring up in the running instance.
#[derive(Debug, PartialEq, Eq)]
enum SecondLaunch {
    FirstRun,
    Home,
}

/// Until first run is finished, launching again must lead back to the welcome window,
/// not to the home window.
fn second_launch_target(first_run_done: bool) -> SecondLaunch {
    if first_run_done {
        SecondLaunch::Home
    } else {
        SecondLaunch::FirstRun
    }
}

fn on_second_launch(app: &tauri::AppHandle) {
    // Still starting up (no state yet): Home is the safe answer, and it does nothing
    // until the app is ready.
    let first_run_done = app
        .try_state::<AppState>()
        .map(|s| s.settings.lock().unwrap_or_else(|e| e.into_inner()).first_run_done)
        .unwrap_or(true);
    match second_launch_target(first_run_done) {
        SecondLaunch::FirstRun => mouce::open_firstrun(app),
        SecondLaunch::Home => mouce::open_home(app, mouce::HomeTab::Home),
    }
}

fn on_shortcut(app: &tauri::AppHandle, pressed: &tauri_plugin_global_shortcut::Shortcut) {
    let Some(state) = app.try_state::<AppState>() else { return };
    let (chat_keys, circle_keys) = {
        let s = state.settings.lock().unwrap_or_else(|e| e.into_inner());
        (s.chat_hotkey.clone(), s.circle_hotkey.clone())
    };
    if hotkey::matches(&circle_keys, pressed) {
        circle::start(app);
    } else if hotkey::matches(&chat_keys, pressed) {
        mouce::open_chat(app);
    }
}

/// Opens the data folder and creates the windows. Any error is reported by the caller.
fn start(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let data_dir = app.path().app_data_dir()?;
    let store = AppStore::open(data_dir.clone()).map_err(|e| {
        format!("mouce could not open its data folder at {}: {e}", data_dir.display())
    })?;
    let settings = settings::load(&store);
    app.manage(AppState::new(store, settings.clone(), Secrets::new("com.mouce.app")));
    mouce::create(app.handle(), &settings)?;
    tray::build(app.handle())?;
    // Another app may already own a shortcut; mouce still works without it.
    for (what, keys) in [("chat", &settings.chat_hotkey), ("circle", &settings.circle_hotkey)] {
        if let Err(e) = hotkey::register(app.handle(), keys) {
            eprintln!("could not set up the {what} shortcut: {e}");
        }
    }
    if !settings.first_run_done {
        mouce::open_firstrun(app.handle());
    }
    Ok(())
}

/// Where Windows keeps this app's data, for the error box when no app handle exists yet.
fn fallback_data_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("APPDATA").map(|d| std::path::PathBuf::from(d).join("com.mouce.app"))
}

/// Runs the app. An `Err` means it could not start; the user has already been told in a
/// message box, and returning it from `main` gives a non-zero exit code.
pub fn run() -> Result<(), String> {
    let result = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| on_second_launch(app)))
        // Both plugins are driven only from our own commands; no window is granted
        // their permissions (see capabilities/default.json).
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_opener::init())
        // Only the chat and circle shortcuts are ever registered (see `hotkey`).
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    if event.state() == tauri_plugin_global_shortcut::ShortcutState::Pressed {
                        on_shortcut(app, shortcut);
                    }
                })
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            commands::mouce_clicked,
            commands::get_settings,
            commands::update_settings,
            commands::set_api_key,
            commands::has_api_key,
            commands::clear_api_key,
            commands::list_models,
            commands::test_provider,
            commands::ollama_status,
            commands::pull_ollama_model,
            commands::open_ollama_download,
            commands::get_autostart,
            commands::set_autostart,
            commands::finish_first_run,
            commands::chat_history,
            commands::chat_send,
            commands::chat_stop,
            commands::chat_clear,
            commands::open_chat,
            commands::chat_attachment,
            commands::chat_discard_attachment,
            commands::circle_start,
            commands::circle_frame,
            commands::circle_ready,
            commands::circle_finish,
            commands::circle_cancel,
        ])
        .setup(|app| {
            // A setup error would make Tauri panic, which under the windows subsystem is a
            // silent crash. Say what went wrong, then ask the app to exit with code 1.
            if let Err(e) = start(app) {
                let dir = app.path().app_data_dir().ok();
                startup::show_startup_error(&e.to_string(), dir.as_deref());
                app.handle().exit(1);
            }
            Ok(())
        })
        .run(tauri::generate_context!());
    if let Err(e) = &result {
        startup::show_startup_error(&e.to_string(), fallback_data_dir().as_deref());
    }
    result.map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn second_launch_before_first_run_is_done_opens_the_welcome_window() {
        assert_eq!(second_launch_target(false), SecondLaunch::FirstRun);
    }

    #[test]
    fn second_launch_after_first_run_opens_home() {
        assert_eq!(second_launch_target(true), SecondLaunch::Home);
    }
}
