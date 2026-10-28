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
    // UI Automation: text may be read, but nothing may be written or invoked.
    "SetValue(",
    ".Invoke(",
    "InvokePattern",
    "ValuePattern",
    // Any `fs::` path (`fs::write`, `use std::fs`, `tokio::fs`, ...) outside the store.
    "fs::",
];

const FORBIDDEN_IN_STORE: &[&str] = &[
    "fs::remove",
    "fs::rename",
    "fs::copy",
    "Command::new",
    "std::process",
];

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
    assert!(
        outside.contains(&src.join("lib.rs")),
        "scan did not cover src/lib.rs"
    );
    assert!(
        !in_store.is_empty(),
        "scan did not cover any file under src/store/"
    );

    let mut failures = scan(&outside, FORBIDDEN_EVERYWHERE, &root);
    failures.extend(scan_std_groups(&outside, &root));
    failures.extend(scan(&in_store, FORBIDDEN_IN_STORE, &root));
    // The store may use `fs`, but a group must not smuggle in `process`.
    failures.extend(
        scan_std_groups(&in_store, &root).into_iter().filter(|h| h.contains("process")),
    );

    assert!(
        failures.is_empty(),
        "forbidden APIs found:\n{}",
        failures.join("\n")
    );
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

    assert!(
        failures.is_empty(),
        "forbidden dependencies found:\n{}",
        failures.join("\n")
    );
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
    assert!(failures.is_empty(), "opener guard failed:
{}", failures.join("
"));

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

fn capability_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = fs::read_dir(dir).expect("read capabilities dir");
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            capability_files(&path, out);
        } else if path
            .extension()
            .is_some_and(|e| e == "json" || e == "json5" || e == "toml")
        {
            out.push(path);
        }
    }
}

#[test]
fn capabilities_grant_only_allowlist() {
    let dir = manifest_dir().join("capabilities");
    let mut files = Vec::new();
    capability_files(&dir, &mut files);
    assert!(!files.is_empty(), "no capability files found");

    let mut granted = std::collections::BTreeSet::new();
    let expected_windows = serde_json::json!(["mouce", "home", "firstrun", "chat", "circle"]);

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
        assert_eq!(
            value.get("windows"),
            Some(&expected_windows),
            "{name}: capability windows must be exactly mouce, home, firstrun, chat, circle"
        );
        let perms = value
            .get("permissions")
            .and_then(|p| p.as_array())
            .expect("capability has a permissions array");
        for perm in perms {
            let id = match perm {
                serde_json::Value::String(s) => s.clone(),
                serde_json::Value::Object(o) => o
                    .get("identifier")
                    .and_then(|i| i.as_str())
                    .expect("permission object has identifier")
                    .to_string(),
                other => panic!("unexpected permission entry: {other}"),
            };
            granted.insert(id);
        }
    }

    let expected: std::collections::BTreeSet<String> =
        ["core:default", "core:window:allow-start-dragging"]
            .iter()
            .map(|s| s.to_string())
            .collect();
    assert_eq!(granted, expected, "capabilities must grant exactly the allowlist");
}

#[test]
fn tauri_conf_has_no_inline_capabilities() {
    let text = fs::read_to_string(manifest_dir().join("tauri.conf.json"))
        .expect("read tauri.conf.json");
    let conf: serde_json::Value = serde_json::from_str(&text).expect("parse tauri.conf.json");
    let inline = conf.pointer("/app/security/capabilities");
    let empty = match inline {
        None => true,
        Some(v) => v.as_array().is_some_and(|a| a.is_empty()),
    };
    assert!(
        empty,
        "tauri.conf.json must not define inline app.security.capabilities: {inline:?}"
    );
}

/// The Win32 API areas the `windows` crate may expose. Adding one (for example file system,
/// shell, clipboard/data exchange or keyboard and mouse input) must be a deliberate edit of
/// this list, reviewed against the safety promise.
///
/// `Win32_Graphics_Gdi` is here for "Look at my screen": `screen.rs` copies screen pixels
/// into a memory bitmap. GDI draws and reads pixels; it cannot touch files, input or the
/// other programs' windows.
const WINDOWS_FEATURES: &[&str] =
    &["Win32_Foundation", "Win32_Graphics_Gdi", "Win32_UI_WindowsAndMessaging"];

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
    let text = fs::read_to_string(manifest_dir().join("tauri.conf.json"))
        .expect("read tauri.conf.json");
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
