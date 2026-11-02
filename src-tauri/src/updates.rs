//! Update notices. mouce asks GitHub which release is the latest and, when it's newer than
//! this one, says so. It never downloads or installs anything: the Download button opens
//! the releases page in the browser (`commands::update_open_download`), and the user runs
//! the new installer as they did the first one.
//!
//! The one address read is the fixed `LATEST_RELEASE_API` below, over https, without
//! following redirects; only the version number and the notes are used from the answer.

use crate::ai::net;
use crate::AppState;
use serde::Serialize;
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

/// GitHub's answer for the latest published release (drafts and pre-releases are skipped).
const LATEST_RELEASE_API: &str = "https://api.github.com/repos/Hirukadeven07/mouce/releases/latest";

/// Event to the windows when a newer version is found: payload is an `UpdateInfo`.
pub const UPDATE_AVAILABLE_EVENT: &str = "update-available";

/// The first automatic check waits until mouce has settled after starting.
const FIRST_CHECK_DELAY: Duration = Duration::from_secs(60);
/// While mouce runs, it looks again this often.
const CHECK_EVERY: Duration = Duration::from_secs(12 * 60 * 60);
/// A check that takes longer than this has failed.
const CHECK_TIMEOUT: Duration = Duration::from_secs(15);
/// At most this much of GitHub's answer is read.
const MAX_ANSWER_BYTES: usize = 256 * 1024;
/// Release notes longer than this are cut short; the full notes are on the release page.
const MAX_NOTES_CHARS: usize = 2000;

/// A newer version, as the windows see it.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct UpdateInfo {
    pub version: String,
    pub current_version: String,
    pub notes: Option<String>,
}

#[derive(Default)]
pub struct UpdateState {
    /// The newer version found by the last check.
    found: Mutex<Option<UpdateInfo>>,
}

/// `v1.2.3` or `1.2.3` as numbers. Anything else (a pre-release, a typo) is not a version.
fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let mut parts = text.trim().trim_start_matches('v').split('.');
    let mut next = || parts.next()?.parse::<u64>().ok();
    let version = (next()?, next()?, next()?);
    parts.next().is_none().then_some(version)
}

/// The notes, trimmed and cut to `MAX_NOTES_CHARS`; none when blank.
fn short_notes(body: &str) -> Option<String> {
    let body = body.trim();
    if body.is_empty() {
        return None;
    }
    if body.chars().count() <= MAX_NOTES_CHARS {
        return Some(body.to_string());
    }
    let cut: String = body.chars().take(MAX_NOTES_CHARS).collect();
    Some(format!("{}…", cut.trim_end()))
}

/// What GitHub's answer says, when its release is newer than `current`.
fn newer_release(answer: &serde_json::Value, current: &str) -> Option<UpdateInfo> {
    let tag = answer.get("tag_name")?.as_str()?;
    let latest = parse_version(tag)?;
    if latest <= parse_version(current)? {
        return None;
    }
    Some(UpdateInfo {
        version: format!("{}.{}.{}", latest.0, latest.1, latest.2),
        current_version: current.to_string(),
        notes: answer.get("body").and_then(|b| b.as_str()).and_then(short_notes),
    })
}

/// Reads the latest release from `url`. `None` when it's not newer than `current`.
async fn fetch_newer(url: &str, current: &str) -> Result<Option<UpdateInfo>, String> {
    let client = reqwest::Client::builder()
        .timeout(CHECK_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .get(url)
        // GitHub refuses requests without a user agent.
        .header(reqwest::header::USER_AGENT, format!("mouce/{current}"))
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .send()
        .await
        .map_err(|e| e.without_url().to_string())?;
    let status = resp.status();
    if !status.is_success() {
        return Err(format!("GitHub answered {status}"));
    }
    let body = net::read_body_capped(resp, MAX_ANSWER_BYTES).await;
    let answer: serde_json::Value =
        serde_json::from_slice(&body).map_err(|_| "GitHub's answer could not be read")?;
    Ok(newer_release(&answer, current))
}

fn found(state: &AppState) -> std::sync::MutexGuard<'_, Option<UpdateInfo>> {
    state.updates.found.lock().unwrap_or_else(|e| e.into_inner())
}

/// The newer version found earlier, if any (for a window that opens after the check).
pub fn current(app: &AppHandle) -> Option<UpdateInfo> {
    found(&app.state::<AppState>()).clone()
}

/// Asks GitHub for the latest release. A newer one is kept and announced to the windows;
/// `None` means this is the latest version.
pub async fn check(app: &AppHandle) -> Result<Option<UpdateInfo>, String> {
    let current = app.package_info().version.to_string();
    let newer = fetch_newer(LATEST_RELEASE_API, &current).await.map_err(|e| {
        log::warn!("could not check for updates: {e}");
        "mouce couldn't check for updates. Check your internet connection and try again."
            .to_string()
    })?;
    *found(&app.state::<AppState>()) = newer.clone();
    if let Some(info) = &newer {
        log::info!("mouce {} is available", info.version);
        let _ = app.emit(UPDATE_AVAILABLE_EVENT, info);
    }
    Ok(newer)
}

/// Checks soon after starting and then every `CHECK_EVERY`, while "Check for updates
/// automatically" is on.
pub fn start(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_CHECK_DELAY).await;
        loop {
            let enabled = {
                let state = app.state::<AppState>();
                let s = state.settings.lock().unwrap_or_else(|e| e.into_inner());
                s.check_for_updates
            };
            if enabled {
                // The error is already in the log; an automatic check stays quiet.
                let _ = check(&app).await;
            }
            tokio::time::sleep(CHECK_EVERY).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{header_regex, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn versions_parse_with_or_without_v() {
        assert_eq!(parse_version("v0.2.0"), Some((0, 2, 0)));
        assert_eq!(parse_version("1.10.3"), Some((1, 10, 3)));
        for bad in ["", "v1.2", "1.2.3.4", "v1.2.3-beta", "latest", "1.x.0"] {
            assert_eq!(parse_version(bad), None, "{bad}");
        }
    }

    #[test]
    fn only_a_newer_release_is_offered() {
        let release = json!({"tag_name": "v0.2.0", "body": "Faster answers."});
        let info = newer_release(&release, "0.1.0").unwrap();
        assert_eq!(info.version, "0.2.0");
        assert_eq!(info.current_version, "0.1.0");
        assert_eq!(info.notes.as_deref(), Some("Faster answers."));
        assert_eq!(newer_release(&release, "0.2.0"), None, "same version");
        assert_eq!(newer_release(&release, "0.10.0"), None, "older, compared as numbers");
        assert_eq!(newer_release(&json!({"tag_name": "v0.2.0-rc1"}), "0.1.0"), None);
        assert_eq!(newer_release(&json!({"name": "no tag"}), "0.1.0"), None);
    }

    #[test]
    fn notes_are_trimmed_and_cut_short() {
        assert_eq!(short_notes(" \n "), None);
        assert_eq!(short_notes("\n Fixes \n").as_deref(), Some("Fixes"));
        let cut = short_notes(&"a".repeat(MAX_NOTES_CHARS + 50)).unwrap();
        assert_eq!(cut.chars().count(), MAX_NOTES_CHARS + 1);
        assert!(cut.ends_with('…'));
    }

    #[test]
    fn the_check_reads_only_github_over_https() {
        assert!(LATEST_RELEASE_API.starts_with("https://api.github.com/repos/Hirukadeven07/mouce/"));
    }

    #[tokio::test]
    async fn fetch_reads_the_latest_release() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/latest"))
            .and(header_regex("user-agent", r"^mouce/\d+\.\d+\.\d+$"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"tag_name": "v0.2.0", "body": "New things"})),
            )
            .mount(&server)
            .await;
        let url = format!("{}/latest", server.uri());
        let info = fetch_newer(&url, "0.1.0").await.unwrap().unwrap();
        assert_eq!(info.version, "0.2.0");
        assert_eq!(fetch_newer(&url, "0.2.0").await.unwrap(), None);
    }

    #[tokio::test]
    async fn fetch_reports_errors_and_never_follows_redirects() {
        let server = MockServer::start().await;
        Mock::given(path("/missing")).respond_with(ResponseTemplate::new(404)).mount(&server).await;
        Mock::given(path("/moved"))
            .respond_with(
                ResponseTemplate::new(302).insert_header("location", "https://evil.example/x"),
            )
            .mount(&server)
            .await;
        Mock::given(path("/junk"))
            .respond_with(ResponseTemplate::new(200).set_body_string("not json"))
            .mount(&server)
            .await;
        for p in ["/missing", "/moved", "/junk"] {
            let url = format!("{}{p}", server.uri());
            assert!(fetch_newer(&url, "0.1.0").await.is_err(), "{p}");
        }
    }
}
