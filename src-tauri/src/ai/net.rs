//! Allowlisted HTTP client and stream-line parsing for AI providers.

use super::AiError;
use std::time::Duration;
use url::{Host, Url};

const REMOTE_HOSTS: &[&str] = &[
    "api.openai.com",
    "api.groq.com",
    "generativelanguage.googleapis.com",
    "api.anthropic.com",
];

const MAX_REDIRECTS: usize = 10;
const MAX_ERROR_MESSAGE_CHARS: usize = 300;

/// Only approved provider hosts (https) and loopback (http or https) may be reached.
pub fn check_allowed(url: &Url) -> Result<(), AiError> {
    let scheme = url.scheme();
    let not_allowed = || AiError::NotAllowed(url.host_str().unwrap_or(url.scheme()).to_string());
    if scheme != "http" && scheme != "https" {
        return Err(not_allowed());
    }
    let loopback = match url.host() {
        Some(Host::Domain(d)) => d == "localhost",
        Some(Host::Ipv4(ip)) => ip.octets() == [127, 0, 0, 1],
        _ => false,
    };
    if loopback {
        return Ok(());
    }
    match url.host() {
        Some(Host::Domain(d)) if scheme == "https" && REMOTE_HOSTS.contains(&d) => Ok(()),
        _ => Err(not_allowed()),
    }
}

/// Why the redirect policy stopped a redirect chain.
#[derive(Debug)]
enum RedirectStopped {
    NotAllowed(String),
    TooMany,
}

impl std::fmt::Display for RedirectStopped {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RedirectStopped::NotAllowed(host) => write!(f, "redirect to {host} blocked"),
            RedirectStopped::TooMany => write!(f, "too many redirects"),
        }
    }
}

impl std::error::Error for RedirectStopped {}

fn same_origin(a: &Url, b: &Url) -> bool {
    a.scheme() == b.scheme()
        && a.host_str() == b.host_str()
        && a.port_or_known_default() == b.port_or_known_default()
}

/// HTTP client: 10 s connect timeout, no overall timeout (responses stream).
/// Redirects are followed only within the original scheme, host and port, so
/// credential headers never travel to another host; other hops are refused.
pub fn client() -> reqwest::Client {
    let policy = reqwest::redirect::Policy::custom(|attempt| {
        if attempt.previous().len() >= MAX_REDIRECTS {
            return attempt.error(RedirectStopped::TooMany);
        }
        let allowed = check_allowed(attempt.url()).is_ok()
            && attempt.previous().first().is_some_and(|origin| same_origin(origin, attempt.url()));
        if allowed {
            attempt.follow()
        } else {
            let host = attempt.url().host_str().unwrap_or("unknown host").to_string();
            attempt.error(RedirectStopped::NotAllowed(host))
        }
    });
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .redirect(policy)
        .build()
        .expect("build HTTP client")
}

fn map_send_error(e: reqwest::Error) -> AiError {
    let mut source = std::error::Error::source(&e);
    while let Some(err) = source {
        if let Some(stopped) = err.downcast_ref::<RedirectStopped>() {
            return match stopped {
                RedirectStopped::NotAllowed(host) => AiError::NotAllowed(host.clone()),
                RedirectStopped::TooMany => AiError::Network(stopped.to_string()),
            };
        }
        source = err.source();
    }
    // `without_url` keeps query-string credentials out of error text.
    AiError::Network(e.without_url().to_string())
}

/// Sends `req` (which must target `url`) after checking the allowlist on both
/// `url` and the request's real URL; non-2xx becomes `AiError::Http`.
pub async fn send(req: reqwest::RequestBuilder, url: &Url) -> Result<reqwest::Response, AiError> {
    check_allowed(url)?;
    let (client, request) = req.build_split();
    let request = request.map_err(|e| AiError::Network(e.without_url().to_string()))?;
    check_allowed(request.url())?;
    if request.url().origin() != url.origin() {
        return Err(AiError::NotAllowed(format!(
            "request targets {} but {} was approved",
            request.url().host_str().unwrap_or("unknown host"),
            url.host_str().unwrap_or("unknown host")
        )));
    }
    let resp = client.execute(request).await.map_err(map_send_error)?;
    if resp.status().is_success() {
        Ok(resp)
    } else {
        Err(error_from_response(resp).await)
    }
}

/// Builds `AiError::Http`, preferring the provider's own message from the body.
pub async fn error_from_response(resp: reqwest::Response) -> AiError {
    let status = resp.status().as_u16();
    let body = resp.text().await.unwrap_or_default();
    AiError::Http { status, message: error_message(&body) }
}

fn error_message(body: &str) -> String {
    let from_json = serde_json::from_str::<serde_json::Value>(body).ok().and_then(|v| {
        let pick = |x: Option<&serde_json::Value>| x.and_then(|s| s.as_str()).map(str::to_string);
        pick(v.pointer("/error/message"))
            .or_else(|| pick(v.get("error")))
            .or_else(|| pick(v.get("message")))
    });
    from_json.unwrap_or_else(|| body.trim().chars().take(MAX_ERROR_MESSAGE_CHARS).collect())
}

/// Accumulates raw stream bytes and yields complete, non-empty lines.
#[derive(Default)]
pub struct LineBuffer {
    buf: Vec<u8>,
}

impl LineBuffer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a chunk; returns lines completed by it (`\n` or `\r\n` terminated).
    pub fn push(&mut self, chunk: &[u8]) -> Vec<String> {
        self.buf.extend_from_slice(chunk);
        let mut lines = Vec::new();
        while let Some(pos) = self.buf.iter().position(|&b| b == b'\n') {
            let rest = self.buf.split_off(pos + 1);
            let mut line = std::mem::replace(&mut self.buf, rest);
            line.pop();
            if line.last() == Some(&b'\r') {
                line.pop();
            }
            if !line.is_empty() {
                lines.push(String::from_utf8_lossy(&line).into_owned());
            }
        }
        lines
    }

    /// Returns any trailing unterminated line.
    pub fn finish(mut self) -> Option<String> {
        if self.buf.last() == Some(&b'\r') {
            self.buf.pop();
        }
        if self.buf.is_empty() {
            None
        } else {
            Some(String::from_utf8_lossy(&self.buf).into_owned())
        }
    }
}

/// Payload of an SSE `data:` line (one optional space after the colon).
pub fn sse_data(line: &str) -> Option<&str> {
    let rest = line.strip_prefix("data:")?;
    Some(rest.strip_prefix(' ').unwrap_or(rest))
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn u(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn allowlist() {
        for ok in [
            "https://api.openai.com/v1/x",
            "https://api.groq.com/openai/v1",
            "https://generativelanguage.googleapis.com/v1beta",
            "https://api.anthropic.com/v1/messages",
            "http://localhost:11434",
            "http://127.0.0.1:5555",
        ] {
            assert!(check_allowed(&u(ok)).is_ok(), "{ok} should be allowed");
        }
        for bad in [
            "http://api.openai.com",
            "https://evil.com",
            "https://api.openai.com.evil.com",
            "https://evil.com/api.openai.com",
            "https://localhost.evil.com",
            "file:///C:/x",
        ] {
            assert!(
                matches!(check_allowed(&u(bad)), Err(AiError::NotAllowed(_))),
                "{bad} should be rejected"
            );
        }
    }

    #[test]
    fn line_buffer_handles_split_chunks() {
        let mut b = LineBuffer::new();
        assert!(b.push(b"data: {\"a\"").is_empty());
        assert_eq!(b.push(b":1}\n\ndata: [DONE]\r\n"), vec!["data: {\"a\":1}", "data: [DONE]"]);
        assert!(b.push(b"tail").is_empty());
        assert_eq!(b.finish(), Some("tail".to_string()));
    }

    #[test]
    fn line_buffer_splits_multibyte_char_across_chunks() {
        let bytes = "é\n".as_bytes();
        let mut b = LineBuffer::new();
        assert!(b.push(&bytes[..1]).is_empty());
        assert_eq!(b.push(&bytes[1..]), vec!["é"]);
        assert_eq!(b.finish(), None);
    }

    #[test]
    fn sse_data_parses() {
        assert_eq!(sse_data("data: x"), Some("x"));
        assert_eq!(sse_data("data:x"), Some("x"));
        assert_eq!(sse_data("event: ping"), None);
    }

    #[tokio::test]
    async fn http_error_surfaces_provider_message() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(
                ResponseTemplate::new(401)
                    .set_body_string(r#"{"error":{"message":"invalid x-api-key"}}"#),
            )
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(429).set_body_string("slow down"))
            .mount(&server)
            .await;

        let url = u(&server.uri());
        let err = send(client().get(url.clone()), &url).await.unwrap_err();
        assert_eq!(err, AiError::Http { status: 401, message: "invalid x-api-key".into() });
        assert_eq!(err.to_string(), "HTTP 401: invalid x-api-key");

        let err = send(client().get(url.clone()), &url).await.unwrap_err();
        assert_eq!(err, AiError::Http { status: 429, message: "slow down".into() });
    }

    #[tokio::test]
    async fn send_rejects_disallowed_url_before_sending() {
        let url = u("https://evil.com/x");
        let err = send(client().get(url.clone()), &url).await.unwrap_err();
        assert!(matches!(err, AiError::NotAllowed(_)));
    }

    fn redirect_to(location: String) -> Mock {
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(302).insert_header("location", location.as_str()))
    }

    async fn request_count(server: &MockServer) -> usize {
        server.received_requests().await.unwrap().len()
    }

    #[tokio::test]
    async fn redirect_to_non_allowlisted_host_is_rejected() {
        // 127.0.0.2 is loopback but not an allowlisted host name.
        let listener = std::net::TcpListener::bind("127.0.0.2:0").expect("bind 127.0.0.2");
        let target = MockServer::builder().listener(listener).start().await;
        let source = MockServer::start().await;
        redirect_to(format!("{}/x", target.uri())).mount(&source).await;
        Mock::given(method("GET")).respond_with(ResponseTemplate::new(200)).mount(&target).await;

        let url = u(&source.uri());
        let err = send(client().get(url.clone()), &url).await.unwrap_err();
        assert!(matches!(err, AiError::NotAllowed(_)), "{err:?}");
        assert_eq!(request_count(&target).await, 0);
    }

    #[tokio::test]
    async fn redirect_to_other_allowlisted_origin_is_rejected() {
        // Both servers are 127.0.0.1 (allowlisted) but on different ports.
        let target = MockServer::start().await;
        let source = MockServer::start().await;
        redirect_to(format!("{}/x", target.uri())).mount(&source).await;
        Mock::given(method("GET")).respond_with(ResponseTemplate::new(200)).mount(&target).await;

        let url = u(&source.uri());
        let err = send(client().get(url.clone()), &url).await.unwrap_err();
        assert!(matches!(err, AiError::NotAllowed(_)), "{err:?}");
        assert_eq!(request_count(&target).await, 0);
    }

    #[tokio::test]
    async fn same_origin_redirect_is_followed() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/a"))
            .respond_with(ResponseTemplate::new(302).insert_header("location", "/b"))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/b"))
            .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
            .mount(&server)
            .await;
        let url = u(&format!("{}/a", server.uri()));
        let resp = send(client().get(url.clone()), &url).await.unwrap();
        assert_eq!(resp.text().await.unwrap(), "ok");
    }

    #[tokio::test]
    async fn send_rejects_request_that_targets_unapproved_url() {
        let approved = MockServer::start().await;
        let other = MockServer::start().await;
        let url = u(&approved.uri());

        // Builder aims at a disallowed host while `url` is allowed.
        let err = send(client().get("https://evil.com/x"), &url).await.unwrap_err();
        assert!(matches!(err, AiError::NotAllowed(_)), "{err:?}");

        // Builder aims at another allowed origin than the approved one.
        let err = send(client().get(other.uri()), &url).await.unwrap_err();
        assert!(matches!(err, AiError::NotAllowed(_)), "{err:?}");
        assert_eq!(request_count(&other).await, 0);
        assert_eq!(request_count(&approved).await, 0);
    }

    #[tokio::test]
    async fn error_message_fallbacks() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(500).set_body_string(r#"{"error":"boom"}"#))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(400).set_body_string(r#"{"message":"bad"}"#))
            .up_to_n_times(1)
            .mount(&server)
            .await;
        let long = "x".repeat(1000);
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(502).set_body_string(long))
            .mount(&server)
            .await;
        let url = u(&server.uri());
        let mut msgs = Vec::new();
        for _ in 0..3 {
            match send(client().get(url.clone()), &url).await.unwrap_err() {
                AiError::Http { message, .. } => msgs.push(message),
                e => panic!("{e:?}"),
            }
        }
        assert_eq!(msgs[0], "boom");
        assert_eq!(msgs[1], "bad");
        assert_eq!(msgs[2].chars().count(), 300);
    }
}
