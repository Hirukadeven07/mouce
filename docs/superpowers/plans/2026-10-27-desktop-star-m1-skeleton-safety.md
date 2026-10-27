# Desktop Star — Milestone 1 (Skeleton + Safety) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A running Windows app that shows a tiny rotating star in the bottom-right corner, has a tray menu, a Settings window and a first-run window, can talk to all five AI providers, and enforces the "look but never touch" safety model with automated guard tests.

**Architecture:** Tauri 2 app. The Rust backend owns everything privileged: storage (`store`, the only disk writer), API keys (`secrets`), AI calls (`ai`) and window and tray management. The React frontend is one Vite bundle that renders a different screen depending on the window label (`star`, `settings`, `firstrun`) and talks to Rust only through our own `#[tauri::command]`s. The capability file grants the webview nothing beyond core defaults and window dragging.

**Tech Stack:** Tauri 2, Rust (stable), React 18 + TypeScript + Vite, rusqlite (bundled), keyring 3, reqwest (rustls, stream), async-trait, tokio, wiremock (dev), windows crate (Win32 window styles), tauri-plugin-autostart, tauri-plugin-opener, tauri-plugin-single-instance, Vitest + @testing-library/react + jsdom.

**Spec:** `docs/superpowers/specs/2026-10-27-desktop-star-design.md`. Read it before starting. This plan covers its build-order item 1 only. Milestones 2–6 each get their own plan.

## Global Constraints

- Windows 11 only. Tauri **2.x**; use the newest 2.x of each crate/package at install time (the registry was unreachable while planning, so versions aren't pinned here).
- **No file, shell, clipboard or program access anywhere.** Do not add `tauri-plugin-fs`, `tauri-plugin-shell`, `tauri-plugin-clipboard-manager`, or `tauri-plugin-dialog`. Do not call `std::process::Command`, `ShellExecute*`, `SendInput`, `keybd_event`, `SetClipboardData`. The guard tests in Task 1 enforce this.
- All disk writes go through `store` (Task 2) and only under the app data dir (`%APPDATA%\com.desktopstar.app\`).
- AI HTTP calls are made in Rust only. API keys never cross IPC back to the webview (only "has key: yes/no").
- Network hosts allowed: `api.openai.com`, `api.groq.com`, `generativelanguage.googleapis.com`, `api.anthropic.com`, and loopback (`localhost`, `127.0.0.1`) for Ollama. HTTPS required for every non-loopback host.
- The only URL the app may open in the browser is the constant `https://ollama.com/download`.
- App identity: productName `Star`, identifier `com.desktopstar.app`.
- Star default: about 36 px (size setting: small 28 / medium 36 / large 48 logical px), bottom-right of the primary monitor's work area, 16 px margin, always on top, no taskbar entry, never takes focus (`WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW`). Rotation on by default and can be turned off.
- Circle hotkey setting default `Ctrl+Alt+Space` (stored only in M1; registered in M3).
- Start with Windows: a first-run checkbox, **on by default**. It's the only system setting the app changes.
- Default local model tag: `gemma3:4b` (spec leaves the exact tag to the builder; confirm it's still a good small vision model in `ollama` at build time and change the constant if not). It's downloaded only after the user clicks.
- English-only UI copy.

## Review Focus

1. **Taskbar not at the bottom, or display scaling at 125%/150%:** the star must still land inside the primary monitor's *work area* (not under the taskbar) at the right physical size. Test: `default_position_respects_offset_work_area_and_scale` (Task 8).
2. **Saved star position on a monitor that's since been unplugged:** the star must come back to the default corner, not stay invisible off-screen. Test: `clamp_rejects_position_off_all_monitors` (Task 8).
3. **API key pasted with spaces or a trailing newline:** it must be stored trimmed, and an empty or whitespace-only key must be rejected. Test: `set_key_trims_and_rejects_blank` (Task 3).
4. **Ollama not running or not installed:** the first run should show a clear state (`NotInstalled` / `NotRunning`), not a raw connection error. Test: `status_not_running_when_connection_refused` (Task 7).
5. **Provider returns 401/429 with a JSON error body:** the user should see a readable message like "HTTP 401: invalid x-api-key", not "parse error". Test: `http_error_surfaces_provider_message` (Task 4), reused by every provider test file.

---

## File Structure

```
package.json, vite.config.ts, tsconfig.json, index.html, .gitignore
src/
  main.tsx                 # picks screen by window label
  lib/ipc.ts               # typed invoke/listen wrappers (the only file importing @tauri-apps/api)
  lib/types.ts             # TS mirrors of Rust Settings / ProviderId / OllamaStatus
  star/Star.tsx, star.css
  settings/Settings.tsx
  firstrun/FirstRun.tsx
  test/setup.ts
  **/*.test.tsx
src-tauri/
  Cargo.toml, build.rs, tauri.conf.json
  capabilities/default.json
  src/main.rs              # calls desktop_star_lib::run()
  src/lib.rs               # Builder, plugins, setup, command registration
  src/store/mod.rs         # AppStore: path guard + SQLite kv  (ONLY disk writer)
  src/settings.rs          # Settings struct, defaults, load/save via AppStore
  src/secrets.rs           # keyring wrapper
  src/ai/mod.rs            # types, Provider trait, provider_for, AiError
  src/ai/net.rs            # allowlisted reqwest client, LineBuffer, error_from_response
  src/ai/ollama.rs
  src/ai/openai_compat.rs  # OpenAI + Groq
  src/ai/gemini.rs
  src/ai/anthropic.rs
  src/ollama_setup.rs      # status + pull
  src/star/mod.rs          # star window creation, styles, move persistence
  src/star/position.rs     # pure positioning math
  src/tray.rs
  src/commands.rs          # every #[tauri::command]
  tests/safety_scan.rs     # forbidden-API + dependency + capability guards
```

---

### Task 1: Scaffold + safety guard tests

**Files:**
- Create: everything the Tauri `react-ts` template creates, plus `.gitignore` (`node_modules/`, `dist/`, `src-tauri/target/`, `src-tauri/gen/`)
- Create: `src-tauri/capabilities/default.json` (replace template's)
- Create: `src-tauri/tests/safety_scan.rs`
- Modify: `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml`

**Interfaces:**
- Produces: crate name `desktop_star_lib` (lib) with `pub fn run()`; window labels `star`, `settings`, `firstrun` (only `star` is created at startup, in Task 8); `npm test` and `cargo test` both working.

- [ ] **Step 1: Scaffold.** The repo isn't empty (`docs/`), so scaffold in a temp dir and move the contents up: `npm create tauri-app@latest star-tmp -- --template react-ts --manager npm --identifier com.desktopstar.app -y`, move everything except `.git` into the repo root, delete `star-tmp`. Rename the Rust lib to `desktop_star_lib` in `Cargo.toml` `[lib] name`.
- [ ] **Step 2: Configure `tauri.conf.json`.** `productName: "Star"`, remove the template's default window from `app.windows` (set `[]`), set `app.security.csp` to `"default-src 'self'; img-src 'self' data:; style-src 'self' 'unsafe-inline'; connect-src ipc: http://ipc.localhost"`, `bundle.targets: ["nsis"]`. Remove `tauri-plugin-opener` usage the template adds to the frontend; the plugin is re-added server-side in Task 9.
- [ ] **Step 3: Write `capabilities/default.json`:**

```json
{
  "identifier": "default",
  "windows": ["star", "settings", "firstrun"],
  "permissions": ["core:default", "core:window:allow-start-dragging"]
}
```

- [ ] **Step 4: Add Vitest.** Dev deps `vitest @testing-library/react @testing-library/user-event @testing-library/jest-dom jsdom`; `vite.config.ts` `test: { environment: "jsdom", setupFiles: "src/test/setup.ts" }`; `"test": "vitest"` script. `src/test/setup.ts` imports `@testing-library/jest-dom/vitest`.
- [ ] **Step 5: Write `src-tauri/tests/safety_scan.rs`** with three tests that read files under `CARGO_MANIFEST_DIR` (reading in tests is fine):

```rust
#[test] fn no_forbidden_apis_outside_store()
// walk src/**/*.rs; skip src/store/; fail listing file:line for any line containing one of:
// "std::fs", "fs::write", "fs::remove", "fs::rename", "fs::copy", "File::create", "OpenOptions",
// "Command::new", "std::process", "ShellExecute", "SendInput", "keybd_event", "mouse_event",
// "SetClipboardData", "clipboard", "open_path", "reveal_item"
// Also fail if src/store/ contains any of: "fs::remove", "fs::rename", "fs::copy", "Command::new", "std::process".

#[test] fn no_forbidden_dependencies()
// Cargo.toml must not contain: "tauri-plugin-fs", "tauri-plugin-shell", "tauri-plugin-clipboard", "tauri-plugin-dialog", "tauri-plugin-process", "tauri-plugin-updater"
// ../package.json must not contain: "@tauri-apps/plugin-fs", "plugin-shell", "plugin-clipboard", "plugin-dialog", "plugin-opener", "plugin-process", "plugin-updater"

#[test] fn capabilities_grant_only_allowlist()
// parse every capabilities/*.json; collect "permissions" (strings, or objects' "identifier");
// assert the set == {"core:default", "core:window:allow-start-dragging"}
```

A comment line `// safety-scan: allow` on the same line exempts one match (for the scanner's own pattern list if it's ever moved into `src/`). Nothing in M1 should need it.

- [ ] **Step 6: Run the guards.** `cd src-tauri && cargo test --test safety_scan`. Expected: 3 passed. Then temporarily add `use std::process::Command;` to `src/lib.rs`, rerun, and expect `no_forbidden_apis_outside_store` to FAIL naming `src/lib.rs`. Remove the line.
- [ ] **Step 7: Sanity run.** `npm install`, `npx vitest --run` (no tests yet; it should exit cleanly with `--passWithNoTests`; add that flag to the script), `npm run tauri build -- --debug --no-bundle` compiles.
- [ ] **Step 8: Commit.** `git add -A && git commit -m "chore: scaffold Tauri app with safety guard tests"`

---

### Task 2: `store`, the only disk writer

**Files:**
- Create: `src-tauri/src/store/mod.rs`
- Modify: `src-tauri/src/lib.rs` (`mod store;`), `Cargo.toml` (`rusqlite = { features = ["bundled"] }`, dev `tempfile`)

**Interfaces:**
- Produces:
  - `pub struct AppStore` (holds `root: PathBuf` and `Mutex<rusqlite::Connection>`)
  - `pub fn AppStore::open(root: PathBuf) -> Result<AppStore, StoreError>`: `create_dir_all(root)`, opens `root/star.db`, creates table `kv(key TEXT PRIMARY KEY, value TEXT NOT NULL)`
  - `pub fn AppStore::guard_path(&self, p: &Path) -> Result<PathBuf, StoreError>`: returns the canonical path only if it's inside `root`
  - `pub fn get_kv(&self, key: &str) -> Result<Option<String>, StoreError>`, `pub fn set_kv(&self, key: &str, value: &str) -> Result<(), StoreError>`
  - `pub fn connection(&self) -> MutexGuard<'_, Connection>` (for later milestones' tables)
  - `pub enum StoreError { OutsideAppDir(PathBuf), Io(String), Db(String) }` (thiserror, `Serialize` as string)

- [ ] **Step 1: Write failing tests** (in `store/mod.rs` `#[cfg(test)]`, using `tempfile::tempdir()`):
  - `kv_roundtrip`: `set_kv("a","1")` then `get_kv("a") == Some("1")`; overwrite with `"2"` gives `"2"`; `get_kv("missing") == None`.
  - `guard_accepts_inside_root`: `guard_path(root.join("star.db"))` is Ok.
  - `guard_rejects_outside_and_traversal`: `guard_path(root.join("..").join("evil.txt"))` and `guard_path(Path::new("C:\\Windows\\win.ini"))` are both `Err(OutsideAppDir(_))`.
  - `reopen_persists`: set a key, drop the store, `open` the same root again, and the value is still there.
- [ ] **Step 2: Run** `cargo test store::` and expect FAIL (does not compile).
- [ ] **Step 3: Implement.** `guard_path` must normalise without requiring the file to exist: canonicalise `root` once at open; for the input, canonicalise its parent dir (if the parent doesn't exist, reject) and join the file name. `open` passes the db path through `guard_path` before opening.
- [ ] **Step 4: Run** `cargo test store:: && cargo test --test safety_scan` and expect all to PASS.
- [ ] **Step 5: Commit.** `git commit -am "feat(store): app-dir-guarded SQLite key-value store"`

---

### Task 3: Settings + secrets

**Files:**
- Create: `src-tauri/src/settings.rs`, `src-tauri/src/secrets.rs`
- Modify: `lib.rs`, `Cargo.toml` (`keyring = { features = ["windows-native"] }`, `serde`, `serde_json`)

**Interfaces:**
- Consumes: `AppStore::get_kv/set_kv` (Task 2)
- Produces:
  - `#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash, Debug)] #[serde(rename_all = "lowercase")] pub enum ProviderId { Ollama, OpenAi /* "openai" */, Groq, Gemini, Anthropic }`
  - `pub enum StarSize { Small, Medium, Large }` (lowercase serde), `impl StarSize { pub fn px(self) -> u32 }` → 28/36/48
  - `pub struct Settings { provider: ProviderId, models: HashMap<ProviderId, String>, ollama_base_url: String, star_size: StarSize, star_rotation: bool, star_position: Option<(i32, i32)>, circle_hotkey: String, history_enabled: bool, fallback_to_local: bool, first_run_done: bool }` (all `pub`, `#[serde(default)]` on the struct)
  - `impl Default for Settings`: provider `Ollama`, models `{Ollama: "gemma3:4b"}`, ollama_base_url `"http://localhost:11434"`, star_size `Medium`, star_rotation `true`, star_position `None`, circle_hotkey `"Ctrl+Alt+Space"`, history_enabled `false`, fallback_to_local `true`, first_run_done `false`
  - `pub fn load(store: &AppStore) -> Settings` (key `"settings"`; missing or invalid JSON gives `Settings::default()`), `pub fn save(store: &AppStore, s: &Settings) -> Result<(), StoreError>`
  - `pub struct Secrets { service: String }`, `Secrets::new(service: &str)`, the app uses service `"com.desktopstar.app"`
  - `pub fn set_key(&self, p: ProviderId, key: &str) -> Result<(), SecretError>`, `get_key(&self, p) -> Result<Option<String>, SecretError>`, `has_key(&self, p) -> bool`, `clear_key(&self, p) -> Result<(), SecretError>` (keyring user name = provider's serde name)
  - `pub enum SecretError { Blank, Keyring(String) }`

- [ ] **Step 1: Write failing tests.**
  - `settings.rs`: `defaults_match_spec` (asserts every default listed above); `load_returns_defaults_when_missing_or_corrupt` (empty store gives default; `set_kv("settings","{not json")` gives default); `save_then_load_roundtrip` (change provider to `Anthropic`, size to `Small`, position to `Some((10,20))`, then compare); `partial_json_fills_defaults` (`set_kv("settings", r#"{"provider":"groq"}"#)` gives provider Groq, with everything else default).
  - `secrets.rs` (each test uses a unique service like `"com.desktopstar.app.test.<test name>"` and calls `clear_key` at the end): `set_key_trims_and_rejects_blank` (`set_key(Groq, "  gsk_abc\n")` then `get_key == Some("gsk_abc")`; `set_key(Groq, "   ")` gives `Err(Blank)`); `has_and_clear` (`has_key` false → set → true → clear → false; clearing a missing key is Ok).
- [ ] **Step 2: Run** `cargo test settings:: secrets::` and expect FAIL.
- [ ] **Step 3: Implement** both files to the interfaces.
- [ ] **Step 4: Run** `cargo test` and expect PASS (including safety_scan).
- [ ] **Step 5: Commit.** `git commit -am "feat: settings persistence and Credential Manager key storage"`

---

### Task 4: AI core: types, allowlisted network, stream line parsing

**Files:**
- Create: `src-tauri/src/ai/mod.rs`, `src-tauri/src/ai/net.rs`
- Modify: `lib.rs`, `Cargo.toml` (`reqwest = { default-features = false, features = ["rustls-tls", "json", "stream"] }`, `url`, `async-trait`, `futures-util`, `tokio = { features = ["macros","rt-multi-thread"] }`, `thiserror`, dev `wiremock`)

**Interfaces:**
- Produces (`ai/mod.rs`):
  - `pub enum Role { User, Assistant }`; `pub struct ImageInput { pub mime: String, pub base64: String }`
  - `pub struct ChatMessage { pub role: Role, pub text: String, pub images: Vec<ImageInput> }`
  - `pub struct ChatRequest { pub model: String, pub system: Option<String>, pub messages: Vec<ChatMessage>, pub max_tokens: u32 }`
  - `#[derive(thiserror::Error)] pub enum AiError { NotAllowed(String), MissingKey, Network(String), Http { status: u16, message: String }, Parse(String) }`. `Display` for `Http` is `"HTTP {status}: {message}"`. It derives `Serialize` as the display string.
  - `#[async_trait] pub trait Provider: Send + Sync { async fn chat_stream(&self, req: &ChatRequest, on_delta: &mut (dyn FnMut(&str) + Send)) -> Result<String, AiError>; async fn list_models(&self) -> Result<Vec<String>, AiError>; }`. `chat_stream` returns the full concatenated text.
  - `pub fn provider_for(id: ProviderId, base_url: Option<String>, api_key: Option<String>) -> Result<Box<dyn Provider>, AiError>`: declared here as `todo!()`, implemented in Task 6
- Produces (`ai/net.rs`):
  - `pub fn check_allowed(url: &Url) -> Result<(), AiError>`: allowlist per Global Constraints; non-loopback hosts must be `https`
  - `pub fn client() -> reqwest::Client`: 10 s connect timeout, no overall timeout (streams), redirect policy that rejects any hop failing `check_allowed`
  - `pub async fn send(req: reqwest::RequestBuilder, url: &Url) -> Result<reqwest::Response, AiError>`: `check_allowed`, send, map non-2xx through `error_from_response`
  - `pub async fn error_from_response(resp: reqwest::Response) -> AiError`: `Http{status, message}` where message is the first found of JSON `error.message`, `error` (string), `message`, else the body text truncated to 300 chars
  - `pub struct LineBuffer`, `fn push(&mut self, chunk: &[u8]) -> Vec<String>` (complete lines, `\r\n` or `\n`, empty lines dropped), `fn finish(self) -> Option<String>`
  - `pub fn sse_data(line: &str) -> Option<&str>`: payload after `data:` (one optional space), `None` for other lines

- [ ] **Step 1: Write failing tests** (`net.rs`):
  - `allowlist`: these are allowed: `https://api.openai.com/v1/x`, `https://api.groq.com/openai/v1`, `https://generativelanguage.googleapis.com/v1beta`, `https://api.anthropic.com/v1/messages`, `http://localhost:11434`, `http://127.0.0.1:5555`. These are rejected with `NotAllowed`: `http://api.openai.com` (not https), `https://evil.com`, `https://api.openai.com.evil.com`, `file:///C:/x`.
  - `line_buffer_handles_split_chunks`: pushing `b"data: {\"a\""` then `b":1}\n\ndata: [DONE]\r\n"` yields `["data: {\"a\":1}"]` then `["data: [DONE]"]`.
  - `sse_data_parses`: `sse_data("data: x") == Some("x")`, `sse_data("data:x") == Some("x")`, `sse_data("event: ping") == None`.
  - `http_error_surfaces_provider_message` (async, wiremock on 127.0.0.1): mock returns 401 with `{"error":{"message":"invalid x-api-key"}}`; `send(...)` gives `Http{status:401, message:"invalid x-api-key"}` and `to_string() == "HTTP 401: invalid x-api-key"`. A second mock returns 429 with plain text `slow down` and gives `message == "slow down"`.
- [ ] **Step 2: Run** `cargo test ai::net` and expect FAIL.
- [ ] **Step 3: Implement** `mod.rs` types and `net.rs`.
- [ ] **Step 4: Run** `cargo test ai::` and expect PASS.
- [ ] **Step 5: Commit.** `git commit -am "feat(ai): provider types, allowlisted HTTP client, stream parsing"`

---

### Task 5: Ollama + OpenAI-compatible (OpenAI, Groq) providers

**Files:**
- Create: `src-tauri/src/ai/ollama.rs`, `src-tauri/src/ai/openai_compat.rs`

**Interfaces:**
- Consumes: Task 4's `Provider`, `ChatRequest`, `net::{client, send, LineBuffer, sse_data}`
- Produces: `OllamaProvider::new(base_url: String)`; `OpenAiCompat::new(base_url: String, api_key: String)`; constants `OPENAI_BASE = "https://api.openai.com"`, `GROQ_BASE = "https://api.groq.com/openai"`

Wire formats (tests assert on these exactly):
- **Ollama chat:** `POST {base}/api/chat`, body `{"model", "stream": true, "messages": [{"role": "system"|"user"|"assistant", "content": text, "images": [b64...]}]}`. `images` is omitted when empty; `system` becomes a first `role: "system"` message. The response is NDJSON lines `{"message":{"content":"..."},"done":false}`. Stop at `"done":true`. **List:** `GET {base}/api/tags` gives `models[].name`.
- **OpenAI-compatible chat:** `POST {base}/v1/chat/completions`, header `Authorization: Bearer {key}`, body `{"model", "stream": true, "max_completion_tokens": max_tokens, "messages": [{"role", "content": [{"type":"text","text"}, {"type":"image_url","image_url":{"url":"data:{mime};base64,{b64}"}}]}]}`. Text-only messages may use a plain string `content`. System goes in as `role: "system"`. The response is SSE: `data: {"choices":[{"delta":{"content":"..."}}]}`, terminated by `data: [DONE]`. **List:** `GET {base}/v1/models` gives `data[].id`, sorted.

- [ ] **Step 1: Write failing tests** (wiremock, assert method + path + headers + JSON body via `body_partial_json`):
  - `ollama_streams_and_sends_images`: the request has a user message with one image, and the body has `messages[1].images == ["QUJD"]` and `messages[0].role == "system"`. The mock streams 3 NDJSON lines (`"Hel"`, `"lo"`, then `done:true`). Deltas collected are `["Hel","lo"]` and the return value is `"Hello"`.
  - `ollama_lists_models`: `{"models":[{"name":"gemma3:4b"}]}` gives `["gemma3:4b"]`.
  - `openai_streams_with_bearer_and_image_parts`: the request has the header `authorization: Bearer sk-test`, the image part URL `data:image/png;base64,QUJD`, and an SSE body with two deltas then `[DONE]`. The result is the concatenation.
  - `openai_compat_lists_models_sorted`.
  - `openai_compat_401_maps_to_http_error`: a 401 JSON error gives `AiError::Http{status:401, ..}` (Review Focus 5).
- [ ] **Step 2: Run** `cargo test ai::ollama ai::openai_compat` and expect FAIL.
- [ ] **Step 3: Implement** both providers. Read the response via `bytes_stream()` into `LineBuffer`.
- [ ] **Step 4: Run** and expect PASS.
- [ ] **Step 5: Commit.** `git commit -am "feat(ai): Ollama and OpenAI-compatible providers"`

---

### Task 6: Gemini + Anthropic providers, `provider_for`

**Files:**
- Create: `src-tauri/src/ai/gemini.rs`, `src-tauri/src/ai/anthropic.rs`
- Modify: `src-tauri/src/ai/mod.rs` (`provider_for`)

**Interfaces:**
- Produces: `GeminiProvider::new(base_url, api_key)`, `AnthropicProvider::new(base_url, api_key)`, constants `GEMINI_BASE = "https://generativelanguage.googleapis.com"`, `ANTHROPIC_BASE = "https://api.anthropic.com"`, and `provider_for` fully implemented: `base_url` override else the constant (Ollama's comes from settings); `MissingKey` for any non-Ollama provider without a key.

Wire formats:
- **Gemini chat:** `POST {base}/v1beta/models/{model}:streamGenerateContent?alt=sse`, header `x-goog-api-key: {key}`, body `{"contents":[{"role":"user"|"model","parts":[{"text"},{"inline_data":{"mime_type","data"}}]}], "systemInstruction":{"parts":[{"text"}]}, "generationConfig":{"maxOutputTokens"}}`. The response is SSE `data: {"candidates":[{"content":{"parts":[{"text":"..."}]}}]}` (concatenate all parts). **List:** `GET {base}/v1beta/models` gives `models[].name` with the `models/` prefix stripped, keeping only those whose `supportedGenerationMethods` contains `generateContent`.
- **Anthropic chat:** `POST {base}/v1/messages`, headers `x-api-key: {key}`, `anthropic-version: 2023-06-01`, body `{"model","max_tokens","stream":true,"system"?,"messages":[{"role","content":[{"type":"image","source":{"type":"base64","media_type","data"}},{"type":"text","text"}]}]}`. Images go before text in each message. The response is SSE. Take `data` lines whose JSON `type == "content_block_delta"` and `delta.type == "text_delta"` and append `delta.text`. An event `type == "error"` becomes `AiError::Http{status: 500, message: error.message}`. **List:** `GET {base}/v1/models` with the same headers gives `data[].id`.

- [ ] **Step 1: Write failing tests:** `gemini_streams_with_key_header_and_inline_image`, `gemini_maps_assistant_role_to_model`, `gemini_lists_generate_models_only`, `anthropic_streams_text_deltas_only` (the stream includes `message_start`, `ping`, two `content_block_delta`s, and `message_stop`, and only the deltas are collected), `anthropic_sends_version_header_and_image_first`, `anthropic_stream_error_event_maps_to_http_error`, `provider_for_requires_key_except_ollama` (`provider_for(Anthropic, None, None)` gives `Err(MissingKey)`; `provider_for(Ollama, Some(url), None)` is Ok), `provider_for_rejects_disallowed_override` (`base_url = "https://evil.com"` makes the first `list_models` call return `NotAllowed`).
- [ ] **Step 2: Run** `cargo test ai::` and expect FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test` and expect PASS.
- [ ] **Step 5: Commit.** `git commit -am "feat(ai): Gemini and Anthropic providers, provider factory"`

---

### Task 7: Ollama setup (status + model download)

**Files:**
- Create: `src-tauri/src/ollama_setup.rs`

**Interfaces:**
- Consumes: `net::{client, send, LineBuffer}`
- Produces:
  - `pub const DEFAULT_LOCAL_MODEL: &str = "gemma3:4b";`
  - `#[derive(Serialize)] #[serde(tag = "state", rename_all = "camelCase")] pub enum OllamaStatus { NotInstalled, NotRunning, Running { models: Vec<String> } }`
  - `pub async fn status(base_url: &str, exe_hint: &Path) -> OllamaStatus`: `GET /api/tags`. On success it returns `Running`. On a connect error it returns `NotRunning` if `exe_hint.exists()`, else `NotInstalled`. The `exe_hint` the app passes is `%LOCALAPPDATA%\Programs\Ollama\ollama.exe`; `Path::exists` is read-only.
  - `#[derive(Serialize, Clone)] pub struct PullProgress { pub status: String, pub completed: Option<u64>, pub total: Option<u64> }`
  - `pub async fn pull(base_url: &str, model: &str, on_progress: &mut (dyn FnMut(PullProgress) + Send)) -> Result<(), AiError>`: `POST /api/pull {"model", "stream": true}`, NDJSON. An `{"error": "..."}` line becomes `Err(Http{status:500, message})`, and `{"status":"success"}` ends the pull.
- The app never starts or installs Ollama itself, since that would mean running a program. The UI tells the user what to do instead.

- [ ] **Step 1: Write failing tests:** `status_running_lists_models` (wiremock); `status_not_running_when_connection_refused` (base `http://127.0.0.1:1`, exe_hint = a temp file that exists, gives `NotRunning`); `status_not_installed_when_no_exe` (same base, hint a non-existent path, gives `NotInstalled`); `pull_reports_progress_then_success` (3 lines with completed/total, then success, gives 4 progress callbacks and Ok); `pull_error_line_is_error`.
- [ ] **Step 2: Run** `cargo test ollama_setup` and expect FAIL.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** and expect PASS.
- [ ] **Step 5: Commit.** `git commit -am "feat: Ollama status detection and model download"`

---

### Task 8: Star window + tray

**Files:**
- Create: `src-tauri/src/star/mod.rs`, `src-tauri/src/star/position.rs`, `src-tauri/src/tray.rs`
- Create: `src/main.tsx` (replace template), `src/lib/ipc.ts`, `src/lib/types.ts`, `src/star/Star.tsx`, `src/star/star.css`, `src/star/Star.test.tsx`
- Modify: `src-tauri/src/lib.rs` (setup: open store at `app.path().app_data_dir()`, load settings, `manage(AppState)`, create star, build tray), `Cargo.toml` (`windows = { features = ["Win32_UI_WindowsAndMessaging", "Win32_Foundation"] }`, `tauri = { features = ["tray-icon"] }`, `tauri-plugin-single-instance`)

**Interfaces:**
- Consumes: `AppStore`, `settings::{load, save, Settings, StarSize}`
- Produces:
  - `pub struct AppState { pub store: AppStore, pub settings: Mutex<Settings>, pub secrets: Secrets }` in `lib.rs`
  - `position.rs`: `#[derive(Clone, Copy, Debug, PartialEq)] pub struct Rect { pub x: i32, pub y: i32, pub w: u32, pub h: u32 }`; `pub fn default_position(work_area: Rect, scale: f64, size_logical: u32) -> (i32, i32)` (bottom-right, 16 logical px margin, physical result); `pub fn clamp_to_monitors(pos: (i32, i32), size_physical: u32, work_areas: &[Rect]) -> Option<(i32, i32)>` (Some only if the whole star fits in some work area)
  - `star::create(app: &AppHandle, settings: &Settings) -> tauri::Result<()>` creates the window labelled `star`: url `index.html`, transparent, no decorations, always on top, `skip_taskbar(true)`, `shadow(false)`, `resizable(false)`, `focused(false)`, size = `star_size.px() + 12` logical (room for glow), position = saved and clamped, else default. It then sets `WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW` on `hwnd()` via `GetWindowLongPtrW`/`SetWindowLongPtrW(GWL_EXSTYLE)`. It listens for `WindowEvent::Moved`, debounced 500 ms, and saves `star_position`.
  - `pub fn open_settings(app: &AppHandle)` in `star/mod.rs`: focuses the existing `settings` window or creates it (480×640, decorated, centered, title "Star Settings"). M2 replaces the star click target with chat.
  - Events emitted to the webview: `"star-state"` with payload `"idle" | "thinking" | "looking" | "reminder"`, and `"settings-changed"` with the `Settings` payload.
  - Command `star_clicked()` (in `commands.rs`) calls `open_settings`.
  - Tray (`tray.rs`) menu: **Settings**, **Pause star** (hide/show toggle; the label flips to **Show star**), separator, **Quit**. Left-clicking the tray icon opens Settings. Tooltip "Star".
  - Single-instance: a second launch focuses Settings and exits.
  - TS (`types.ts`): `ProviderId = "ollama"|"openai"|"groq"|"gemini"|"anthropic"`, `StarState = "idle"|"thinking"|"looking"|"reminder"`, and `Settings` mirroring the Rust field names in snake_case (`star_size: "small"|"medium"|"large"`, `star_position: [number, number] | null`, ...).
  - `ipc.ts`: `getSettings(): Promise<Settings>`, `onStarState(cb): Promise<UnlistenFn>`, `onSettingsChanged(cb)`, `starClicked()`, `startDragging()` (wraps `getCurrentWindow().startDragging()`), `windowLabel(): string`. Task 9 adds more.
  - `Star.tsx`: `export function Star(props: { state: StarState; rotate: boolean; sizePx: number })` renders an inline SVG 5-point star with root `className={"star star--" + state + (rotate ? "" : " star--still")}` and `width = height = sizePx`. A `StarWindow` wrapper wires up the events, and the pointer logic is: pointerdown records the point; moving more than 3 px calls `startDragging()`; pointerup without a drag calls `starClicked()`.
  - `star.css`: `.star` rotates 360° over 12 s linear infinite; `--thinking` 2 s; `--looking` opacity 0.55 + pulse; `--reminder` drop-shadow glow; `.star--still` `animation: none`; `@media (prefers-reduced-motion: reduce)` stops rotation; `html, body` background transparent.

- [ ] **Step 1: Write failing Rust tests** (`position.rs`):
  - `default_position_bottom_right_100pct`: work area `{0,0,1920,1040}`, scale 1.0, size 48 gives `(1920-16-48, 1040-16-48) = (1856, 976)`.
  - `default_position_respects_offset_work_area_and_scale`: taskbar on the left, so the work area is `{60,0,2500,1600}`, scale 1.5, size 48 gives physical size 72 and margin 24, so `(60+2500-24-72, 1600-24-72) = (2464, 1504)`.
  - `clamp_keeps_visible_position`: `(100,100)`, size 72, monitor `{0,0,1920,1040}` gives `Some((100,100))`.
  - `clamp_rejects_position_off_all_monitors`: `(3000,200)` with only `{0,0,1920,1040}` gives `None`; a position that's partly off the edge, like `(1900,100)` with size 72, also gives `None`.
- [ ] **Step 2: Write a failing Vitest** `Star.test.tsx`: `renders state class` (`state="thinking"` gives the class `star--thinking`); `still when rotation off` (`rotate={false}` gives `star--still`); `size applied` (`sizePx={28}` gives svg width `"28"`).
- [ ] **Step 3: Run** `cargo test star::position` and `npx vitest --run src/star` and expect FAIL.
- [ ] **Step 4: Implement** `position.rs`, `Star.tsx`, `star.css`, and `main.tsx` (switch on `windowLabel()`: `star` gives `<StarWindow/>`, `settings` gives `<Settings/>`, `firstrun` gives `<FirstRun/>`; the last two are placeholders until Task 9).
- [ ] **Step 5: Run the tests** and expect PASS.
- [ ] **Step 6: Implement** `star/mod.rs`, `tray.rs`, `star_clicked`, `lib.rs` setup and single-instance. If `Monitor::work_area()` isn't available in the installed Tauri 2.x, get the work area via `GetMonitorInfoW` (add `Win32_Graphics_Gdi` to the windows features).
- [ ] **Step 7: Manual check.** `npm run tauri dev`:
  - The star appears bottom-right above the taskbar and rotates, with no taskbar button and no Alt+Tab entry.
  - Click into Notepad, type, then click the star: Settings opens, and Notepad's caret was never stolen by the *star* itself (only the Settings window takes focus).
  - Dragging moves the star. Quit from the tray and relaunch: it comes back where you left it.
  - Tray Pause hides it and Show brings it back.
- [ ] **Step 8: Run** `cargo test` (safety_scan still green) and **commit.** `git commit -am "feat: rotating no-focus star window, tray, position persistence"`

---

### Task 9: Settings + first-run windows and their commands

**Files:**
- Create: `src-tauri/src/commands.rs` (move `star_clicked` here if not already), `src/settings/Settings.tsx`, `src/settings/Settings.test.tsx`, `src/firstrun/FirstRun.tsx`, `src/firstrun/FirstRun.test.tsx`
- Modify: `src/lib/ipc.ts`, `src/lib/types.ts`, `src-tauri/src/lib.rs` (register commands and plugins `tauri-plugin-autostart`, `tauri-plugin-opener`; on startup, if `!first_run_done`, open `firstrun` 520×600 centered and titled "Welcome to Star"), `Cargo.toml`

**Interfaces:**
- Consumes: everything above
- Produces these Tauri commands. Errors are returned as `String`, using the error's Display:
  - `get_settings() -> Settings`
  - `update_settings(settings: Settings) -> Settings`: saves, emits `"settings-changed"`, and if `star_size` changed, resizes the star
  - `set_api_key(provider: ProviderId, key: String) -> ()`; `has_api_key(provider) -> bool`; `clear_api_key(provider) -> ()`. **There is no `get_api_key` command.**
  - `list_models(provider: ProviderId) -> Vec<String>`
  - `test_provider(provider: ProviderId) -> String`: sends the system prompt `"You are a connection test."` and the user message `"Reply with exactly: ready"` with `max_tokens` 16, using `settings.models[provider]`, and returns the reply text. It emits `"star-state"` `thinking` and then `idle`.
  - `ollama_status() -> OllamaStatus`; `pull_ollama_model(model: String) -> ()` emits `"ollama-pull-progress"` with `PullProgress`
  - `open_ollama_download() -> ()`: calls the opener plugin's `open_url` with `const OLLAMA_DOWNLOAD_URL: &str = "https://ollama.com/download"` and takes **no URL argument**
  - `get_autostart() -> bool`, `set_autostart(enabled: bool) -> ()` (autostart plugin's Rust `ManagerExt`)
  - `finish_first_run() -> ()`: sets `first_run_done = true`, saves, closes `firstrun`
- `ipc.ts` adds a matching camelCase wrapper for each command plus `onPullProgress(cb)`.

UI copy (fixed):
- Settings sections: **AI** (provider select with options "Local (Ollama)", "OpenAI", "Groq", "Gemini", "Claude"; a model select filled from `listModels`, with a "Refresh" button; for non-Ollama, a password-type key input with "Save key" and "Remove key", status "Key saved" / "No key yet", and the key itself never shown; a "Test connection" button showing `ready`-style output or the error; checkbox "If the online AI fails, use local"), **Star** (size Small/Medium/Large, checkbox "Rotate"), **Shortcuts** (read-only text "Circle to look up: Ctrl+Alt+Space", editable in M3), **Privacy** (checkbox "Keep chat history", off by default, and the line "Screen captures are only sent to the AI you pick above. Star cannot open, change, or delete your files."), **Startup** (checkbox "Start Star when Windows starts").
- First run, three steps: (1) "Hi, I'm Star." plus the "Start Star when Windows starts" checkbox, checked; (2) "Choose your AI". Under `NotInstalled` it shows the buttons "Get Ollama (free, runs on this laptop)" (calls `openOllamaDownload`) and "Use an online AI". Under `NotRunning` it shows "Ollama is installed but not running — start it from the Start menu, then press Check again" with buttons "Check again" and "Use an online AI". Under `Running` without the default model it shows "Download gemma3:4b (about 3 GB)" with a progress bar and "Skip". "Use an online AI" reveals the provider and key fields from Settings, reused as a component `AiSettings`. (3) "All set" with a "Done" button, which calls `setAutostart(checked)` then `finishFirstRun()`.

- [ ] **Step 1: Write failing Vitest tests** (`vi.mock("../lib/ipc")`):
  - `Settings.test.tsx`: `lists five providers`; `saving a key trims and calls setApiKey` (choose "Claude", type `"  sk-ant-x  "`, click Save key, and `setApiKey` is called with `("anthropic", "sk-ant-x")`; the UI shows "Key saved" and the input is cleared); `never displays a stored key` (when `hasApiKey` gives true, no input has a non-empty value); `test connection shows error text` (`testProvider` rejects with `"HTTP 401: invalid x-api-key"`, which appears on screen); `toggling rotate calls updateSettings with star_rotation false`.
  - `FirstRun.test.tsx`: `not installed offers download and online` (`ollamaStatus` gives `{state:"notInstalled"}`, both buttons are visible, and clicking "Get Ollama…" calls `openOllamaDownload`); `running without model offers gemma download` (`{state:"running", models:[]}` shows "Download gemma3:4b"); `running with model skips download` (`models:["gemma3:4b"]`, so no download button); `done applies autostart and finishes` (go through the steps with the checkbox left checked, and `setAutostart(true)` is called before `finishFirstRun()`).
- [ ] **Step 2: Run** `npx vitest --run` and expect FAIL.
- [ ] **Step 3: Implement** `commands.rs`, the plugin registration, and the three React screens. The component is `export function AiSettings(props: { compact?: boolean })`, shared by both screens.
- [ ] **Step 4: Run** `npx vitest --run` and `cd src-tauri && cargo test` and expect all PASS. That includes `safety_scan`, which proves opener is used only through the constant-URL command and that no `plugin-opener` npm package was added.
- [ ] **Step 5: Commit.** `git commit -am "feat: settings and first-run screens with provider setup"`

---

### Task 10: Milestone 1 end-to-end verification

**Files:** none (fix-ups only if something fails, each with its own test first)

- [ ] **Step 1: Full test run.** `npx vitest --run` and `cd src-tauri && cargo test`, all green. Record the counts.
- [ ] **Step 2: Fresh first run.** Delete `%APPDATA%\com.desktopstar.app` by hand (the developer does this, not the app), then `npm run tauri dev`. Check: the first-run window appears; Ollama is detected as running (it's installed on this laptop, so start it first); "Download gemma3:4b" shows progress and completes; Done; the star appears.
- [ ] **Step 3: Providers.** In Settings, Test connection on Ollama gives "ready" (or similar). Then do one online provider with a real key, which the user supplies. Then use a wrong key and check you get a readable HTTP 401 message.
- [ ] **Step 4: Safety spot-checks.** `capabilities/default.json` has only the two permissions. In the devtools console of the star window, check that `window.__TAURI_INTERNALS__.invoke("plugin:fs|read_file", {path:"C:\\Windows\\win.ini"})` rejects (plugin not present). In Credential Manager → Windows Credentials, the key appears under `com.desktopstar.app`.
- [ ] **Step 5: Resource check.** Task Manager with the star idle: CPU ≈ 0–1%, memory recorded in the summary.
- [ ] **Step 6: Autostart.** Start-with-Windows on gives a `Star` entry under Task Manager → Startup apps. Toggle it off in Settings and the entry disappears.
- [ ] **Step 7: Commit any fixes,** then tag `git tag m1-skeleton`.
