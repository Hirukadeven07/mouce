pub mod ai;
pub mod commands;
pub mod ollama_setup;
pub mod secrets;
pub mod settings;
pub mod star;
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

pub fn run() {
    tauri::Builder::default()
        // A second launch just brings up Settings in the running instance.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            star::open_settings(app);
        }))
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
            let data_dir = app.path().app_data_dir()?;
            let store = AppStore::open(data_dir.clone()).map_err(|e| {
                format!("Star could not open its data folder at {}: {e}", data_dir.display())
            })?;
            let settings = settings::load(&store);
            app.manage(AppState::new(
                store,
                settings.clone(),
                Secrets::new("com.desktopstar.app"),
            ));
            star::create(app.handle(), &settings)?;
            tray::build(app.handle())?;
            if !settings.first_run_done {
                star::open_firstrun(app.handle());
            }
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Star");
}
