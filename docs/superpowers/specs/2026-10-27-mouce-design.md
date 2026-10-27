# Mouce — Design

*Working name: "Mouce". Repo: `C:\Projectss\mouce` (empty git repo). Date: 2026-10-27.*
*Once approved, this document is saved to `docs/superpowers/specs/2026-10-27-mouce-design.md` and committed. The next step is the implementation plan (superpowers:writing-plans).*

## Context and intent

The user wants a tiny companion that lives on their Windows desktop and helps them **understand whatever is on their screen** without leaving what they're doing. The moment that started it: they were reading a document, came across an image with no description, and wished they could press a few keys, circle the image with the mouse, get a description, and then chat about it.

What makes it special is that it's always there, out of the way, and **only looks, never touches**. It explains, defines and remembers things for you, but it can't change anything on your laptop.

Success: it's always running in the corner, and when the user doesn't understand an image, a word or a window, getting an answer and asking follow-ups takes a couple of seconds and one gesture. When they say "add to calendar" (in chat or on a circled capture), the event lands on a calendar that reminds them at sensible times. The app **never proposes events on its own**: it doesn't scan chats or watch the screen for dates.

## Goals (user's words, confirmed through the playbacks)

1. **The mouce.** A very small, slowly rotating mouce in the corner of the desktop. Standalone app.
2. **Circle to look up.** Hold a key combo, draw a circle around anything on screen, get an AI description, then keep chatting about it.
3. **Dictionary.** Right-click a word in any app (with Ctrl held) to get a definition card.
4. **Chat helper.** Click the mouce to chat, mainly about what's on screen: an open app, a window, a Word doc, something they don't understand.
5. **Heavy safety guard rails.** It never deletes or updates anything on the laptop: files, documents, anything.
6. **Smart calendar with smart reminders.** Add events by chatting ("add this to calendar") or by circling something on screen and saying "add to calendar". It reminds you ahead of each event at a sensible time.
7. **Choice of AI.** A local model on the laptop, or an API key for OpenAI, Groq, Gemini or Claude.

## Non-goals (v1)

- Acting on the computer for the user: no opening, editing, moving or deleting files, no running commands, no typing or clicking into other apps.
- Syncing with Google, Outlook or any other calendar. The calendar is laptop-only. Read-only sync may come later.
- macOS or Linux. Windows 11 only for now.
- Languages other than English. v1 is **English only** (user's decision): dictionary, OCR language and prompts.
- An OS-level sandbox (AppContainer or a low-integrity process). That would stop UI Automation from reading words in Word or Chrome. Safety is enforced by the app's own permission system and code structure (see the safety model).
- Auto-update. It would mean the app rewriting itself, which goes against the spirit of the rules.

## Anti-goals (this would be a failure even if it technically worked)

- Any code path that could delete, edit, move or overwrite a user file, even "by accident" or because the AI asked for it.
- The mouce getting in the way: stealing keyboard focus, covering content, eating CPU or battery, or nagging.
- Events saved to the calendar without the user seeing and confirming them.
- Screen captures saved or sent anywhere the user didn't choose.
- Hijacking common shortcuts or the normal right-click menu.

## Constraints and environment (checked on the laptop)

- Windows 11 Home, 15.3 GB RAM, integrated AMD Radeon graphics (no discrete GPU).
- **Ollama is installed, with no models yet.** Rust (`rustc` via cargo) and Node v24 are installed.
- Local vision models will be slow on this hardware (seconds per answer). The design has to feel OK with that: streaming text and a visible "thinking" state.

## Decisions

### Made by the user (in conversation)
- Circle-to-look-up flow as played back: hotkey, draw a circle, a capture of just that area goes to the AI, the answer opens in chat with follow-ups.
- Chat is for **understanding what's on screen**, not for operating the laptop.
- Safety model as played back: look but never touch. The app only writes to its own private folder. Lockdown is enforced at the system level, keys go in the Windows secure store, nothing is saved by default, and an indicator shows when it's looking.
- Calendar as played back: natural-language and circle-to-add, a confirm card before saving, AI-chosen reminder timing, the mouce nudges plus a Windows notification with snooze, a month/week view, **laptop-only**.
- Mouce as played back: very small, rotating, in a corner, always on top, doesn't steal focus, draggable, has a tray icon, starts with Windows.
- AI settings as played back: Local (Ollama) or OpenAI / Groq / Gemini / Claude keys. First run offers a small local vision model.
- Build order: mouce + settings → chat → circle-to-look-up → dictionary → calendar.
- Tech left to Claude.
- **Picking an online provider in Settings counts as permission** to send captures to it. There's no per-capture confirm, but the chat shows a provider badge on every capture so it's always clear where the image went.
- **English only** for v1.
- **It will be shared with other people**, not just used on this laptop. That means a polished installer, code signing, and a first run that works on machines without Ollama.

### Made by Claude (the user can override any of these)
- **Stack: Tauri 2** (Rust backend, WebView2 frontend) with **React + TypeScript + Vite** for the UI. Reason: a small installer and low memory for an always-on app, plus Rust for the Windows-level work (input hooks, screen capture, UI Automation).
- **Circle hotkey: `Ctrl+Alt+Space`** (configurable). Not Ctrl+Shift+S, which is Save As, and not Win+Shift+S, which is the Snipping Tool.
- **Dictionary trigger: Ctrl + right-click.** A low-level mouse hook swallows that one click so the app's own menu doesn't open. A plain right-click is never touched.
- **Selected text is read through UI Automation** (read-only), **never by simulating Ctrl+C**, because that would overwrite the user's clipboard.
- **AI calls happen in Rust, not in the web UI**, so API keys never reach the webview.
- **Storage: SQLite** in the app's own data folder (`%APPDATA%\<app id>\`) holds events, reminders, settings and opt-in chat history.
- **Default size and position:** about 36 px, bottom-right above the taskbar. Rotation can be turned off.
- **Default local model:** the best small vision-capable model Ollama offers when building (Gemma 3 4B or Qwen-VL 3B class). It is downloaded only after the user confirms on first run.
- **"System-level lockdown" means the app's own enforcement**: Tauri's capability system (no file, shell or program access is granted) plus a single disk-writer module and guard tests. It does *not* mean an OS sandbox, which would break the dictionary. The app is *unable* to touch files, not just asked not to.
- **Repeating events in v1:** daily, weekly, monthly or yearly, with an optional end date, in the database schema from the start. Reminders follow each occurrence.
- **First run on machines without Ollama:** detect that it's missing and offer either "Set up an online AI (paste a key)" or "Get Ollama", which opens ollama.com in the browser. The only URL-opening allowed is fixed https links like this one, never file paths. If neither is set up, the offline dictionary still works and the other features show a friendly "connect an AI" prompt.
- **Code signing:** the build pipeline has a signing step ready (Tauri supports Windows signing, e.g. Azure Trusted Signing or an OV certificate). **Buying the certificate is the user's call later**; until then builds are unsigned and SmartScreen will warn. The low-level hook and screen capture can trigger antivirus heuristics; signing reduces that.
- **Start with Windows** is a first-run toggle, on by default. It adds the app's own Run registry entry, the **only** system setting the app ever changes, and the user explicitly agrees to it.

## How it works

### Windows (Tauri multi-window)
- **Mouce window:** tiny, transparent, frameless, always on top, hidden from the taskbar and Alt+Tab, and doesn't take focus when clicked (`WS_EX_NOACTIVATE`). It's an SVG mouce with a CSS rotation, which runs on the GPU and costs almost nothing. States: idle (slow spin), thinking (faster spin), looking (dimmed/pulsing), reminder due (glow + speech bubble). Drag to move; the position is saved.
- **Chat panel:** a small window anchored next to the mouce, which takes focus only when the user clicks into it. It has a "Look at my screen" button and a model and provider badge.
- **Capture overlay:** a full-screen transparent window across all monitors, showing a frozen screenshot. The user draws a lasso with a glowing trail. On release, the lasso's bounding box is cropped (pixels outside the lasso are masked) and the overlay closes.
- **Dictionary card:** a small popup at the cursor.
- **Calendar window:** month/week view and an upcoming-events list.
- **Settings:** provider and model, API keys, hotkey, mouce size/rotation, start-with-Windows, history on/off.
- **Tray icon:** Open chat, Calendar, Settings, Pause mouce, Quit.

### Rust backend modules
- `capture`: screen, region and foreground-window capture (e.g. the `xcap` crate). The foreground window is the one active *before* the mouce or chat took focus.
- `input`: a global hotkey (Tauri global-shortcut plugin) and a low-level mouse hook thread for Ctrl+right-click.
- `uia`: Windows UI Automation, used read-only to get the word under the cursor (`TextPattern.RangeFromPoint` expanded to a word) and the current text selection. Fallback when an app exposes no text: OCR on a small area around the cursor using the built-in offline **Windows.Media.Ocr**.
- `ai`: a `Provider` trait with streaming chat, image input and structured-JSON output. Implementations: Ollama (`localhost:11434`), OpenAI, Groq (OpenAI-compatible), Gemini, Anthropic. Optional fallback to local when the online provider fails.
- `store`: the **only** module allowed to write to disk, and it writes only inside the app data folder (SQLite).
- `secrets`: API keys in Windows Credential Manager (`keyring` crate).
- `reminders`: a scheduler thread that triggers due reminders, shows a Windows toast (Tauri notification plugin) and signals the mouce. Snooze options are 5 / 15 / 60 minutes. Reminders missed while the laptop was off are shown at the next start.

### Safety model (defense in depth)
1. **No capability to act.** The Tauri capability config grants no `fs`, `shell`, or `opener`-for-files plugins, and there is no command that runs programs. The frontend can only call our own whitelisted commands.
2. **The AI has no tools.** The only structured output it can produce is a *proposed* calendar event, which always goes to a confirm card. Screen content may contain prompt-injection text; because the AI can't act, the worst outcome is a wrong answer or a bad proposal the user rejects.
3. **Single writer.** All disk writes go through `store`, which resolves paths only under the app data folder and rejects anything else.
4. **Guard tests in CI and the test suite:** a test that scans the Rust source for forbidden APIs outside `store` (`std::fs::remove*`, `write`, `rename`, `copy`, `Command::new`, `ShellExecute`, `SendInput`/keystroke simulation, clipboard writes), plus a test that the `store` path guard rejects paths outside the app folder.
5. **Network allowlist:** the CSP and Rust HTTP client allow only the configured provider hosts and `localhost:11434`.
6. **Privacy:** captures stay in memory and are discarded after the conversation unless chat history is turned on. A visible "looking" state appears on every capture.

### Feature flows
- **Circle to look up:** `Ctrl+Alt+Space` → overlay → lasso → crop → chat opens with the image thumbnail → AI streams a description → follow-ups keep the image in context. Quick action buttons: "Describe", "Explain", "Extract text", "Add to calendar".
- **Dictionary:** Ctrl+right-click a word → the card shows the word, pronunciation (phonetic text, plus a play button using the WebView2 `speechSynthesis` built-in voices), part of speech, meaning(s), an example sentence, and a "Chat about this" button. With AI available it gives a context-aware meaning (it sees the sentence around the word). **Offline fallback:** a bundled open dictionary (WordNet-derived).
- **Chat:** click the mouce → panel. "Look at my screen" captures the previous foreground window; selected text from that window is attached automatically if present. Answers stream in.
- **Calendar:** in chat ("add dentist next Tue 3pm") or "Add to calendar" on a capture → the AI returns JSON `{title, start, end?, location?, notes?, kind, suggested_reminders[]}`, given the current date and timezone → validated → **confirm card** (Save / Edit / Cancel). Reminder timing: the AI suggests based on the event type, with a rules table as fallback (meeting 15 min; appointment 1 h; exam/deadline the evening before + that morning; flight/travel 3 h; birthday the day before). The user can edit reminders on the card or in the calendar.

## Build order (milestones; each one is usable on its own)
1. **Skeleton + safety:** Tauri app, mouce window (rotating, draggable, no focus steal), tray, settings, provider layer + key storage, `store`, capability lockdown, guard tests, first-run (start-with-Windows toggle, optional Ollama model download).
2. **Chat:** panel, streaming, "Look at my screen", selected-text attach.
3. **Circle to look up:** hotkey, overlay, lasso, crop, quick actions.
4. **Dictionary:** mouse hook, UIA word-under-cursor, OCR fallback, card, offline dictionary.
5. **Calendar + reminders:** SQLite schema, event extraction, confirm card, calendar view, scheduler, toasts, mouce nudge, snooze.
6. **Packaging and sharing:** Windows installer (Tauri NSIS bundle), signing step wired in (active once a certificate exists), first-run tested on a machine/user without Ollama, plus a short README for other users.

## Left to the builder
- Exact crates and versions, the folder layout, and React state management.
- Visual polish of the mouce and panels (keep it small, calm and pleasant; no mascot face unless asked).
- Prompt wording for describe, explain, define and event extraction.
- Choice of offline dictionary dataset (licence must allow bundling).
- Multi-monitor and DPI handling details.
- Exact local model tag.

## Verification
- `cargo test` (Rust): provider request formatting against mocked HTTP for all 5 providers; event JSON parsing and validation; reminder scheduling (due, snooze, missed-while-off); the `store` path guard; the forbidden-API source scan.
- `npm test` (Vitest): confirm card, settings, chat rendering.
- `npm run tauri dev`, then a manual end-to-end checklist:
  - The mouce sits in the corner and rotates, clicking it doesn't steal focus from Word, and dragging it works and is remembered after a restart.
  - Circle an image in a document: a description arrives and a follow-up question works, on Ollama and on at least one API provider.
  - Ctrl+right-click a word in Word, Chrome and a PDF viewer: the card appears and the app's own menu doesn't. A plain right-click still works normally.
  - "Look at my screen" explains the previous window, and the clipboard is unchanged afterwards.
  - "Add dentist tomorrow 3pm": the confirm card appears, and the reminder fires as a toast plus a mouce glow; snooze works.
  - Safety spot-checks: ask the AI to delete or edit a file and confirm nothing happens and nothing can; check Task Manager for idle CPU and memory.
- Recurring event: a yearly birthday and a weekly class each remind correctly on the next occurrence.
- Build the installer, then install and launch on a clean user session **with Ollama not available**: the first-run choices appear, the dictionary works offline, and other features show the "connect an AI" prompt.
