//! Ollama setup helpers: detect whether the local server is available and
//! download a model through it. The app never starts or installs Ollama; the
//! UI tells the user what to do instead.

use crate::ai::{net, AiError};
use serde::Serialize;
use serde_json::{json, Value};
use std::path::Path;
use std::time::Duration;
use url::Url;

pub const DEFAULT_LOCAL_MODEL: &str = "gemma3:4b";

/// How long the status check waits for the server's answer.
const STATUS_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Serialize, Debug, PartialEq, Eq)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum OllamaStatus {
    NotInstalled,
    NotRunning,
    Running { models: Vec<String> },
}

#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct PullProgress {
    pub status: String,
    pub completed: Option<u64>,
    pub total: Option<u64>,
}

fn endpoint(base_url: &str, path: &str) -> Result<Url, AiError> {
    Url::parse(&format!("{}{}", base_url.trim_end_matches('/'), path))
        .map_err(|_| AiError::Network("Invalid Ollama address".to_string()))
}

/// Asks the server for its model list (`GET /api/tags`).
/// - Answers with a model list: `Running`.
/// - Cannot connect: `NotRunning` if the Ollama program exists at `exe_hint`,
///   otherwise `NotInstalled`.
/// - Answers but with a non-2xx status or an unreadable body: `NotRunning`
///   (something is listening, but it is not a usable Ollama server).
/// - Address rejected by the allowlist or not a URL: `NotRunning`.
///
/// Only checks `exe_hint` for existence; nothing is ever launched.
pub async fn status(base_url: &str, exe_hint: &Path) -> OllamaStatus {
    status_with_timeout(base_url, exe_hint, STATUS_TIMEOUT).await
}

async fn status_with_timeout(base_url: &str, exe_hint: &Path, limit: Duration) -> OllamaStatus {
    // A listener that accepts the connection but never answers must not hang the UI.
    match tokio::time::timeout(limit, fetch_models(base_url)).await {
        Err(_elapsed) => OllamaStatus::NotRunning,
        Ok(Ok(models)) => OllamaStatus::Running { models },
        Ok(Err(AiError::Network(_))) if !exe_hint.exists() => OllamaStatus::NotInstalled,
        Ok(Err(_)) => OllamaStatus::NotRunning,
    }
}

async fn fetch_models(base_url: &str) -> Result<Vec<String>, AiError> {
    let url = endpoint(base_url, "/api/tags")?;
    let resp = net::send(net::client_for(url.as_str()).get(url.clone()), &url).await?;
    let text = resp.text().await.map_err(|e| AiError::Parse(e.without_url().to_string()))?;
    let v: Value = serde_json::from_str(&text)
        .map_err(|_| AiError::Parse("unreadable model list".to_string()))?;
    let models = v
        .get("models")
        .and_then(Value::as_array)
        .ok_or_else(|| AiError::Parse("model list missing".to_string()))?;
    Ok(models
        .iter()
        .filter_map(|m| m.get("name").and_then(Value::as_str))
        .map(str::to_string)
        .collect())
}

/// Downloads `model` (`POST /api/pull`, NDJSON), reporting each progress line.
/// Ends `Ok` at `{"status":"success"}`; an `{"error": ...}` line becomes
/// `Http { status: 500, .. }`; a stream that ends before success is an error.
pub async fn pull(
    base_url: &str,
    model: &str,
    on_progress: &mut (dyn FnMut(PullProgress) + Send),
) -> Result<(), AiError> {
    let url = endpoint(base_url, "/api/pull")?;
    let req = net::client_for(url.as_str())
        .post(url.clone())
        .json(&json!({ "model": model, "stream": true }));
    let resp = net::send(req, &url).await?;
    let mut finished = false;
    net::read_stream(resp, |line| {
        let v: Value = serde_json::from_str(line)
            .map_err(|_| AiError::Parse("unreadable line in Ollama download".to_string()))?;
        if let Some(message) = v.get("error").and_then(Value::as_str) {
            return Err(AiError::Http { status: 500, message: message.to_string() });
        }
        let status = v.get("status").and_then(Value::as_str).unwrap_or_default().to_string();
        let done = status == "success";
        on_progress(PullProgress {
            status,
            completed: v.get("completed").and_then(Value::as_u64),
            total: v.get("total").and_then(Value::as_u64),
        });
        if done {
            finished = true;
            Ok(net::LineOutcome::Stop)
        } else {
            Ok(net::LineOutcome::Continue)
        }
    })
    .await?;
    if finished {
        Ok(())
    } else {
        Err(AiError::Network("download ended before it finished".to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{body_partial_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    const DEAD_BASE: &str = "http://127.0.0.1:1";

    #[test]
    fn status_json_shape() {
        assert_eq!(
            serde_json::to_value(OllamaStatus::Running { models: vec!["gemma3:4b".into()] })
                .unwrap(),
            json!({ "state": "running", "models": ["gemma3:4b"] })
        );
        assert_eq!(
            serde_json::to_value(OllamaStatus::NotInstalled).unwrap(),
            json!({ "state": "notInstalled" })
        );
        assert_eq!(
            serde_json::to_value(OllamaStatus::NotRunning).unwrap(),
            json!({ "state": "notRunning" })
        );
    }

    #[test]
    fn progress_json_shape() {
        let p = PullProgress { status: "pulling".into(), completed: Some(1), total: None };
        assert_eq!(
            serde_json::to_value(p).unwrap(),
            json!({ "status": "pulling", "completed": 1, "total": null })
        );
    }

    #[tokio::test]
    async fn status_running_lists_models() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/tags"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(r#"{"models":[{"name":"gemma3:4b"},{"name":"llama3:8b"}]}"#),
            )
            .expect(1)
            .mount(&server)
            .await;
        // A trailing slash on the base must not break the request path.
        let hint = Path::new("does-not-matter.exe");
        let s = status(&format!("{}/", server.uri()), hint).await;
        assert_eq!(
            s,
            OllamaStatus::Running { models: vec!["gemma3:4b".into(), "llama3:8b".into()] }
        );
    }

    #[tokio::test]
    async fn status_not_running_when_connection_refused() {
        let hint = tempfile::NamedTempFile::new().unwrap();
        assert_eq!(status(DEAD_BASE, hint.path()).await, OllamaStatus::NotRunning);
    }

    #[tokio::test]
    async fn status_not_installed_when_no_exe() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("ollama.exe");
        assert_eq!(status(DEAD_BASE, &missing).await, OllamaStatus::NotInstalled);
    }

    #[tokio::test]
    async fn status_not_running_when_server_answers_badly() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/tags"))
            .respond_with(ResponseTemplate::new(500).set_body_string("oops"))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/tags"))
            .respond_with(ResponseTemplate::new(200).set_body_string("<html>not json</html>"))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/tags"))
            .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"other":1}"#))
            .mount(&server)
            .await;
        // The server is reachable, so even a missing exe hint reads as "not running".
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("ollama.exe");
        for _ in 0..3 {
            assert_eq!(status(&server.uri(), &missing).await, OllamaStatus::NotRunning);
        }
    }

    #[tokio::test]
    async fn status_not_running_when_server_never_answers() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/tags"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(r#"{"models":[]}"#)
                    .set_delay(std::time::Duration::from_secs(2)),
            )
            .mount(&server)
            .await;
        // Not-installed would be wrong too: something is listening.
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("ollama.exe");
        let started = std::time::Instant::now();
        let s = status_with_timeout(&server.uri(), &missing, std::time::Duration::from_millis(150))
            .await;
        assert_eq!(s, OllamaStatus::NotRunning);
        assert!(started.elapsed() < std::time::Duration::from_millis(1500));
    }

    #[test]
    fn status_timeout_is_five_seconds() {
        assert_eq!(STATUS_TIMEOUT, std::time::Duration::from_secs(5));
    }

    #[tokio::test]
    async fn status_does_not_contact_disallowed_host() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("ollama.exe");
        assert_eq!(status("https://evil.com", &missing).await, OllamaStatus::NotRunning);
    }

    async fn pull_server(body: &str) -> MockServer {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/pull"))
            .and(body_partial_json(json!({ "model": "gemma3:4b", "stream": true })))
            .respond_with(ResponseTemplate::new(200).set_body_string(body))
            .expect(1)
            .mount(&server)
            .await;
        server
    }

    #[tokio::test]
    async fn pull_reports_progress_then_success() {
        let server = pull_server(
            "{\"status\":\"pulling manifest\"}\n\
             {\"status\":\"downloading\",\"completed\":10,\"total\":100}\n\
             {\"status\":\"downloading\",\"completed\":100,\"total\":100}\n\
             {\"status\":\"success\"}\n\
             {\"status\":\"ignored after success\"}\n",
        )
        .await;
        let mut seen: Vec<PullProgress> = Vec::new();
        pull(&server.uri(), DEFAULT_LOCAL_MODEL, &mut |p| seen.push(p)).await.unwrap();
        assert_eq!(seen.len(), 4, "{seen:?}");
        assert_eq!(
            seen[0],
            PullProgress { status: "pulling manifest".into(), completed: None, total: None }
        );
        assert_eq!(seen[1].completed, Some(10));
        assert_eq!(seen[1].total, Some(100));
        assert_eq!(seen[2].completed, Some(100));
        assert_eq!(seen[3].status, "success");
    }

    #[tokio::test]
    async fn pull_error_line_is_error() {
        let server = pull_server(
            "{\"status\":\"pulling manifest\"}\n{\"error\":\"pull model manifest: file does not exist\"}\n",
        )
        .await;
        let mut seen = Vec::new();
        let err =
            pull(&server.uri(), DEFAULT_LOCAL_MODEL, &mut |p| seen.push(p)).await.unwrap_err();
        assert_eq!(
            err,
            AiError::Http {
                status: 500,
                message: "pull model manifest: file does not exist".into()
            }
        );
        assert_eq!(seen.len(), 1);
    }

    #[tokio::test]
    async fn pull_stream_ending_without_success_is_error() {
        let server =
            pull_server("{\"status\":\"downloading\",\"completed\":1,\"total\":9}\n").await;
        let err = pull(&server.uri(), DEFAULT_LOCAL_MODEL, &mut |_| {}).await.unwrap_err();
        assert!(matches!(err, AiError::Network(_)), "{err:?}");
    }

    #[tokio::test]
    async fn pull_unreadable_line_is_parse_error() {
        let server = pull_server("not json\n").await;
        let err = pull(&server.uri(), DEFAULT_LOCAL_MODEL, &mut |_| {}).await.unwrap_err();
        assert!(matches!(err, AiError::Parse(_)), "{err:?}");
    }

    #[tokio::test]
    async fn pull_http_error_status_surfaces() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(404).set_body_string(r#"{"error":"nope"}"#))
            .mount(&server)
            .await;
        let err = pull(&server.uri(), DEFAULT_LOCAL_MODEL, &mut |_| {}).await.unwrap_err();
        assert_eq!(err, AiError::Http { status: 404, message: "nope".into() });
    }
}
