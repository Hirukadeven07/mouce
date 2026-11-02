# Contributing

How to build mouce and make a release. For what mouce does, see the [README](README.md).

## Build from source

**Prerequisites:**

- [Node.js](https://nodejs.org)
- [Rust](https://rustup.rs) with the MSVC toolchain
- The WebView2 runtime (already part of Windows 11)

```sh
git clone https://github.com/Hirukadeven07/mouce.git
cd mouce
npm ci
npm run tauri dev     # run it while developing
npm run tauri build   # build the installer
```

The installer is written to `src-tauri/target/release/bundle/nsis/`.

## Checks

CI runs these on every pull request:

```sh
npx vitest run
cd src-tauri
cargo fmt --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

`cargo test` includes the safety scan, [`src-tauri/tests/safety_scan.rs`](src-tauri/tests/safety_scan.rs).
It fails the build if the app gains file, process, clipboard or input access, a hook other
than the dictionary's mouse hook, a window permission it doesn't need, or a plugin or Windows
API area outside the reviewed list. A change that needs one of these must update the scan on
purpose, with the reason in the pull request.

## Making a release

1. Set the same new version in `package.json`, `src-tauri/tauri.conf.json` and
   `src-tauri/Cargo.toml`, and rename the CHANGELOG's **Unreleased** section to the version
   (`## 0.2.0`).
2. Update the README's download links (the button and step 1 of Install) and the command in its
   checksum step to the new version's installer, for example
   `releases/download/v0.2.0/mouce_0.2.0_x64-setup.exe`. Merge these changes.
3. Tag the merged commit on `main` and push the tag:
   `git tag v0.2.0 && git push origin v0.2.0`
4. The **Release** workflow checks that the tag matches all three versions, builds the
   installer and attaches it, with its checksum, to a **draft** release.
5. Review the draft on the Releases page, paste the version's CHANGELOG section into the
   notes, and press **Publish**. The release notes are what people see in **Settings →
   Updates**, so keep them about what changed for them.

The README links point at the new installer as soon as step 2 is merged, but the file only
exists once step 5 is done, so do steps 3 to 5 straight after merging.
