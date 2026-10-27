# Star

Star is a tiny, always-on-top rotating star that lives on your Windows desktop and helps you
understand what is on your screen. It never takes keyboard focus from the app you are using,
and it asks the AI you choose (a free local model through Ollama, or an online provider with
your own API key) about what you point it at.

## The safety promise

Star can look but never touch. It can never open, change, delete or run anything on your
computer. Concretely:

- The app has no file-write, process-launch, clipboard-write or input-synthesis code. An
  automated scan (`src-tauri/tests/safety_scan.rs`) fails the build if such APIs, extra
  capabilities or extra permissions show up.
- API keys are stored in Windows Credential Manager. They are written from Settings and read
  only inside the Rust side to talk to the provider you picked; they never reach the web view.
- The only address the app ever opens in your browser is the Ollama download page.

## Development setup (Windows)

You need Node.js, the Rust toolchain (MSVC), and the WebView2 runtime (already part of
Windows 11).

```
npm ci
npm run build        # the Rust tests need dist/ to exist, so build the frontend first
npm run tauri dev    # run the app with hot reload
```

To produce a debug executable without an installer: `npm run tauri build -- --debug --no-bundle`.

## Tests

```
npx vitest --run             # frontend
cd src-tauri && cargo test   # Rust, including the safety scan
```

## Installing a build

Builds are not code-signed yet, so Windows SmartScreen will warn that the publisher is
unknown the first time you run the installer. Choose "More info", then "Run anyway", if you
trust where the file came from.
