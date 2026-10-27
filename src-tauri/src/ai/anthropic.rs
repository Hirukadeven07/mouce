//! Anthropic provider: SSE chat streaming and model listing.

use super::{net, AiError, ChatRequest, Provider, Role};
use async_trait::async_trait;
use serde_json::{json, Value};
use url::Url;

pub const ANTHROPIC_BASE: &str = "https://api.anthropic.com";
const API_VERSION: &str = "2023-06-01";

pub struct AnthropicProvider {
    base: String,
    api_key: String,
    client: reqwest::Client,
}

// Hand-written so the key can never reach logs through `{:?}`.
impl std::fmt::Debug for AnthropicProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnthropicProvider")
            .field("base", &self.base)
            .field("api_key", &"<redacted>")
            .finish()
    }
}

impl AnthropicProvider {
    pub fn new(base_url: String, api_key: String) -> Self {
        Self { base: base_url.trim_end_matches('/').to_string(), api_key, client: net::client() }
    }

    fn url(&self, path: &str) -> Result<Url, AiError> {
        if self.api_key.is_empty() {
            return Err(AiError::MissingKey);
        }
        Url::parse(&format!("{}{}", self.base, path))
            .map_err(|_| AiError::Network("Invalid provider address".to_string()))
    }

    fn authed(&self, builder: reqwest::RequestBuilder) -> Result<reqwest::RequestBuilder, AiError> {
        Ok(builder
            .header("x-api-key", net::secret_header(&self.api_key)?)
            .header("anthropic-version", API_VERSION))
    }
}

fn request_body(req: &ChatRequest) -> Value {
    let messages: Vec<Value> = req
        .messages
        .iter()
        .map(|m| {
            let role = match m.role {
                Role::User => "user",
                Role::Assistant => "assistant",
            };
            // Images go before the text.
            let mut content: Vec<Value> = m
                .images
                .iter()
                .map(|img| {
                    json!({
                        "type": "image",
                        "source": { "type": "base64", "media_type": img.mime, "data": img.base64 }
                    })
                })
                .collect();
            // Anthropic rejects empty text blocks, so an image-only message has none.
            if !(m.text.trim().is_empty() && !m.images.is_empty()) {
                content.push(json!({ "type": "text", "text": m.text }));
            }
            json!({ "role": role, "content": content })
        })
        .collect();
    let mut body = json!({
        "model": req.model,
        "max_tokens": req.max_tokens,
        "stream": true,
        "messages": messages,
    });
    if let Some(system) = &req.system {
        body["system"] = json!(system);
    }
    body
}

/// Parses one SSE line into (text delta, stream finished).
fn parse_line(line: &str) -> Result<(Option<String>, bool), AiError> {
    let Some(data) = net::sse_data(line) else {
        return Ok((None, false));
    };
    let v: Value = serde_json::from_str(data.trim())
        .map_err(|_| AiError::Parse("unreadable event in response stream".to_string()))?;
    match v.get("type").and_then(Value::as_str) {
        Some("content_block_delta")
            if v.pointer("/delta/type").and_then(Value::as_str) == Some("text_delta") =>
        {
            let text = v.pointer("/delta/text").and_then(Value::as_str).filter(|t| !t.is_empty());
            Ok((text.map(str::to_string), false))
        }
        Some("message_stop") => Ok((None, true)),
        Some("error") => {
            let message = v.pointer("/error/message").and_then(Value::as_str);
            Err(AiError::Http {
                status: 500,
                message: message.unwrap_or("provider reported an error").to_string(),
            })
        }
        _ => Ok((None, false)),
    }
}

#[async_trait]
impl Provider for AnthropicProvider {
    async fn chat_stream(
        &self,
        req: &ChatRequest,
        on_delta: &mut (dyn for<'a> FnMut(&'a str) + Send),
    ) -> Result<String, AiError> {
        let url = self.url("/v1/messages")?;
        let builder = self.authed(self.client.post(url.clone()))?.json(&request_body(req));
        let resp = net::send(builder, &url).await?;
        let mut full = String::new();
        net::read_stream(resp, |line| {
            let (text, finished) = parse_line(line)?;
            if let Some(text) = text {
                full.push_str(&text);
                on_delta(&text);
            }
            Ok(if finished { net::LineOutcome::Stop } else { net::LineOutcome::Continue })
        })
        .await?;
        Ok(full)
    }

    async fn list_models(&self) -> Result<Vec<String>, AiError> {
        let mut url = self.url("/v1/models")?;
        url.query_pairs_mut().append_pair("limit", "1000");
        let resp = net::send(self.authed(self.client.get(url.clone()))?, &url).await?;
        let text = resp.text().await.map_err(|e| AiError::Network(e.without_url().to_string()))?;
        let v: Value = serde_json::from_str(&text)
            .map_err(|_| AiError::Parse("unreadable model list".to_string()))?;
        let data = v
            .get("data")
            .and_then(Value::as_array)
            .ok_or_else(|| AiError::Parse("model list missing".to_string()))?;
        Ok(data
            .iter()
            .filter_map(|m| m.get("id").and_then(Value::as_str))
            .map(str::to_string)
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::{ChatMessage, ImageInput, Role};
    use serde_json::{json, Value};
    use wiremock::matchers::{body_partial_json, header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn anthropic_key_header_is_sensitive() {
        let provider = AnthropicProvider::new("https://api.anthropic.com".into(), "a-test".into());
        let built = provider
            .authed(provider.client.get("https://api.anthropic.com/v1/models"))
            .unwrap()
            .build()
            .unwrap();
        let key = built.headers().get("x-api-key").expect("key header");
        assert!(key.is_sensitive());
        assert!(!built.headers().get("anthropic-version").unwrap().is_sensitive());
    }

    fn request() -> ChatRequest {
        ChatRequest {
            model: "claude-test".into(),
            system: Some("be brief".into()),
            messages: vec![
                ChatMessage {
                    role: Role::User,
                    text: "what is this?".into(),
                    images: vec![ImageInput { mime: "image/png".into(), base64: "QUJD".into() }],
                },
                ChatMessage { role: Role::Assistant, text: "a cat".into(), images: vec![] },
            ],
            max_tokens: 256,
        }
    }

    #[tokio::test]
    async fn anthropic_streams_text_deltas_only() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                "event: message_start\n\
                 data: {\"type\":\"message_start\",\"message\":{\"id\":\"m\"}}\n\n\
                 event: ping\n\
                 data: {\"type\":\"ping\"}\n\n\
                 event: content_block_delta\n\
                 data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hel\"}}\n\n\
                 event: content_block_delta\n\
                 data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"lo\"}}\n\n\
                 event: message_stop\n\
                 data: {\"type\":\"message_stop\"}\n\n",
            ))
            .mount(&server)
            .await;
        let provider = AnthropicProvider::new(server.uri(), "a-test".into());
        let mut deltas: Vec<String> = Vec::new();
        let full = provider
            .chat_stream(&request(), &mut |d: &str| deltas.push(d.to_string()))
            .await
            .unwrap();
        assert_eq!(deltas, vec!["Hel", "lo"]);
        assert_eq!(full, "Hello");
    }

    #[tokio::test]
    async fn anthropic_non_text_deltas_are_ignored_and_stop_ends_stream() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                "data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{}\"}}\n\n\
                 data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"x\"}}\n\n\
                 data: {\"type\":\"message_stop\"}\n\n\
                 data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"late\"}}\n\n",
            ))
            .mount(&server)
            .await;
        let provider = AnthropicProvider::new(server.uri(), "a-test".into());
        let full = provider.chat_stream(&request(), &mut |_: &str| {}).await.unwrap();
        assert_eq!(full, "x");
    }

    #[tokio::test]
    async fn anthropic_sends_version_header_and_image_first() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/messages"))
            .and(header("x-api-key", "a-test"))
            .and(header("anthropic-version", "2023-06-01"))
            .and(body_partial_json(json!({
                "model": "claude-test",
                "max_tokens": 256,
                "stream": true,
                "system": "be brief",
                "messages": [
                    { "role": "user", "content": [
                        { "type": "image",
                          "source": { "type": "base64", "media_type": "image/png", "data": "QUJD" } },
                        { "type": "text", "text": "what is this?" }
                    ] },
                    { "role": "assistant", "content": [ { "type": "text", "text": "a cat" } ] }
                ]
            })))
            .respond_with(
                ResponseTemplate::new(200).set_body_string("data: {\"type\":\"message_stop\"}\n\n"),
            )
            .expect(1)
            .mount(&server)
            .await;
        let provider = AnthropicProvider::new(format!("{}/", server.uri()), "a-test".into());
        provider.chat_stream(&request(), &mut |_: &str| {}).await.unwrap();

        // Without a system prompt the field is omitted entirely (this request no
        // longer matches the mock above, which requires "system").
        let mut req = request();
        req.system = None;
        let _ = provider.chat_stream(&req, &mut |_: &str| {}).await;
        let received = server.received_requests().await.unwrap();
        let body: Value = serde_json::from_slice(&received[1].body).unwrap();
        assert!(body.get("system").is_none(), "{body}");
    }

    #[tokio::test]
    async fn anthropic_stream_error_event_maps_to_http_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                "data: {\"type\":\"content_block_delta\",\"delta\":{\"type\":\"text_delta\",\"text\":\"a\"}}\n\n\
                 event: error\n\
                 data: {\"type\":\"error\",\"error\":{\"type\":\"overloaded_error\",\"message\":\"Overloaded\"}}\n\n",
            ))
            .mount(&server)
            .await;
        let provider = AnthropicProvider::new(server.uri(), "a-test".into());
        let err = provider.chat_stream(&request(), &mut |_: &str| {}).await.unwrap_err();
        assert_eq!(err, AiError::Http { status: 500, message: "Overloaded".into() });
    }

    #[tokio::test]
    async fn anthropic_lists_models() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .and(query_param("limit", "1000"))
            .and(header("x-api-key", "a-test"))
            .and(header("anthropic-version", "2023-06-01"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(r#"{"data":[{"id":"claude-b"},{"id":"claude-a"}]}"#),
            )
            .mount(&server)
            .await;
        let provider = AnthropicProvider::new(server.uri(), "a-test".into());
        assert_eq!(provider.list_models().await.unwrap(), vec!["claude-b", "claude-a"]);
    }

    #[tokio::test]
    async fn anthropic_image_only_message_has_no_text_block() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(200).set_body_string("data: {\"type\":\"message_stop\"}\n\n"),
            )
            .mount(&server)
            .await;
        let provider = AnthropicProvider::new(server.uri(), "a-test".into());
        let img = || ImageInput { mime: "image/png".into(), base64: "QUJD".into() };
        let req = ChatRequest {
            model: "claude-test".into(),
            system: None,
            messages: vec![
                ChatMessage { role: Role::User, text: String::new(), images: vec![img()] },
                ChatMessage { role: Role::User, text: " \n\t ".into(), images: vec![img()] },
                ChatMessage { role: Role::User, text: "hi".into(), images: vec![] },
            ],
            max_tokens: 10,
        };
        provider.chat_stream(&req, &mut |_: &str| {}).await.unwrap();
        let received = server.received_requests().await.unwrap();
        let body: Value = serde_json::from_slice(&received[0].body).unwrap();
        let image = json!({
            "type": "image",
            "source": { "type": "base64", "media_type": "image/png", "data": "QUJD" }
        });
        assert_eq!(body["messages"][0]["content"], json!([image.clone()]), "{body}");
        assert_eq!(body["messages"][1]["content"], json!([image]), "{body}");
        assert_eq!(body["messages"][2]["content"], json!([{ "type": "text", "text": "hi" }]));
    }

    #[tokio::test]
    async fn anthropic_empty_key_is_missing_key_and_sends_nothing() {
        let server = MockServer::start().await;
        let provider = AnthropicProvider::new(server.uri(), String::new());
        let err = provider.chat_stream(&request(), &mut |_: &str| {}).await.unwrap_err();
        assert_eq!(err, AiError::MissingKey);
        assert_eq!(provider.list_models().await.unwrap_err(), AiError::MissingKey);
        assert!(server.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn anthropic_error_and_debug_never_contain_key() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(401).set_body_string(
                r#"{"type":"error","error":{"type":"authentication_error","message":"invalid x-api-key"}}"#,
            ))
            .mount(&server)
            .await;
        let provider = AnthropicProvider::new(server.uri(), "a-secret-value".into());
        let err = provider.chat_stream(&request(), &mut |_: &str| {}).await.unwrap_err();
        assert_eq!(err, AiError::Http { status: 401, message: "invalid x-api-key".into() });
        assert!(!format!("{err:?}{err}{provider:?}").contains("a-secret-value"));
    }
}
