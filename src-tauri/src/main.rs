// Prevents an extra console window on Windows in release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// An error from `run` (already shown to the user in a message box) exits with code 1.
fn main() -> Result<(), String> {
    mouce_lib::run()
}
