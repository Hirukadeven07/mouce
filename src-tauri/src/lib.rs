pub mod ai;
pub mod ollama_setup;
pub mod secrets;
pub mod settings;
pub mod store;

pub fn run() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("error while running Star");
}
