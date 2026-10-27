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
    failures.extend(scan(&in_store, FORBIDDEN_IN_STORE, &root));

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
    let expected_windows = serde_json::json!(["star", "settings", "firstrun"]);

    for path in &files {
        let name = path.display();
        assert!(
            path.extension().is_some_and(|e| e == "json"),
            "{name}: only plain .json capability files are supported; .json5/.toml are not allowed"
        );
        let text = fs::read_to_string(path).expect("read capability file");
        let value: serde_json::Value = serde_json::from_str(&text).expect("parse capability json");
        assert_eq!(
            value.get("windows"),
            Some(&expected_windows),
            "{name}: capability windows must be exactly star, settings, firstrun"
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
