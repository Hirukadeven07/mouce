# Security

## Reporting a vulnerability

Please report security problems privately, through GitHub's
[private vulnerability reporting](https://github.com/Hirukadeven07/mouce/security/advisories/new),
not in a public issue. Include what you found, how to reproduce it and which version you
used (shown in the installer's file name).

You'll get a reply within a week. Once a fix is released, the advisory is published with
credit to you, unless you'd rather stay anonymous.

## Supported versions

Only the latest release gets security fixes. mouce tells you when there's a new version
(**Settings → Updates**); download its installer from the
[Releases](https://github.com/Hirukadeven07/mouce/releases) page.

## What mouce promises

The safety promises in the [README](README.md#your-privacy-and-safety) are enforced in code and checked on
every build by [`src-tauri/tests/safety_scan.rs`](src-tauri/tests/safety_scan.rs). A way
around any of them counts as a vulnerability.
