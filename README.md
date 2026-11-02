<p align="center">
  <img src="images/mouce-banner.jpg" alt="mouce" width="100%">
</p>

<h1 align="center">mouce</h1>

<p align="center">
  <strong>A tiny desktop companion that helps you understand what's on your screen, without ever touching your files.</strong>
</p>

<p align="center">
  <a href="https://github.com/Hirukadeven07/mouce/releases/download/v0.1.1/mouce_0.1.1_x64-setup.exe"><img alt="Download mouce 0.1.1 for Windows" src="https://img.shields.io/badge/Download-mouce%200.1.1%20for%20Windows-0078D4?style=for-the-badge&logo=windows11&logoColor=white"></a>
</p>

<p align="center">
  Windows 11 · 8.4 MB · free and open source (MIT) ·
  <a href="https://github.com/Hirukadeven07/mouce/releases/latest">all releases and checksums</a>
</p>

---

## What mouce does

mouce is a small, always-on-top companion that sits in a corner of your desktop. It never
takes keyboard focus from the app you're working in. When you want help, it asks the AI you
choose: a free local AI on your own computer, or an online one with your own API key.

- **Chat beside your work.** Click mouce, or press **Ctrl+Alt+M** in any app, to open a small
  chat panel next to it. Answers appear as they're written, and you can stop one at any time.
- **Look at my screen.** Press "Look at my screen" and mouce takes one picture of the screen
  it's on, so you can ask about whatever is in front of you.
- **Circle to look up.** Press **Ctrl+Alt+O** in any app and the screen freezes. Draw around
  anything, and only that part goes to the chat for you to ask about.
- **Dictionary.** Hold **Ctrl** and right-click a word in any app to see how to say it, its
  meanings and examples, even offline. With an AI set up, it also explains what the word means
  in the sentence you're reading. Select a phrase first to have the whole phrase explained.
- **Calendar and reminders.** Type "dentist tomorrow at 3pm" or tell the chat "remind me to call
  mum every Saturday at 6pm". mouce fills in a short form for you to check, and nothing is saved
  until you press Save. Events can repeat, and you can change a single day of a repeating one.
  When something is due, a small card appears beside mouce (no sound): snooze it, mark it done,
  or open it. Reminders wait during your quiet hours and while you're in a full-screen app. Ask
  the chat "am I free on Friday?" and it answers from your calendar.
- **Shows what it's doing.** It sways gently when idle, moves faster while thinking, pulses
  while looking, and glows when something needs your attention.
- **Yours to adjust.** Three sizes, your own shortcuts, an optional start with Windows, and
  everything else from the home window or the tray icon.

## Getting started

### 1. Install

1. [Download the installer](https://github.com/Hirukadeven07/mouce/releases/download/v0.1.1/mouce_0.1.1_x64-setup.exe).
2. Optional but recommended: check the download is genuine. In PowerShell, in your Downloads
   folder, run:

   ```powershell
   (Get-FileHash .\mouce_0.1.1_x64-setup.exe).Hash
   ```

   It must print the same code as the `.sha256` file on the
   [release page](https://github.com/Hirukadeven07/mouce/releases/latest) (letter case doesn't
   matter). If it doesn't, delete the download and don't run it.
3. Run the installer. It doesn't need administrator rights.

The installer isn't code-signed yet, so Windows SmartScreen warns that the publisher is
unknown. Choose **More info**, then **Run anyway**.

### 2. Choose an AI

The first time mouce starts, it helps you pick one:

| AI | Runs on | What you need |
| --- | --- | --- |
| Local (Ollama) | Your computer, nothing leaves it | [Ollama](https://ollama.com), free. mouce offers to download a starter model. |
| OpenAI | Online | An OpenAI API key |
| Groq | Online | A Groq API key |
| Gemini | Online | A Google AI Studio API key |
| Claude | Online | An Anthropic API key |

You can change it any time in **Settings**. If an online AI fails, mouce can answer with the
local one instead.

### 3. Use it

| To… | Do this |
| --- | --- |
| Open the chat | Click mouce, or press **Ctrl+Alt+M** |
| Ask about part of the screen | Press **Ctrl+Alt+O** and draw around it (**Esc** cancels) |
| Look up a word | Hold **Ctrl** and right-click it |
| Open the home window, calendar and settings | Click mouce while the chat is open, or use the tray icon |
| Hide mouce or quit | Right-click the mouce icon in the system tray |

You can change both shortcuts in **Settings**.

## Your privacy and safety

mouce can look but never touch. It can't open, change, delete or run anything on your
computer, and it can't type, click or use your clipboard.

- **It looks only when you ask.** A picture of your screen is taken only when you press "Look
  at my screen" or circle something. Pictures are never saved, and password boxes are never read.
- **It sends only what you ask about, only to the AI you chose.** With the local AI, nothing
  leaves your computer. [PRIVACY.md](PRIVACY.md) lists exactly what goes where.
- **Your API keys stay locked away** in Windows Credential Manager.
- **It keeps to itself.** It saves only in its own folder, keeps chat history only if you turn
  it on, and changes nothing else on your PC except an optional "Start with Windows" entry.
- **It only opens two web pages,** and only when you press their buttons: the Ollama download
  page and mouce's own release page.
- **Update checks ask GitHub one question:** which version is the newest? Nothing about you or
  your computer is sent, and you can turn them off.

## Updating

mouce checks for a new version now and then. When there is one, Home tells you, and
**Settings → Updates** shows what's new with a **Download** button that opens the release page.
Download the new installer and run it; your settings, keys and calendar are kept. mouce never
downloads or installs anything by itself. You can turn the automatic check off in
**Settings → Updates** and press **Check for updates** whenever you like.

**On mouce 0.1.0?** It doesn't check for updates yet, so get the newest installer from the
[releases page](https://github.com/Hirukadeven07/mouce/releases/latest).

## Uninstalling

Uninstall mouce from **Settings → Apps → Installed apps**. This always removes its "Start
with Windows" entry. Tick **Delete the application data** to also remove your chat history,
calendar, settings and saved API keys.

## Help

- **Something isn't working?** [Open an issue](https://github.com/Hirukadeven07/mouce/issues)
  and describe what happened. It helps to attach the log, `%LOCALAPPDATA%\com.mouce.app\logs\mouce.log`
  (paste that path into File Explorer's address bar). The log never contains your chats,
  screen pictures or API keys.
- **Found a security problem?** Please report it privately; see [SECURITY.md](SECURITY.md).

## License and credits

mouce is free and open source under the [MIT License](LICENSE). What changed in each version
is in the [changelog](CHANGELOG.md).

The offline dictionary is built from [Open English WordNet](https://github.com/globalwordnet/english-wordnet)
(2025 edition), © the Open English WordNet team, licensed under
[CC BY 4.0](https://creativecommons.org/licenses/by/4.0/). mouce uses a trimmed copy; see
[`src-tauri/dictionary`](src-tauri/dictionary) for what was changed.
