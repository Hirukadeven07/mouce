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
use std::sync::Mutex;
use store::AppStore;
use tauri::Manager;

pub struct AppState {
    pub store: AppStore,
    pub settings: Mutex<Settings>,
    pub secrets: Secrets,
}

pub fn run() {
    tauri::Builder::default()
        // A second launch just brings up Settings in the running instance.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            star::open_settings(app);
        }))
        .invoke_handler(tauri::generate_handler![commands::star_clicked])
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            let store = AppStore::open(data_dir.clone()).map_err(|e| {
                format!("Star could not open its data folder at {}: {e}", data_dir.display())
            })?;
            let settings = settings::load(&store);
            app.manage(AppState {
                store,
                settings: Mutex::new(settings.clone()),
                secrets: Secrets::new("com.desktopstar.app"),
            });
            star::create(app.handle(), &settings)?;
            tray::build(app.handle())?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Star");
}
