//! OpenAI-compatible provider (OpenAI, Groq): SSE chat streaming and model listing.

use super::{net, AiError, ChatRequest, Provider, Role};
use async_trait::async_trait;
use serde_json::{json, Value};
use url::Url;

pub const OPENAI_BASE: &str = "https://api.openai.com";
pub const GROQ_BASE: &str = "https://api.groq.com/openai";

pub struct OpenAiCompat {
    base: String,
    api_key: String,
    client: reqwest::Client,
}

// Hand-written so the key can never reach logs through `{:?}`.
impl std::fmt::Debug for OpenAiCompat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenAiCompat")
            .field("base", &self.base)
            .field("api_key", &"<redacted>")
            .finish()
    }
}

impl OpenAiCompat {
    pub fn new(base_url: String, api_key: String) -> Self {
        let base = base_url.trim_end_matches('/').to_string();
        Self { client: net::client_for(&base), base, api_key }
    }

    fn url(&self, path: &str) -> Result<Url, AiError> {
        if self.api_key.is_empty() {
            return Err(AiError::MissingKey);
        }
        Url::parse(&format!("{}{}", self.base, path))
            .map_err(|_| AiError::Network("Invalid provider address".to_string()))
    }
}

fn request_body(req: &ChatRequest) -> Value {
    let mut messages = Vec::new();
    if let Some(system) = &req.system {
        messages.push(json!({ "role": "system", "content": system }));
    }
    for m in &req.messages {
        let role = match m.role {
            Role::User => "user",
            Role::Assistant => "assistant",
        };
        let content = if m.images.is_empty() {
            json!(m.text)
        } else {
            let mut parts = vec![json!({ "type": "text", "text": m.text })];
            for img in &m.images {
                let url = format!("data:{};base64,{}", img.mime, img.base64);
                parts.push(json!({ "type": "image_url", "image_url": { "url": url } }));
            }
            json!(parts)
        };
        messages.push(json!({ "role": role, "content": content }));
    }
    json!({
        "model": req.model,
        "stream": true,
        "max_completion_tokens": req.max_tokens,
        "messages": messages,
    })
}

/// Parses one SSE line into (text delta, stream finished).
fn parse_line(line: &str) -> Result<(Option<String>, bool), AiError> {
    let Some(data) = net::sse_data(line) else {
        return Ok((None, false));
    };
    let data = data.trim();
    if data == "[DONE]" {
        return Ok((None, true));
    }
    let v: Value = serde_json::from_str(data)
        .map_err(|_| AiError::Parse("unreadable event in response stream".to_string()))?;
    if let Some(err) = v.get("error") {
        let message = err.get("message").or(Some(err)).and_then(Value::as_str);
        return Err(AiError::Network(message.unwrap_or("provider reported an error").to_string()));
    }
    let text =
        v.pointer("/choices/0/delta/content").and_then(Value::as_str).filter(|t| !t.is_empty());
    Ok((text.map(str::to_string), false))
}

#[async_trait]
impl Provider for OpenAiCompat {
    async fn chat_stream(
        &self,
        req: &ChatRequest,
        on_delta: &mut (dyn for<'a> FnMut(&'a str) + Send),
    ) -> Result<String, AiError> {
        let url = self.url("/v1/chat/completions")?;
        let builder =
            self.client.post(url.clone()).bearer_auth(&self.api_key).json(&request_body(req));
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
        let url = self.url("/v1/models")?;
        let builder = self.client.get(url.clone()).bearer_auth(&self.api_key);
        let resp = net::send(builder, &url).await?;
        let text = resp.text().await.map_err(|e| AiError::Network(e.without_url().to_string()))?;
        let v: Value = serde_json::from_str(&text)
            .map_err(|_| AiError::Parse("unreadable model list".to_string()))?;
        let data = v
            .get("data")
            .and_then(Value::as_array)
            .ok_or_else(|| AiError::Parse("model list missing".to_string()))?;
        let mut ids: Vec<String> = data
            .iter()
            .filter_map(|m| m.get("id").and_then(Value::as_str))
            .map(str::to_string)
            .collect();
        ids.sort();
        Ok(ids)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::{ChatMessage, ImageInput};
    use wiremock::matchers::{body_partial_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn request() -> ChatRequest {
        ChatRequest {
            model: "gpt-test".into(),
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
    async fn openai_streams_with_bearer_and_image_parts() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .and(header("authorization", "Bearer sk-test"))
            .and(body_partial_json(json!({
                "model": "gpt-test",
                "stream": true,
                "max_completion_tokens": 256,
                "messages": [
                    { "role": "system", "content": "be brief" },
                    { "role": "user", "content": [
                        { "type": "text", "text": "what is this?" },
                        { "type": "image_url",
                          "image_url": { "url": "data:image/png;base64,QUJD" } }
                    ] },
                    { "role": "assistant", "content": "a cat" }
                ]
            })))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                "data: {\"choices\":[{\"delta\":{\"content\":\"Hel\"}}]}\n\n\
                 data: {\"choices\":[{\"delta\":{\"role\":\"assistant\"}}]}\n\n\
                 data: {\"choices\":[{\"delta\":{\"content\":\"lo\"}}]}\n\n\
                 data: [DONE]\n\n",
            ))
            .expect(1)
            .mount(&server)
            .await;

        let provider = OpenAiCompat::new(server.uri(), "sk-test".into());
        let mut deltas: Vec<String> = Vec::new();
        let full = provider
            .chat_stream(&request(), &mut |d: &str| deltas.push(d.to_string()))
            .await
            .unwrap();
        assert_eq!(deltas, vec!["Hel", "lo"]);
        assert_eq!(full, "Hello");
    }

    #[tokio::test]
    async fn openai_processes_unterminated_final_line() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                "data: {\"choices\":[{\"delta\":{\"content\":\"a\"}}]}\n\n\
                 data: {\"choices\":[{\"delta\":{\"content\":\"b\"}}]}",
            ))
            .mount(&server)
            .await;
        let provider = OpenAiCompat::new(server.uri(), "sk-test".into());
        let full = provider.chat_stream(&request(), &mut |_: &str| {}).await.unwrap();
        assert_eq!(full, "ab");
    }

    #[tokio::test]
    async fn openai_compat_lists_models_sorted() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .and(header("authorization", "Bearer sk-test"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_string(r#"{"data":[{"id":"zeta"},{"id":"alpha"},{"id":"mid"}]}"#),
            )
            .mount(&server)
            .await;
        let provider = OpenAiCompat::new(server.uri(), "sk-test".into());
        assert_eq!(provider.list_models().await.unwrap(), vec!["alpha", "mid", "zeta"]);
    }

    #[tokio::test]
    async fn openai_compat_401_maps_to_http_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(401)
                    .set_body_string(r#"{"error":{"message":"Incorrect API key provided"}}"#),
            )
            .mount(&server)
            .await;
        let provider = OpenAiCompat::new(server.uri(), "sk-secret-value".into());
        let err = provider.chat_stream(&request(), &mut |_: &str| {}).await.unwrap_err();
        assert_eq!(
            err,
            AiError::Http { status: 401, message: "Incorrect API key provided".into() }
        );
        assert!(!format!("{err:?}{err}").contains("sk-secret-value"));
    }

    #[tokio::test]
    async fn openai_compat_empty_key_is_missing_key_and_sends_nothing() {
        let server = MockServer::start().await;
        let provider = OpenAiCompat::new(server.uri(), String::new());
        let err = provider.chat_stream(&request(), &mut |_: &str| {}).await.unwrap_err();
        assert_eq!(err, AiError::MissingKey);
        assert_eq!(provider.list_models().await.unwrap_err(), AiError::MissingKey);
        assert!(server.received_requests().await.unwrap().is_empty());
    }

    #[test]
    fn debug_output_redacts_key() {
        let provider = OpenAiCompat::new(OPENAI_BASE.into(), "sk-secret-value".into());
        assert!(!format!("{provider:?}").contains("sk-secret-value"));
    }
}
