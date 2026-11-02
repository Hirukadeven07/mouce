# Privacy

mouce has no account, no analytics and no servers of its own. It talks only to the AI you
choose in Settings, and only when you ask it something.

## Where your data goes

**With the local AI (Ollama), nothing leaves your computer.** Ollama runs on your PC, and
mouce reaches it directly, never through a proxy.

**With an online AI (OpenAI, Groq, Gemini or Claude),** mouce sends that provider only what
an answer needs:

| When you… | mouce sends |
| --- | --- |
| Ask a question in the chat | Your question and the recent messages of the chat |
| Press "Look at my screen" | A picture of your screen, with your question |
| Circle part of the screen | A picture of only the circled part, with your question |
| Ctrl + right-click a word | The word (or phrase you selected) and the sentence around it |
| Ask about your plans (if "Answer questions about my plans in the chat" is on, as it is by default) | Your plans for the next 14 days, with your question |
| Add a plan by typing a note | The note, so the AI can read the date and time from it |

Each provider handles what it receives under its own privacy policy and API terms.
mouce talks only to these addresses: `api.openai.com`, `api.groq.com`,
`generativelanguage.googleapis.com` and `api.anthropic.com`. It can't reach any other site,
and an answer can't redirect it to one.

If the online AI fails and "If the online AI fails, use local" is on (it is by default),
the same question goes to Ollama on your computer instead.

## What stays on your computer

- **Screen pictures** stay in memory only. They're never saved.
- **Chat history** is saved only if you turn on "Keep chat history" (it's off by default), as text
  only, in `%APPDATA%\com.mouce.app\mouce.db`. Turning it off deletes the saved copy.
- **Your calendar, reminders and settings** are in the same file.
- **API keys** are in Windows Credential Manager, and only mouce's backend reads them.
- **The log**, in `%LOCALAPPDATA%\com.mouce.app\logs`, holds start-up lines, warnings and
  errors. It never contains your chats, screen pictures or API keys.

## Removing your data

Uninstall mouce from **Settings → Apps → Installed apps** and tick **Delete the application
data**. That removes the data and log folders above and the saved API keys. Uninstalling
always removes the "Start with Windows" entry.
