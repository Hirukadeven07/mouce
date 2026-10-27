//! Reporting a failed start. Under `windows_subsystem = "windows"` there is no console, so a
//! startup error would otherwise be a silent crash; a message box makes it visible.
//! Nothing here touches, renames or deletes any file.

use std::path::Path;
use windows::core::HSTRING;
use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};

/// Title of the message box.
pub const STARTUP_ERROR_TITLE: &str = "mouce couldn't start";

/// The text of the start-up error box: the error, and where mouce keeps its settings.
pub fn startup_error_message(err: &str, data_dir: Option<&Path>) -> String {
    let mut text = err.trim().to_string();
    if let Some(dir) = data_dir {
        text.push_str(&format!("\n\nYour settings folder: {}", dir.display()));
    }
    text
}

/// Shows the start-up error in a message box and waits for it to be closed.
pub fn show_startup_error(err: &str, data_dir: Option<&Path>) {
    let text = HSTRING::from(startup_error_message(err, data_dir));
    let title = HSTRING::from(STARTUP_ERROR_TITLE);
    // SAFETY: both strings outlive the call; there is no owner window.
    unsafe {
        MessageBoxW(None, &text, &title, MB_OK | MB_ICONERROR);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn message_has_the_error_and_the_settings_folder() {
        let dir = PathBuf::from(r"C:\Users\me\AppData\Roaming\com.desktopstar.app");
        let text = startup_error_message("disk is full", Some(&dir));
        assert_eq!(text, format!("disk is full\n\nYour settings folder: {}", dir.display()));
        assert!(text.ends_with("com.desktopstar.app"));
    }

    #[test]
    fn message_without_a_known_folder_is_just_the_error() {
        assert_eq!(startup_error_message("  WebView2 is missing\n", None), "WebView2 is missing");
    }
}
