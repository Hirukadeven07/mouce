//! Safety guard tests: the app must never gain file-write, process-spawn,
//! input-synthesis or similar capabilities outside the store module.

use std::fs;
use std::path::{Path, PathBuf};

const EXEMPT_MARKER: &str = "// safety-scan: allow";

const FORBIDDEN_EVERYWHERE: &[&str] = &[
    "std::fs",
    "fs::write",
    "fs::remove",
    "fs::rename",
    "fs::copy",
    "File::create",
    "OpenOptions",
    "Command::new",
    "std::process",
    "ShellExecute",
    "SendInput",
    "keybd_event",
    "mouse_event",
    "SetClipboardData",
    "clipboard",
    "open_path",
    "reveal_item",
    // Anything that creates, changes or removes files, or starts processes, through the
    // Win32 API or another Rust route than `std::fs`.
    "CreateProcess",
    "WinExec",
    "DeleteFile",
    "MoveFile",
    "CopyFile",
    "CreateFile",
    "WriteFile",
    "SHFileOperation",
    "IFileOperation",
    "tokio::fs",
    "std::os::windows::fs",
    // Posting or sending window messages can drive other programs.
    "SendMessage",
    "PostMessage",
    "SetWindowText",
    "DoDefaultAction",
    // UI Automation: text may be read, but nothing may be written or invoked, and the
    // selection, scroll position and focus of other apps stay as they are.
    "SetValue(",
    ".Invoke(",
    "InvokePattern",
    "ValuePattern",
    ".Select(",
    "AddToSelection",
    "RemoveFromSelection",
    "ScrollIntoView",
    "SetFocus(",
    // Input: the dictionary's mouse hook may only watch (see `hooks_only_watch_the_mouse`).
    // Nothing may read the keyboard as it is typed, move or trap the pointer, or block input.
    "WH_KEYBOARD",
    "GetKeyboardState",
    "GetKeyState(",
    "RegisterRawInputDevices",
    "SetCursorPos",
    "ClipCursor",
    "BlockInput",
    // The calendar only converts times; it never changes the computer's clock or time zone.
    "SetSystemTime",
    "SetLocalTime",
    "SetTimeZoneInformation",
    "SetDynamicTimeZoneInformation",
    // Dragging can hand data to another program.
    "DoDragDrop",
    // WinRT file access (the text recognition needs no files).
    "Windows::Storage",
    "StorageFile",
    "StorageFolder",
    "FileRandomAccessStream",
    // Any `fs::` path (`fs::write`, `use std::fs`, `tokio::fs`, ...) outside the store.
    "fs::",
    // The log may only be written to the app's own log folder (`TargetKind::LogDir`).
    "TargetKind::Folder",
];

const FORBIDDEN_IN_STORE: &[&str] =
    &["fs::remove", "fs::rename", "fs::copy", "Command::new", "std::process"];

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// Identifiers (split on anything that is not a letter, digit or underscore).
fn identifiers(text: &str) -> impl Iterator<Item = &str> {
    text.split(|c: char| !(c.is_alphanumeric() || c == '_')).filter(|w| !w.is_empty())
}

/// Every `use std::{ ... }` group (single or multi line) whose members name `fs` or
/// `process`, as `(line number, group text)`. A group can hide the forbidden module
/// from the path-based patterns (`use std::{fs, io};`).
fn std_group_hits(text: &str) -> Vec<(usize, String)> {
    let mut hits = Vec::new();
    let lines: Vec<&str> = text.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if line.trim_start().starts_with("use std::{") && !line.contains(EXEMPT_MARKER) {
            let start = i;
            let mut group = String::from(line);
            // Gather the lines of a multi-line group up to its closing brace.
            while !group.contains('}') && i + 1 < lines.len() {
                i += 1;
                group.push(' ');
                group.push_str(lines[i]);
            }
            if identifiers(&group).any(|w| w == "fs" || w == "process") {
                hits.push((start + 1, group));
            }
        }
        i += 1;
    }
    hits
}

fn scan_std_groups(files: &[PathBuf], root: &Path) -> Vec<String> {
    let mut hits = Vec::new();
    for file in files {
        let text = fs::read_to_string(file).expect("read source file");
        for (line, group) in std_group_hits(&text) {
            let rel = file.strip_prefix(root).unwrap_or(file);
            hits.push(format!(
                "{}:{line}: `use std::{{ ... }}` group names fs or process: {group}",
                rel.display().to_string().replace(std::path::MAIN_SEPARATOR, "/"),
            ));
        }
    }
    hits
}

fn scan(files: &[PathBuf], patterns: &[&str], root: &Path) -> Vec<String> {
    let mut hits = Vec::new();
    for file in files {
        let text = fs::read_to_string(file).expect("read source file");
        for (i, line) in text.lines().enumerate() {
            if line.contains(EXEMPT_MARKER) {
                continue;
            }
            let lower = line.to_lowercase();
            for pat in patterns {
                if lower.contains(&pat.to_lowercase()) {
                    let rel = file.strip_prefix(root).unwrap_or(file);
                    hits.push(format!(
                        "{}:{}: contains `{}`",
                        rel.display().to_string().replace(std::path::MAIN_SEPARATOR, "/"),
                        i + 1,
                        pat
                    ));
                }
            }
        }
    }
    hits
}

#[test]
fn no_forbidden_apis_outside_store() {
    let root = manifest_dir();
    let src = root.join("src");
    let store = src.join("store");

    let mut all = Vec::new();
    rust_files(&src, &mut all);
    assert!(!all.is_empty(), "scan found no Rust source files");
    let (in_store, outside): (Vec<_>, Vec<_>) =
        all.into_iter().partition(|p| p.starts_with(&store));
    assert!(outside.contains(&src.join("lib.rs")), "scan did not cover src/lib.rs");
    assert!(!in_store.is_empty(), "scan did not cover any file under src/store/");

    let mut failures = scan(&outside, FORBIDDEN_EVERYWHERE, &root);
    failures.extend(scan_std_groups(&outside, &root));
    failures.extend(scan(&in_store, FORBIDDEN_IN_STORE, &root));
    // The store may use `fs`, but a group must not smuggle in `process`.
    failures
        .extend(scan_std_groups(&in_store, &root).into_iter().filter(|h| h.contains("process")));

    assert!(failures.is_empty(), "forbidden APIs found:\n{}", failures.join("\n"));
}

#[test]
fn no_forbidden_dependencies() {
    let root = manifest_dir();

    let cargo = fs::read_to_string(root.join("Cargo.toml")).expect("read Cargo.toml");
    let mut failures = Vec::new();
    for dep in [
        "tauri-plugin-fs",
        "tauri-plugin-shell",
        "tauri-plugin-clipboard",
        "tauri-plugin-dialog",
        "tauri-plugin-process",
        "tauri-plugin-updater",
    ] {
        if cargo.contains(dep) {
            failures.push(format!("Cargo.toml contains `{dep}`"));
        }
    }

    let pkg = fs::read_to_string(root.join("../package.json")).expect("read ../package.json");
    for dep in [
        "@tauri-apps/plugin-fs",
        "plugin-shell",
        "plugin-clipboard",
        "plugin-dialog",
        "plugin-opener",
        "plugin-process",
        "plugin-updater",
    ] {
        if pkg.contains(dep) {
            failures.push(format!("package.json contains `{dep}`"));
        }
    }

    assert!(failures.is_empty(), "forbidden dependencies found:\n{}", failures.join("\n"));
}

#[test]
fn no_command_returns_an_api_key() {
    let src = manifest_dir().join("src");
    let commands = fs::read_to_string(src.join("commands.rs")).expect("read commands.rs");
    assert!(!commands.contains("fn get_api_key"), "commands.rs must not define get_api_key");
    assert!(!commands.contains("fn get_key"), "no command may wrap Secrets::get_key");
    let lib = fs::read_to_string(src.join("lib.rs")).expect("read lib.rs");
    assert!(!lib.contains("get_api_key"), "lib.rs must not register get_api_key");
}

/// Every way the opener plugin can be reached: its crate path, its extension trait,
/// the `.opener()` accessor and the `open_url` function.
fn opener_mentions(text: &str) -> Vec<&'static str> {
    ["tauri_plugin_opener::", "OpenerExt", ".opener()", "open_url"]
        .into_iter()
        .filter(|p| text.contains(p))
        .collect()
}

#[test]
fn opener_is_only_used_for_the_fixed_download_url() {
    let src = manifest_dir().join("src");
    let mut files = Vec::new();
    rust_files(&src, &mut files);
    let lib_rs = src.join("lib.rs");
    let commands_rs = src.join("commands.rs");

    let mut failures = Vec::new();
    for file in &files {
        let rel = file.strip_prefix(&src).unwrap_or(file).display().to_string();
        let text = fs::read_to_string(file).unwrap();
        if *file == lib_rs {
            // Plugin registration only: `tauri_plugin_opener::init()` and nothing else.
            let stripped = text.replace("tauri_plugin_opener::init()", "");
            let found = opener_mentions(&stripped);
            if !found.is_empty() {
                failures.push(format!("{rel}: opener used beyond plugin registration: {found:?}"));
            }
        } else if *file != commands_rs {
            let found = opener_mentions(&text);
            if !found.is_empty() {
                failures.push(format!("{rel}: opener used outside commands.rs: {found:?}"));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "opener guard failed:
{}",
        failures.join(
            "
"
        )
    );

    // lib.rs still registers the plugin.
    let lib = fs::read_to_string(&lib_rs).unwrap();
    assert!(lib.contains("tauri_plugin_opener::init()"), "lib.rs must register the opener plugin");

    // commands.rs: exactly one import and one call site, and the call uses the fixed URL.
    let commands = fs::read_to_string(&commands_rs).unwrap();
    assert_eq!(commands.matches("tauri_plugin_opener::").count(), 1, "one opener import only");
    assert_eq!(commands.matches(".opener()").count(), 1, "one opener call site only");
    assert_eq!(commands.matches("open_url(").count(), 1, "one open_url call only");
    assert!(commands.contains("open_url(OLLAMA_DOWNLOAD_URL"));
    assert!(
        commands.contains("\"https://ollama.com/download\""),
        "commands.rs must contain the literal download URL"
    );
}

/// The only keys a capability file may have. Notably absent: `remote` (web pages calling the
/// commands), `webviews` and `platforms` (they change which windows the grant reaches).
const CAPABILITY_KEYS: &[&str] =
    &["$schema", "identifier", "description", "local", "windows", "permissions"];

/// What each window may call: Tauri's core set, plus `allow-<command>` for each of our own
/// commands it uses (see `build.rs`). A window that shows other apps' text (dictionary) or
/// covers the screen (circle) gets nothing it does not need, so a fault in one of them
/// cannot reach the API keys, the settings or the screen picture.
const WINDOW_GRANTS: &[(&str, &[&str])] = &[
    (
        "mouce",
        &["core:window:allow-start-dragging", "get_settings", "mouce_clicked", "alerts_current"],
    ),
    (
        "home",
        &[
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
            "chat_take_draft",
            "circle_start",
            "calendar_between",
            "calendar_reminders",
            "calendar_save",
            "calendar_delete",
            "calendar_set_done",
            "calendar_clear_done",
            "calendar_skip_day",
            "calendar_change_day",
            "calendar_understand",
        ],
    ),
    (
        "chat",
        &[
            "get_settings",
            "chat_history",
            "chat_send",
            "chat_stop",
            "chat_clear",
            "open_home",
            "open_home_chat",
            "chat_attachment",
            "chat_discard_attachment",
            "chat_take_draft",
            "circle_start",
            "calendar_between",
            "calendar_save",
            "calendar_change_day",
            "calendar_understand",
        ],
    ),
    ("circle", &["circle_frame", "circle_ready", "circle_finish", "circle_cancel"]),
    (
        "dictionary",
        &[
            "get_settings",
            "dictionary_current",
            "dictionary_fit",
            "dictionary_close",
            "dictionary_chat",
        ],
    ),
    ("alerts", &["get_settings", "alerts_current", "alerts_fit", "alerts_act", "alerts_open"]),
    (
        "firstrun",
        &[
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
            "set_autostart",
            "finish_first_run",
        ],
    ),
];

/// The commands that touch keys, settings, the screen or saved data, and the only windows
/// that may call each one. Checked on top of `WINDOW_GRANTS`, so widening one of these
/// grants takes two deliberate edits.
const SENSITIVE: &[(&str, &[&str])] = &[
    ("set_api_key", &["home", "firstrun"]),
    ("clear_api_key", &["home", "firstrun"]),
    ("update_settings", &["home", "firstrun"]),
    ("set_autostart", &["home", "firstrun"]),
    ("pull_ollama_model", &["home", "firstrun"]),
    ("circle_frame", &["circle"]),
    ("circle_finish", &["circle"]),
    ("chat_send", &["home", "chat"]),
    ("chat_clear", &["home", "chat"]),
    ("calendar_delete", &["home"]),
    ("calendar_clear_done", &["home"]),
];

/// `allow-chat-send` for `chat_send`; core permissions stay as they are.
fn permission_id(grant: &str) -> String {
    if grant.contains(':') {
        grant.to_string()
    } else {
        format!("allow-{}", grant.replace('_', "-"))
    }
}

fn capability_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(dir).expect("read capabilities dir");
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            capability_files(&path, out);
        } else {
            out.push(path);
        }
    }
}

type Grants = std::collections::BTreeMap<String, std::collections::BTreeSet<String>>;

#[test]
fn each_window_is_granted_only_what_it_uses() {
    let dir = manifest_dir().join("capabilities");
    let mut files = Vec::new();
    capability_files(&dir, &mut files);
    assert!(!files.is_empty(), "no capability files found");

    let mut granted = Grants::new();
    for path in &files {
        let name = path.display();
        assert!(
            path.extension().is_some_and(|e| e == "json"),
            "{name}: only plain .json capability files are supported; .json5/.toml are not allowed"
        );
        let text = fs::read_to_string(path).expect("read capability file");
        let value: serde_json::Value = serde_json::from_str(&text).expect("parse capability json");
        let object = value.as_object().expect("capability file is a JSON object");
        for key in object.keys() {
            assert!(
                CAPABILITY_KEYS.contains(&key.as_str()),
                "{name}: capability key `{key}` is not allowed (`remote`, `webviews` and \
                 `platforms` change who may call the commands); allowed keys: {CAPABILITY_KEYS:?}"
            );
        }
        let windows = value.get("windows").and_then(|w| w.as_array()).expect("windows array");
        assert_eq!(windows.len(), 1, "{name}: a capability covers exactly one window");
        let window = windows[0].as_str().expect("window label").to_string();
        assert!(!window.contains('*'), "{name}: no wildcard windows");
        let perms = value
            .get("permissions")
            .and_then(|p| p.as_array())
            .expect("capability has a permissions array");
        let set = granted.entry(window).or_default();
        for perm in perms {
            let id = perm.as_str().unwrap_or_else(|| {
                panic!(
                    "{name}: permissions are plain names; scoped entries are not allowed: {perm}"
                )
            });
            set.insert(id.to_string());
        }
    }

    let expected: Grants = WINDOW_GRANTS
        .iter()
        .map(|(window, grants)| {
            let mut set: std::collections::BTreeSet<String> =
                grants.iter().map(|g| permission_id(g)).collect();
            set.insert("core:default".to_string());
            (window.to_string(), set)
        })
        .collect();
    assert_eq!(
        granted, expected,
        "capabilities must grant exactly WINDOW_GRANTS, window by window"
    );

    for (command, allowed) in SENSITIVE {
        let id = permission_id(command);
        for (window, set) in &granted {
            if set.contains(&id) {
                assert!(
                    allowed.contains(&window.as_str()),
                    "{window} must not be granted {command}; only {allowed:?} may call it"
                );
            }
        }
    }
}

/// The names in the first `[ ... ]` list after `marker` in `text`.
fn names_after(text: &str, marker: &str) -> Vec<String> {
    let at = text.find(marker).unwrap_or_else(|| panic!("`{marker}` not found")) + marker.len();
    let open = text[at..].find('[').expect("list opens") + at;
    let close = text[open..].find(']').expect("list closes") + open;
    text[open + 1..close]
        .split(',')
        .map(|n| n.trim().trim_matches('"').trim_start_matches("commands::").to_string())
        .filter(|n| !n.is_empty())
        .collect()
}

#[test]
fn every_command_is_under_the_access_check() {
    // A command missing from build.rs's list is refused in every window, and one listed but
    // not registered is a stale grant; either way the two lists must match.
    let root = manifest_dir();
    let build = fs::read_to_string(root.join("build.rs")).expect("read build.rs");
    let lib = fs::read_to_string(root.join("src/lib.rs")).expect("read lib.rs");
    assert!(build.contains(".app_manifest("), "build.rs must hand COMMANDS to tauri-build");
    let mut listed = names_after(&build, "const COMMANDS: &[&str] = &");
    let mut registered = names_after(&lib, "generate_handler!");
    listed.sort();
    registered.sort();
    assert_eq!(listed, registered, "build.rs COMMANDS and lib.rs generate_handler! differ");

    for (window, grants) in WINDOW_GRANTS {
        for grant in grants.iter().filter(|g| !g.contains(':')) {
            assert!(
                listed.iter().any(|c| c == grant),
                "{window} is granted unknown command {grant}"
            );
        }
    }
}

#[test]
fn tauri_conf_has_no_inline_capabilities() {
    let text =
        fs::read_to_string(manifest_dir().join("tauri.conf.json")).expect("read tauri.conf.json");
    let conf: serde_json::Value = serde_json::from_str(&text).expect("parse tauri.conf.json");
    let inline = conf.pointer("/app/security/capabilities");
    let empty = match inline {
        None => true,
        Some(v) => v.as_array().is_some_and(|a| a.is_empty()),
    };
    assert!(empty, "tauri.conf.json must not define inline app.security.capabilities: {inline:?}");
}

/// The Win32 API areas the `windows` crate may expose. Adding one (for example file system,
/// shell, clipboard/data exchange or keyboard and mouse input) must be a deliberate edit of
/// this list, reviewed against the safety promise.
///
/// `Win32_Graphics_Gdi` is here for "Look at my screen": `screen.rs` copies screen pixels
/// into a memory bitmap. GDI draws and reads pixels; it cannot touch files, input or the
/// other programs' windows.
///
/// The dictionary (`src/dictionary/`) adds the rest:
/// - `Win32_UI_Input_KeyboardAndMouse`: `GetAsyncKeyState`, to see whether Ctrl is held
///   during a right-click. The same area can make input, which the patterns above forbid
///   (`SendInput`, `keybd_event`, `mouse_event`, `BlockInput`, `SetCursorPos`), and
///   `hooks_only_watch_the_mouse` limits which keys may be asked about.
/// - `Win32_UI_Accessibility`, with `Win32_System_Com` to create it and `Win32_System_Ole`
///   for the arrays it returns: UI Automation, read-only, for the word under the pointer.
///   Invoking, setting values, selecting and scrolling are forbidden above; the drag and
///   copy-paste parts of OLE are forbidden too.
/// - `Foundation`, `Globalization`, `Graphics_Imaging`, `Media_Ocr` and `Win32_System_WinRT`:
///   the text recognition built into Windows, reading a picture held in memory. None of them
///   opens files (the WinRT file APIs are forbidden above, and `Storage` is not enabled).
///
/// The calendar (`src/calendar/`) adds `Win32_System_Time`: turning a day and time on the
/// user's clock into a moment and back, with the computer's own time zone rules. It only
/// converts times; it cannot set the clock (that needs a privilege mouce never asks for).
const WINDOWS_FEATURES: &[&str] = &[
    "Foundation",
    "Globalization",
    "Graphics_Imaging",
    "Media_Ocr",
    "Win32_Foundation",
    "Win32_Graphics_Gdi",
    "Win32_System_Com",
    "Win32_System_Ole",
    "Win32_System_Time",
    "Win32_System_WinRT",
    "Win32_UI_Accessibility",
    "Win32_UI_Input_KeyboardAndMouse",
    "Win32_UI_WindowsAndMessaging",
];

/// The `features` of the single `windows = { ... }` dependency in a Cargo.toml.
fn windows_features(cargo: &str) -> Result<Vec<String>, String> {
    let deps: Vec<usize> = cargo
        .lines()
        .enumerate()
        .filter(|(_, l)| {
            let t = l.trim_start();
            t.starts_with("windows ") || t.starts_with("windows=") || t == "[dependencies.windows]"
        })
        .map(|(i, _)| i)
        .collect();
    if deps.len() != 1 {
        return Err(format!("expected exactly one `windows` dependency, found {}", deps.len()));
    }
    let rest: String = cargo.lines().skip(deps[0]).collect::<Vec<_>>().join("\n");
    let at = rest.find("features").ok_or("the `windows` dependency has no features list")?;
    let open = rest[at..].find('[').ok_or("no `[` after `features`")? + at;
    let close = rest[open..].find(']').ok_or("unterminated features list")? + open;
    Ok(rest[open + 1..close]
        .split(',')
        .map(|f| f.trim().trim_matches('"').to_string())
        .filter(|f| !f.is_empty())
        .collect())
}

#[test]
fn windows_crate_features_match_the_allowlist() {
    let cargo = fs::read_to_string(manifest_dir().join("Cargo.toml")).expect("read Cargo.toml");
    let mut found = windows_features(&cargo).expect("parse the windows dependency");
    found.sort();
    let mut expected: Vec<String> = WINDOWS_FEATURES.iter().map(|f| f.to_string()).collect();
    expected.sort();
    assert_eq!(
        found, expected,
        "the `windows` crate features changed; if this is intended, update WINDOWS_FEATURES in \
         tests/safety_scan.rs after checking the new API area cannot touch files, input or the clipboard"
    );
}

#[test]
fn windows_features_parser_reads_inline_and_multiline_lists() {
    let one = "[dependencies]\nwindows = { version = \"0.62\", features = [\"A\", \"B_C\"] }\nserde = \"1\"\n";
    assert_eq!(windows_features(one).unwrap(), vec!["A", "B_C"]);
    let multi = "windows = { version = \"0.62\", features = [\n  \"A\",\n  \"B\",\n] }\n";
    assert_eq!(windows_features(multi).unwrap(), vec!["A", "B"]);
    assert!(windows_features("serde = \"1\"\n").is_err());
    let twice = "windows = { features = [\"A\"] }\n[target.'cfg(windows)'.dependencies]\nwindows = { features = [\"B\"] }\n";
    assert!(windows_features(twice).is_err());
}

#[test]
fn std_group_scan_finds_hidden_fs_and_process() {
    assert_eq!(std_group_hits("use std::{fs, io};\n").len(), 1);
    assert_eq!(std_group_hits("use std::{\n    io,\n    process::Command,\n};\n").len(), 1);
    assert!(std_group_hits("use std::{io, sync::Arc};\n").is_empty());
    assert!(std_group_hits("use std::{fs_like, io};\n").is_empty());
    assert!(std_group_hits("use std::{fs}; // safety-scan: allow\n").is_empty());
}

/// The asset protocol serves local files to the webview; it must stay absent or disabled.
fn asset_protocol_problem(conf: &serde_json::Value) -> Option<String> {
    let proto = conf.pointer("/app/security/assetProtocol")?;
    match proto.get("enable").and_then(|e| e.as_bool()) {
        Some(false) => None,
        _ => Some(format!("app.security.assetProtocol must be absent or disabled: {proto}")),
    }
}

#[test]
fn asset_protocol_check_flags_an_enabled_protocol() {
    use serde_json::json;
    assert!(asset_protocol_problem(&json!({"app": {"security": {}}})).is_none());
    let off = json!({"app": {"security": {"assetProtocol": {"enable": false}}}});
    assert!(asset_protocol_problem(&off).is_none());
    let on = json!({"app": {"security": {"assetProtocol": {"enable": true, "scope": ["**"]}}}});
    assert!(asset_protocol_problem(&on).is_some());
    let unspecified = json!({"app": {"security": {"assetProtocol": {"scope": ["**"]}}}});
    assert!(asset_protocol_problem(&unspecified).is_some());
}

#[test]
fn tauri_conf_security_is_locked_down() {
    let text =
        fs::read_to_string(manifest_dir().join("tauri.conf.json")).expect("read tauri.conf.json");
    let conf: serde_json::Value = serde_json::from_str(&text).expect("parse tauri.conf.json");

    if let Some(problem) = asset_protocol_problem(&conf) {
        panic!("{problem}");
    }

    assert_eq!(
        conf.pointer("/app/security/csp").and_then(|c| c.as_str()),
        Some(
            "default-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'; \
             connect-src ipc: http://ipc.localhost"
        ),
        "the content security policy must be exactly the reviewed one"
    );
}

/// The keys whose state may be asked about: the modifiers held during a right-click.
const MODIFIER_KEYS: &[&str] = &["VK_CONTROL", "VK_SHIFT", "VK_MENU"];

/// Problems with how `text` (the file `rel`) uses Windows hooks and key state. Only the
/// dictionary's hook file may install a hook, only a low-level mouse hook, and only it may
/// ask whether a key is down, for the modifier keys alone.
fn hook_problems(rel: &str, text: &str) -> Vec<String> {
    const HOOK_FILE: &str = "dictionary/hook.rs";
    let mut problems = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let at = format!("{rel}:{}", i + 1);
        for word in identifiers(line) {
            if word.starts_with("WH_") && word != "WH_MOUSE_LL" {
                problems
                    .push(format!("{at}: only the low-level mouse hook is allowed, not {word}"));
            }
        }
        if rel != HOOK_FILE {
            for api in ["SetWindowsHookEx", "GetAsyncKeyState", "WH_MOUSE_LL"] {
                if line.contains(api) {
                    problems.push(format!("{at}: {api} is only allowed in {HOOK_FILE}"));
                }
            }
        }
        if let Some(call) = line.find("GetAsyncKeyState(") {
            // Either a modifier key, or the `key` of `is_down`, whose callers are checked below.
            let arg = line[call + "GetAsyncKeyState(".len()..].trim_start();
            if !(arg.starts_with("key.0") || MODIFIER_KEYS.iter().any(|k| arg.starts_with(k))) {
                problems
                    .push(format!("{at}: GetAsyncKeyState may only ask about {MODIFIER_KEYS:?}"));
            }
        }
        if line.contains("is_down(") && !line.contains("fn is_down(") {
            let asked: Vec<&str> = line
                .match_indices("is_down(")
                .map(|(j, _)| line[j + "is_down(".len()..].split(')').next().unwrap_or(""))
                .collect();
            for key in asked {
                if !MODIFIER_KEYS.contains(&key.trim()) {
                    problems.push(format!(
                        "{at}: is_down may only ask about {MODIFIER_KEYS:?}, not {key}"
                    ));
                }
            }
        }
    }
    problems
}

#[test]
fn hooks_only_watch_the_mouse() {
    let src = manifest_dir().join("src");
    let mut files = Vec::new();
    rust_files(&src, &mut files);
    let mut problems = Vec::new();
    for file in &files {
        let rel = file
            .strip_prefix(&src)
            .unwrap_or(file)
            .display()
            .to_string()
            .replace(std::path::MAIN_SEPARATOR, "/");
        problems.extend(hook_problems(&rel, &fs::read_to_string(file).unwrap()));
    }
    assert!(
        problems.is_empty(),
        "hook guard failed:
{}",
        problems.join(
            "
"
        )
    );
    let hook = fs::read_to_string(src.join("dictionary/hook.rs")).unwrap();
    assert_eq!(hook.matches("SetWindowsHookExW(").count(), 1, "one hook only");
}

#[test]
fn hook_guard_catches_other_hooks_and_keys() {
    assert!(hook_problems("dictionary/hook.rs", "SetWindowsHookExW(WH_MOUSE_LL, p, None, 0)")
        .is_empty());
    assert!(
        hook_problems("dictionary/hook.rs", "is_down(VK_CONTROL) && is_down(VK_SHIFT)").is_empty()
    );
    assert!(!hook_problems("dictionary/hook.rs", "SetWindowsHookExW(WH_KEYBOARD_LL, p)").is_empty());
    assert!(!hook_problems("dictionary/hook.rs", "WH_GETMESSAGE").is_empty());
    assert!(!hook_problems("dictionary/hook.rs", "is_down(VK_A)").is_empty());
    assert!(!hook_problems("dictionary/hook.rs", "GetAsyncKeyState(0x41)").is_empty());
    assert!(!hook_problems("chat.rs", "SetWindowsHookExW(WH_MOUSE_LL, p)").is_empty());
    assert!(!hook_problems("chat.rs", "GetAsyncKeyState(VK_CONTROL.0 as i32)").is_empty());
}
