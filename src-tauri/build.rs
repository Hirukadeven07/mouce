fn main() {
    // The window, taskbar and tray icons are compiled into the exe, but Cargo
    // only reruns this script for the files tauri-build names. Watch the icons
    // too, so replacing one rebuilds the app instead of reusing the old exe.
    println!("cargo:rerun-if-changed=icons");
    tauri_build::build()
}
