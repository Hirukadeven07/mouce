//! Ollama provider: NDJSON chat streaming and model listing on a local server.

use super::{net, AiError, ChatRequest, Provider, Role};
use async_trait::async_trait;
use futures_util::StreamExt;
use serde_json::{json, Value};
use url::Url;

pub struct OllamaProvider {
    base: String,
    client: reqwest::Client,
}

impl OllamaProvider {
    pub fn new(base_url: String) -> Self {
        Self { base: base_url.trim_end_matches('/').to_string(), client: net::client() }
    }

    fn url(&self, path: &str) -> Result<Url, AiError> {
        Url::parse(&format!("{}{}", self.base, path))
            .map_err(|_| AiError::Network("Invalid Ollama address".to_string()))
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
        let mut msg = json!({ "role": role, "content": m.text });
        if !m.images.is_empty() {
            let images: Vec<&str> = m.images.iter().map(|i| i.base64.as_str()).collect();
            msg["images"] = json!(images);
        }
        messages.push(msg);
    }
    json!({ "model": req.model, "stream": true, "messages": messages })
}

/// Parses one NDJSON line into (text delta, stream finished).
fn parse_line(line: &str) -> Result<(Option<String>, bool), AiError> {
    let v: Value = serde_json::from_str(line)
        .map_err(|_| AiError::Parse("unreadable line in Ollama stream".to_string()))?;
    if let Some(err) = v.get("error").and_then(Value::as_str) {
        return Err(AiError::Network(err.to_string()));
    }
    let text = v.pointer("/message/content").and_then(Value::as_str).filter(|t| !t.is_empty());
    Ok((text.map(str::to_string), v.get("done").and_then(Value::as_bool).unwrap_or(false)))
}

#[async_trait]
impl Provider for OllamaProvider {
    async fn chat_stream(
        &self,
        req: &ChatRequest,
        on_delta: &mut (dyn for<'a> FnMut(&'a str) + Send),
    ) -> Result<String, AiError> {
        let url = self.url("/api/chat")?;
        let resp = net::send(self.client.post(url.clone()).json(&request_body(req)), &url).await?;
        let mut stream = resp.bytes_stream();
        let mut lines = net::LineBuffer::new();
        let mut full = String::new();
        let mut ended = false;
        while !ended {
            let batch: Vec<String> = match stream.next().await {
                Some(chunk) => {
                    let chunk = chunk.map_err(|e| AiError::Network(e.without_url().to_string()))?;
                    lines.push(&chunk)
                }
                None => {
                    ended = true;
                    std::mem::take(&mut lines).finish().into_iter().collect()
                }
            };
            for line in batch {
                let (text, finished) = parse_line(&line)?;
                if let Some(text) = text {
                    full.push_str(&text);
                    on_delta(&text);
                }
                if finished {
                    return Ok(full);
                }
            }
        }
        Ok(full)
    }

    async fn list_models(&self) -> Result<Vec<String>, AiError> {
        let url = self.url("/api/tags")?;
        let resp = net::send(self.client.get(url.clone()), &url).await?;
        let text = resp.text().await.map_err(|e| AiError::Network(e.without_url().to_string()))?;
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::{ChatMessage, ImageInput};
    use wiremock::matchers::{body_partial_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn request() -> ChatRequest {
        ChatRequest {
            model: "gemma3:4b".into(),
            system: Some("be brief".into()),
            messages: vec![ChatMessage {
                role: Role::User,
                text: "what is this?".into(),
                images: vec![ImageInput { mime: "image/png".into(), base64: "QUJD".into() }],
            }],
            max_tokens: 100,
        }
    }

    #[tokio::test]
    async fn ollama_streams_and_sends_images() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .and(body_partial_json(json!({
                "model": "gemma3:4b",
                "stream": true,
                "messages": [
                    { "role": "system", "content": "be brief" },
                    { "role": "user", "content": "what is this?", "images": ["QUJD"] }
                ]
            })))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                "{\"message\":{\"content\":\"Hel\"},\"done\":false}\n\
                 {\"message\":{\"content\":\"lo\"},\"done\":false}\n\
                 {\"message\":{\"content\":\"\"},\"done\":true}\n",
            ))
            .expect(1)
            .mount(&server)
            .await;

        let provider = OllamaProvider::new(server.uri());
        let mut deltas: Vec<String> = Vec::new();
        let full = provider
            .chat_stream(&request(), &mut |d: &str| deltas.push(d.to_string()))
            .await
            .unwrap();
        assert_eq!(deltas, vec!["Hel", "lo"]);
        assert_eq!(full, "Hello");
    }

    #[tokio::test]
    async fn ollama_omits_images_when_empty_and_handles_unterminated_last_line() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                "{\"message\":{\"content\":\"a\"},\"done\":false}\n\
                 {\"message\":{\"content\":\"b\"},\"done\":false}",
            ))
            .mount(&server)
            .await;

        let mut req = request();
        req.messages[0].images.clear();
        let provider = OllamaProvider::new(format!("{}/", server.uri()));
        let full = provider.chat_stream(&req, &mut |_: &str| {}).await.unwrap();
        assert_eq!(full, "ab");

        let received = server.received_requests().await.unwrap();
        let body: Value = serde_json::from_slice(&received[0].body).unwrap();
        assert!(body["messages"][1].get("images").is_none(), "{body}");
    }

    #[tokio::test]
    async fn ollama_error_status_maps_to_http_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(404).set_body_string(r#"{"error":"model not found"}"#),
            )
            .mount(&server)
            .await;
        let provider = OllamaProvider::new(server.uri());
        let err = provider.chat_stream(&request(), &mut |_: &str| {}).await.unwrap_err();
        assert_eq!(err, AiError::Http { status: 404, message: "model not found".into() });
    }

    #[tokio::test]
    async fn ollama_lists_models() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/tags"))
            .respond_with(
                ResponseTemplate::new(200).set_body_string(r#"{"models":[{"name":"gemma3:4b"}]}"#),
            )
            .mount(&server)
            .await;
        let provider = OllamaProvider::new(server.uri());
        assert_eq!(provider.list_models().await.unwrap(), vec!["gemma3:4b"]);
    }
}
