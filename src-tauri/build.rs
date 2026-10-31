/// Every command in `lib.rs`'s `generate_handler!`. Listing them here makes Tauri check each
/// call against the capability files, so a window may run only the commands its own
/// capability (`capabilities/<window>.json`) grants as `allow-<command>`; any other call
/// is refused. `tests/safety_scan.rs` keeps this list and `lib.rs` in step.
const COMMANDS: &[&str] = &[
    "mouce_clicked",
    "get_settings",
    "update_settings",
    "set_api_key",
    "has_api_key",
    "clear_api_key",
    "list_models",
    "test_provider",
    "ollama_status",
    "pull_ollama_model",
    "open_ollama_download",
    "get_autostart",
    "set_autostart",
    "finish_first_run",
    "chat_history",
    "chat_send",
    "chat_stop",
    "chat_clear",
    "open_chat",
    "open_home",
    "open_home_chat",
    "close_chat_panel",
    "chat_attachment",
    "chat_discard_attachment",
    "circle_start",
    "circle_frame",
    "circle_ready",
    "circle_finish",
    "circle_cancel",
    "chat_take_draft",
    "dictionary_current",
    "dictionary_fit",
    "dictionary_close",
    "dictionary_chat",
    "calendar_between",
    "calendar_reminders",
    "calendar_save",
    "calendar_delete",
    "calendar_set_done",
    "calendar_clear_done",
    "calendar_skip_day",
    "calendar_change_day",
    "calendar_understand",
    "alerts_current",
    "alerts_fit",
    "alerts_act",
    "alerts_open",
];

fn main() {
    // The window, taskbar and tray icons are compiled into the exe, but Cargo
    // only reruns this script for the files tauri-build names. Watch the icons
    // too, so replacing one rebuilds the app instead of reusing the old exe.
    println!("cargo:rerun-if-changed=icons");
    let manifest = tauri_build::AppManifest::new().commands(COMMANDS);
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(manifest))
        .expect("failed to run tauri-build");
}
