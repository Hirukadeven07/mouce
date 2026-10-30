<p align="center">
  <img src="images/mouce-banner.jpg" alt="mouce" width="100%">
</p>

<h1 align="center">mouce</h1>

<p align="center">
  <strong>A tiny desktop companion that helps you understand what's on your screen, without ever touching your files.</strong>
</p>

<p align="center">
  <img alt="Platform: Windows 11" src="https://img.shields.io/badge/platform-Windows%2011-0078D4?logo=windows11&logoColor=white">
  <img alt="Built with Tauri 2" src="https://img.shields.io/badge/Tauri-2-24C8DB?logo=tauri&logoColor=white">
  <img alt="Rust" src="https://img.shields.io/badge/Rust-backend-000000?logo=rust&logoColor=white">
  <img alt="React and TypeScript" src="https://img.shields.io/badge/React%20%2B%20TypeScript-UI-3178C6?logo=typescript&logoColor=white">
  <img alt="Status: early development" src="https://img.shields.io/badge/status-early%20development-orange">
</p>

<p align="center">
  <a href="#features">Features</a> ·
  <a href="#the-safety-promise">Safety</a> ·
  <a href="#ai-providers">AI providers</a> ·
  <a href="#getting-started">Getting started</a> ·
  <a href="#roadmap">Roadmap</a>
</p>

---

## Overview

mouce is a small, always-on-top companion that sits in a corner of your Windows desktop. It
never takes keyboard focus from the app you're working in. When you want help, it asks the AI
you choose: a free local model through [Ollama](https://ollama.com), or an online provider with
your own API key.

## Features

- **Stays out of your way.** Small, transparent and always on top. It never steals focus,
  stays out of the taskbar and Alt+Tab, and remembers where you dragged it.
- **Chat beside your work.** Click mouce, or press **Ctrl+Alt+M** from any app (you can
  change the shortcut in Settings), to open a small chat panel next to it. Answers
  stream in as they're written, and you can stop one at any time.
- **Look at my screen.** Press "Look at my screen" and mouce takes one picture of the screen
  it's on, so you can ask about whatever is in front of you.
- **Circle to look up.** Press **Ctrl+Alt+O** in any app (or use the chat's Circle button)
  and the screen freezes. Draw around anything, and only that part goes to the chat for you
  to ask about. Esc or a right-click cancels.
- **Dictionary.** Hold **Ctrl** and right-click a word in any app. A small card beside it
  shows how to say it, its meanings and examples, from a dictionary built into mouce that
  works offline. With an AI set up, the card also says what the word means in the sentence
  you're reading. Select a phrase ("by and large") and Ctrl + right-click inside it to have
  the AI explain the whole phrase. "Chat about this" takes the word to the chat; a click or
  scroll anywhere else closes the card. A plain right-click is never touched, and you can
  turn the dictionary off in Settings.
- **Calendar and reminders.** Type "dentist tomorrow at 3pm" in the Calendar tab, "remind
  me to submit the lab report on Friday" in Reminders, or just tell mouce in the chat
  ("remind me to call mum every Saturday at 6pm"). The AI fills in a short form for you to
  check, and nothing is saved until you press Save. Events and reminders can repeat every
  day, weekday, week, month or year. When something is due, a small card opens beside
  mouce and mouce glows, with no sound: snooze it for 10 minutes, an hour or until
  tomorrow, mark it done, or click it to open it. Home shows what's coming up this week.
  Without an AI, you fill in the form yourself.
- **Shows what it's doing.** It sways gently when idle, moves faster while thinking, dims and
  pulses while looking, and glows when something needs your attention.
- **Your choice of AI.** Run a local model on your own machine, or bring a key for OpenAI,
  Groq, Gemini or Claude. If the online AI fails, it can fall back to the local one.
- **Guided first run.** Detects whether Ollama is installed and running, and downloads a
  starter model only after you confirm.
- **Tray controls.** Open mouce or the chat, circle to look up, open Settings, pause or show
  mouce, or quit, all from the system tray.
- **Adjustable.** Small, medium or large size, sway on or off, and an optional start with
  Windows.

## The safety promise

mouce can look but never touch. It can never open, change, delete or run anything on your
computer.

| Guarantee | How it's enforced |
| --- | --- |
| No file, process, clipboard or input access | The app contains no file-write, process-launch, clipboard-write or input-synthesis code. An automated scan ([`src-tauri/tests/safety_scan.rs`](src-tauri/tests/safety_scan.rs)) fails the build if such APIs, extra capabilities or extra permissions appear. |
| Watches one mouse gesture, and nothing else | While the dictionary is on, a mouse hook waits for Ctrl + right-click. It only notes which button went down and where; it never moves the pointer, clicks or reads the keyboard, and it holds back only that one Ctrl + right-click so the app's own menu stays shut. The scan allows no other kind of hook, and turning the dictionary off removes it. |
| Looks only when you ask | mouce takes a picture of your screen only when you press "Look at my screen" or start Circle to look up. Ctrl + right-click reads just the word (or the phrase you selected) under the pointer and its sentence, through Windows' accessibility interface (read-only), or from a small picture around the pointer when an app doesn't share its text. Password boxes are never read. Pictures stay in memory and are never saved; only the part you circle (or the screen you chose to share) goes to the AI you picked, the dictionary sends it only the word and its sentence, and saved chat history keeps text only. |
| Writes only to its own folder | A single storage module writes to disk, and only inside the app's data folder. The log goes to the app's own log folder; the scan rejects any other log location. |
| API keys stay secret | Keys are stored in Windows Credential Manager. They are written from Settings and read only by the Rust backend to reach the provider you picked; they never reach the web view. |
| No surprise browsing | The only address the app ever opens in your browser is the Ollama download page. |
| One system change, with your consent | "Start with Windows" adds the app's own startup entry, and only if you turn it on. |

## AI providers

| Provider | Runs on | What you need |
| --- | --- | --- |
| Local (Ollama) | Your computer | [Ollama](https://ollama.com) and one model (first run offers to download it) |
| OpenAI | Online | An OpenAI API key |
| Groq | Online | A Groq API key |
| Gemini | Online | A Google AI Studio API key |
| Claude | Online | An Anthropic API key |

Choosing an online provider means content you ask about is sent to that provider. Nothing is
sent anywhere you didn't pick.

## Getting started

mouce runs on Windows 11. Installers are published on the
[Releases page](https://github.com/Hirukadeven07/mouce/releases), each with a `.sha256`
checksum. Until the first release is out, build it from source.

### Build from source

**Prerequisites:**

- [Node.js](https://nodejs.org)
- [Rust](https://rustup.rs) with the MSVC toolchain
- The WebView2 runtime (already part of Windows 11)

```sh
git clone https://github.com/Hirukadeven07/mouce.git
cd mouce
npm ci
npm run tauri build
```

The installer is written to `src-tauri/target/release/bundle/nsis/`. It isn't code-signed, so
the first time you run it Windows SmartScreen will warn that the publisher is unknown. Choose
**More info**, then **Run anyway**.

### First run

When mouce starts for the first time, it checks whether Ollama is installed and running and
offers to download a starter model. To use an online provider instead, choose **Use an online
AI** and enter your API key. To open Settings later, right-click the mouce icon in the system
tray.

### Logs

If something goes wrong, mouce records it in `%LOCALAPPDATA%\com.mouce.app\logs\mouce.log`
(paste that path into File Explorer's address bar). The log holds start-up lines, warnings,
errors and crash details; it never contains your chats, screen pictures or API keys. It keeps
at most three files of about 1 MB each, replacing the oldest.

### Making a release

1. Set the same new version in `package.json`, `src-tauri/tauri.conf.json` and
   `src-tauri/Cargo.toml`, and merge that change.
2. Tag the merged commit on `main` and push the tag:
   `git tag v0.2.0 && git push origin v0.2.0`
3. The **Release** workflow checks that the tag matches all three versions, builds the
   installer and attaches it, with its checksum, to a **draft** release.
4. Review the draft on the Releases page, edit the notes, and press **Publish**.

## Roadmap

- [x] **Skeleton and safety:** companion window, tray, settings, AI provider layer, secure key
      storage, capability lockdown, safety scan, first run
- [x] **Chat:** a panel next to mouce with streaming answers and "Look at my screen"
- [x] **Circle to look up:** draw around anything on screen and ask about it
- [x] **Dictionary:** Ctrl + right-click a word for its meaning, with an offline fallback
- [x] **Calendar and reminders:** add events in plain language, confirm before saving, and get
      gentle reminders
- [ ] **Packaging:** a signed Windows installer

## Credits

The offline dictionary is built from [Open English WordNet](https://github.com/globalwordnet/english-wordnet)
(2025 edition), © the Open English WordNet team, licensed under
[CC BY 4.0](https://creativecommons.org/licenses/by/4.0/). mouce keeps a trimmed copy; see
[`src-tauri/dictionary`](src-tauri/dictionary) for what was changed and how it is built.
