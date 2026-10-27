use crate::star;
use tauri::AppHandle;

/// The star was clicked (not dragged). In M1 this opens Settings.
#[tauri::command]
pub fn star_clicked(app: AppHandle) {
    star::open_settings(&app);
}
