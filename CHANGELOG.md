# Changelog

What changed in each release of mouce. Versions follow [Semantic Versioning](https://semver.org).

## 0.1.0

The first release: a small companion that sits on your desktop and answers questions about
what's on your screen.

### Features

- **Chat beside your work.** Click mouce or press Ctrl+Alt+M in any app to open a chat panel
  next to it, with answers that stream in and can be stopped. Continue full size in the
  home window.
- **Look at my screen.** One picture of the screen mouce is on, sent with your question.
- **Circle to look up.** Press Ctrl+Alt+O, draw around anything, and ask about just that part.
- **Dictionary.** Ctrl + right-click a word in any app for how to say it, its meanings and
  examples, from an offline dictionary. With an AI set up, it also explains the word in its
  sentence, or a whole selected phrase.
- **Calendar and reminders.** Add events and reminders in plain language, from the Calendar
  tab or the chat. Nothing is saved until you check the form and press Save. Repeats, end
  times, clash warnings, single-day changes, snoozing, quiet hours, and questions like "am
  I free on Friday?"
- **Your choice of AI.** A local model through Ollama, or your own key for OpenAI, Groq,
  Gemini or Claude, with an optional fallback to the local AI.
- **Home window.** Chat, calendar, reminders, shortcuts, appearance and settings in one place.
- **Guided first run, tray controls,** three sizes and optional start with Windows.

### Safety and privacy

- mouce can look but never touch: no file, process, clipboard or input access, enforced by
  an automated scan on every build.
- Each window may call only its own commands, so the word card, circle layer and reminder
  card can't reach API keys, settings or the screen picture.
- API keys are kept in Windows Credential Manager and never reach the windows.
- mouce reaches only the four AI provider addresses and Ollama on your computer, which is
  never reached through a proxy.
- Screen pictures are never saved. Chat history is saved only if you turn it on. See
  [PRIVACY.md](PRIVACY.md).

### Installing

- A Windows 11 installer with a SHA-256 checksum. It isn't code-signed yet, so SmartScreen
  warns that the publisher is unknown; see [Installing](README.md#installing).
- Uninstalling removes the "Start with Windows" entry. Ticking "Delete the application data"
  also removes your data and saved API keys.
