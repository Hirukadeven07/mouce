//! Allowlisted HTTP client and stream-line parsing for AI providers.

use super::AiError;
use futures_util::StreamExt;
use std::time::Duration;
use url::{Host, Url};

const REMOTE_HOSTS: &[&str] =
    &["api.openai.com", "api.groq.com", "generativelanguage.googleapis.com", "api.anthropic.com"];

const MAX_REDIRECTS: usize = 10;
const MAX_ERROR_MESSAGE_CHARS: usize = 300;
/// At most this much of an error body is read before parsing.
const MAX_ERROR_BODY_BYTES: usize = 64 * 1024;
/// A stream that stays silent this long is treated as stalled.
const STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(120);

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

/// A header value holding a credential, marked sensitive so HTTP debug output (and any
/// logging of the request) shows `Sensitive` instead of the key.
pub fn secret_header(secret: &str) -> Result<reqwest::header::HeaderValue, AiError> {
    let mut value = reqwest::header::HeaderValue::from_str(secret).map_err(|_| {
        AiError::Network("The API key contains characters that cannot be sent".to_string())
    })?;
    value.set_sensitive(true);
    Ok(value)
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
    let reason = resp.status().canonical_reason();
    let body = read_body_capped(resp, MAX_ERROR_BODY_BYTES).await;
    AiError::Http { status, message: error_message(&String::from_utf8_lossy(&body), reason) }
}

/// Reads at most `limit` bytes of the body, then stops (dropping the rest).
async fn read_body_capped(resp: reqwest::Response, limit: usize) -> Vec<u8> {
    let mut stream = resp.bytes_stream();
    let mut body = Vec::new();
    while body.len() < limit {
        match stream.next().await {
            Some(Ok(chunk)) => {
                let room = limit - body.len();
                body.extend_from_slice(&chunk[..chunk.len().min(room)]);
            }
            _ => break,
        }
    }
    body
}

/// Picks the first non-blank message: the provider's own, then the body text,
/// then the HTTP reason phrase. Every candidate is cut to 300 characters.
fn error_message(body: &str, reason: Option<&str>) -> String {
    let from_json = serde_json::from_str::<serde_json::Value>(body).ok().map(|v| {
        let pick = |x: Option<&serde_json::Value>| x.and_then(|s| s.as_str()).map(str::to_string);
        [pick(v.pointer("/error/message")), pick(v.get("error")), pick(v.get("message"))]
    });
    from_json
        .into_iter()
        .flatten()
        .flatten()
        .chain([body.to_string()])
        .map(|m| m.trim().chars().take(MAX_ERROR_MESSAGE_CHARS).collect::<String>())
        .find(|m| !m.trim().is_empty())
        .unwrap_or_else(|| reason.unwrap_or("(no details)").to_string())
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

/// What the stream reader should do after a line has been handled.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LineOutcome {
    Continue,
    Stop,
}

/// Reads a streaming response line by line, calling `on_line` for each complete
/// line. Reading stops at the first `Stop` (later lines are never delivered) or
/// the first error. A trailing unterminated line is delivered when the stream
/// ends; a stream that ends without `Stop` is still `Ok`.
pub async fn read_stream(
    resp: reqwest::Response,
    on_line: impl FnMut(&str) -> Result<LineOutcome, AiError>,
) -> Result<(), AiError> {
    read_stream_with_idle(resp, STREAM_IDLE_TIMEOUT, on_line).await
}

async fn read_stream_with_idle(
    resp: reqwest::Response,
    idle: Duration,
    mut on_line: impl FnMut(&str) -> Result<LineOutcome, AiError>,
) -> Result<(), AiError> {
    let mut stream = resp.bytes_stream();
    let mut lines = LineBuffer::new();
    loop {
        let next = tokio::time::timeout(idle, stream.next()).await.map_err(|_| {
            AiError::Network("The connection stalled — no data for 120 seconds".to_string())
        })?;
        match next {
            Some(chunk) => {
                let chunk = chunk.map_err(|e| AiError::Network(e.without_url().to_string()))?;
                for line in lines.push(&chunk) {
                    if on_line(&line)? == LineOutcome::Stop {
                        return Ok(());
                    }
                }
            }
            None => {
                if let Some(line) = lines.finish() {
                    on_line(&line)?;
                }
                return Ok(());
            }
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
    fn secret_header_is_marked_sensitive() {
        let value = secret_header("k-123").unwrap();
        assert!(value.is_sensitive());
        assert_eq!(value.as_bytes(), b"k-123");
        assert_eq!(format!("{value:?}"), "Sensitive");
    }

    #[test]
    fn secret_header_rejects_unsendable_keys_without_echoing_them() {
        let err = secret_header(
            "bad
key-123",
        )
        .unwrap_err();
        assert!(!err.to_string().contains("key-123"));
        assert!(matches!(err, AiError::Network(_)));
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

    async fn body_response(body: &'static str) -> (MockServer, reqwest::Response) {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(200).set_body_string(body))
            .mount(&server)
            .await;
        let url = u(&server.uri());
        let resp = send(client().get(url.clone()), &url).await.unwrap();
        (server, resp)
    }

    #[tokio::test]
    async fn read_stream_stops_at_stop_and_ignores_later_lines() {
        let (_server, resp) = body_response(
            "one
two
STOP
ignored
also ignored
",
        )
        .await;
        let mut seen = Vec::new();
        read_stream(resp, |line| {
            seen.push(line.to_string());
            Ok(if line == "STOP" { LineOutcome::Stop } else { LineOutcome::Continue })
        })
        .await
        .unwrap();
        assert_eq!(seen, vec!["one", "two", "STOP"]);
    }

    #[tokio::test]
    async fn read_stream_propagates_line_error_and_stops() {
        let (_server, resp) = body_response(
            "ok
bad
never
",
        )
        .await;
        let mut seen = Vec::new();
        let err = read_stream(resp, |line| {
            seen.push(line.to_string());
            if line == "bad" {
                Err(AiError::Parse("malformed".to_string()))
            } else {
                Ok(LineOutcome::Continue)
            }
        })
        .await
        .unwrap_err();
        assert_eq!(err, AiError::Parse("malformed".to_string()));
        assert_eq!(seen, vec!["ok", "bad"]);
    }

    #[tokio::test]
    async fn read_stream_delivers_final_unterminated_line() {
        let (_server, resp) = body_response(
            "first
last",
        )
        .await;
        let mut seen = Vec::new();
        read_stream(resp, |line| {
            seen.push(line.to_string());
            Ok(LineOutcome::Continue)
        })
        .await
        .unwrap();
        assert_eq!(seen, vec!["first", "last"]);
    }

    #[tokio::test]
    async fn error_message_skips_blank_candidates_and_uses_reason_phrase() {
        let server = MockServer::start().await;
        let bodies: [(u16, &str); 4] = [
            (400, r#"{"error":{"message":"   "},"message":"fallback"}"#),
            (400, r#"{"error":{"message":""}}"#),
            (401, ""),
            (401, "  \n "),
        ];
        for (status, body) in bodies {
            Mock::given(method("GET"))
                .respond_with(ResponseTemplate::new(status).set_body_string(body))
                .up_to_n_times(1)
                .mount(&server)
                .await;
        }
        let url = u(&server.uri());
        let mut msgs = Vec::new();
        for _ in 0..4 {
            match send(client().get(url.clone()), &url).await.unwrap_err() {
                AiError::Http { message, .. } => msgs.push(message),
                e => panic!("{e:?}"),
            }
        }
        assert_eq!(msgs[0], "fallback");
        // Everything extracted is blank, so the raw body text is the answer.
        assert_eq!(msgs[1], r#"{"error":{"message":""}}"#);
        assert_eq!(msgs[2], "Unauthorized");
        assert_eq!(msgs[3], "Unauthorized");
    }

    #[test]
    fn error_message_without_reason_phrase_says_no_details() {
        assert_eq!(error_message("", None), "(no details)");
        assert_eq!(error_message(" ", Some("Teapot")), "Teapot");
    }

    #[tokio::test]
    async fn error_message_from_json_is_truncated_char_safe() {
        let server = MockServer::start().await;
        // Multi-byte characters: a byte-based cut would panic or split one.
        let long = "é".repeat(1000);
        let body = serde_json::json!({ "error": { "message": long } }).to_string();
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(400).set_body_string(body))
            .mount(&server)
            .await;
        let url = u(&server.uri());
        match send(client().get(url.clone()), &url).await.unwrap_err() {
            AiError::Http { message, .. } => assert_eq!(message, "é".repeat(300)),
            e => panic!("{e:?}"),
        }
    }

    #[tokio::test]
    async fn error_body_is_read_only_up_to_64_kib() {
        let server = MockServer::start().await;
        // The useful JSON sits beyond the cap, so it must never be seen.
        let body = format!("{}{}", " ".repeat(MAX_ERROR_BODY_BYTES + 10), r#"{"message":"late"}"#);
        Mock::given(method("GET"))
            .respond_with(ResponseTemplate::new(403).set_body_string(body))
            .mount(&server)
            .await;
        let url = u(&server.uri());
        match send(client().get(url.clone()), &url).await.unwrap_err() {
            AiError::Http { status, message } => {
                assert_eq!(status, 403);
                assert_eq!(message, "Forbidden");
            }
            e => panic!("{e:?}"),
        }
    }

    #[tokio::test]
    async fn read_body_capped_stops_at_limit() {
        let (_server, resp) = body_response("0123456789").await;
        assert_eq!(read_body_capped(resp, 4).await, b"0123");
    }

    /// Serves one chunked response: a first line, then silence for `pause`.
    /// (wiremock cannot pause mid-body, so this uses a bare loopback socket.)
    fn stalling_server(pause: Duration) -> Url {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let Ok((mut sock, _)) = listener.accept() else { return };
            let mut buf = [0u8; 4096];
            let mut seen = Vec::new();
            while !seen.windows(4).any(|w| w == b"\r\n\r\n") {
                match sock.read(&mut buf) {
                    Ok(0) | Err(_) => return,
                    Ok(n) => seen.extend_from_slice(&buf[..n]),
                }
            }
            let head = "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nline\n\r\n";
            if sock.write_all(head.as_bytes()).is_err() || sock.flush().is_err() {
                return;
            }
            std::thread::sleep(pause);
            let _ = sock.write_all(b"0\r\n\r\n");
        });
        u(&format!("http://{addr}/"))
    }

    /// Serves one chunked response of `lines`, one line per chunk, `gap` apart, then ends it.
    fn trickling_server(lines: &'static [&'static str], gap: Duration) -> Url {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let Ok((mut sock, _)) = listener.accept() else { return };
            let mut buf = [0u8; 4096];
            let mut seen = Vec::new();
            while !seen.windows(4).any(|w| w == b"\r\n\r\n") {
                match sock.read(&mut buf) {
                    Ok(0) | Err(_) => return,
                    Ok(n) => seen.extend_from_slice(&buf[..n]),
                }
            }
            let head = "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n";
            if sock.write_all(head.as_bytes()).is_err() {
                return;
            }
            for (i, line) in lines.iter().enumerate() {
                if i > 0 {
                    std::thread::sleep(gap);
                }
                let chunk = format!("{:x}\r\n{line}\n\r\n", line.len() + 1);
                if sock.write_all(chunk.as_bytes()).is_err() || sock.flush().is_err() {
                    return;
                }
            }
            let _ = sock.write_all(b"0\r\n\r\n");
        });
        u(&format!("http://{addr}/"))
    }

    #[tokio::test]
    async fn read_stream_times_out_when_connection_goes_silent() {
        let url = stalling_server(Duration::from_millis(1500));
        let resp = send(client().get(url.clone()), &url).await.unwrap();
        let mut seen = Vec::new();
        let started = std::time::Instant::now();
        let err = read_stream_with_idle(resp, Duration::from_millis(200), |line| {
            seen.push(line.to_string());
            Ok(LineOutcome::Continue)
        })
        .await
        .unwrap_err();
        assert_eq!(
            err,
            AiError::Network("The connection stalled — no data for 120 seconds".to_string())
        );
        assert_eq!(seen, vec!["line"]);
        assert!(started.elapsed() < Duration::from_millis(1400), "{:?}", started.elapsed());
    }

    #[tokio::test]
    async fn read_stream_idle_timer_resets_on_each_chunk() {
        // Three lines 150 ms apart: the whole stream takes about 300 ms, longer than the
        // 200 ms idle limit, but no single silence is. A whole-stream deadline would fail.
        let url = trickling_server(&["a", "b", "c"], Duration::from_millis(150));
        let resp = send(client().get(url.clone()), &url).await.unwrap();
        let mut seen = Vec::new();
        let started = std::time::Instant::now();
        read_stream_with_idle(resp, Duration::from_millis(200), |line| {
            seen.push(line.to_string());
            Ok(LineOutcome::Continue)
        })
        .await
        .unwrap();
        assert_eq!(seen, vec!["a", "b", "c"]);
        assert!(started.elapsed() > Duration::from_millis(200), "{:?}", started.elapsed());
    }
}
