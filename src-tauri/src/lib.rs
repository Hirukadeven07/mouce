pub mod ai;
pub mod commands;
pub mod ollama_setup;
pub mod secrets;
pub mod settings;
pub mod star;
pub mod startup;
pub mod store;
pub mod tray;

use secrets::Secrets;
use settings::Settings;
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;
use store::AppStore;
use tauri::Manager;

pub struct AppState {
    pub store: AppStore,
    pub settings: Mutex<Settings>,
    pub secrets: Secrets,
    /// Set while a model download is running; only one runs at a time.
    pub pull_active: AtomicBool,
    /// Windows (Settings, first run) that are being built right now.
    pub opening: star::OpeningWindows,
}

impl AppState {
    pub fn new(store: AppStore, settings: Settings, secrets: Secrets) -> AppState {
        AppState {
            store,
            settings: Mutex::new(settings),
            secrets,
            pull_active: AtomicBool::new(false),
            opening: star::OpeningWindows::default(),
        }
    }
}

/// Which window a second launch should bring up in the running instance.
#[derive(Debug, PartialEq, Eq)]
enum SecondLaunch {
    FirstRun,
    Settings,
}

/// Until first run is finished, launching again must lead back to the welcome window,
/// not to Settings.
fn second_launch_target(first_run_done: bool) -> SecondLaunch {
    if first_run_done {
        SecondLaunch::Settings
    } else {
        SecondLaunch::FirstRun
    }
}

fn on_second_launch(app: &tauri::AppHandle) {
    // Still starting up (no state yet): Settings is the safe answer, and it does nothing
    // until the app is ready.
    let first_run_done = app
        .try_state::<AppState>()
        .map(|s| s.settings.lock().unwrap_or_else(|e| e.into_inner()).first_run_done)
        .unwrap_or(true);
    match second_launch_target(first_run_done) {
        SecondLaunch::FirstRun => star::open_firstrun(app),
        SecondLaunch::Settings => star::open_settings(app),
    }
}

/// Opens the data folder and creates the windows. Any error is reported by the caller.
fn start(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let data_dir = app.path().app_data_dir()?;
    let store = AppStore::open(data_dir.clone()).map_err(|e| {
        format!("Mouce could not open its data folder at {}: {e}", data_dir.display())
    })?;
    let settings = settings::load(&store);
    app.manage(AppState::new(store, settings.clone(), Secrets::new("com.desktopstar.app")));
    star::create(app.handle(), &settings)?;
    tray::build(app.handle())?;
    if !settings.first_run_done {
        star::open_firstrun(app.handle());
    }
    Ok(())
}

/// Where Windows keeps this app's data, for the error box when no app handle exists yet.
fn fallback_data_dir() -> Option<std::path::PathBuf> {
    std::env::var_os("APPDATA").map(|d| std::path::PathBuf::from(d).join("com.desktopstar.app"))
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
        .invoke_handler(tauri::generate_handler![
            commands::star_clicked,
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
    fn second_launch_after_first_run_opens_settings() {
        assert_eq!(second_launch_target(true), SecondLaunch::Settings);
    }
}
