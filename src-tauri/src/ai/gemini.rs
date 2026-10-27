//! Google Gemini provider: SSE chat streaming and model listing.

use super::{net, AiError, ChatRequest, Provider, Role};
use async_trait::async_trait;
use serde_json::{json, Value};
use url::Url;

pub const GEMINI_BASE: &str = "https://generativelanguage.googleapis.com";

pub struct GeminiProvider {
    base: String,
    api_key: String,
    client: reqwest::Client,
}

// Hand-written so the key can never reach logs through `{:?}`.
impl std::fmt::Debug for GeminiProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GeminiProvider")
            .field("base", &self.base)
            .field("api_key", &"<redacted>")
            .finish()
    }
}

impl GeminiProvider {
    pub fn new(base_url: String, api_key: String) -> Self {
        Self { base: base_url.trim_end_matches('/').to_string(), api_key, client: net::client() }
    }

    /// `{base}/v1beta/models[/{last}]`; each segment is percent-encoded, so a
    /// model name cannot change the path or add query parameters.
    fn url(&self, last: Option<&str>) -> Result<Url, AiError> {
        if self.api_key.is_empty() {
            return Err(AiError::MissingKey);
        }
        let invalid = || AiError::Network("Invalid provider address".to_string());
        let mut url = Url::parse(&self.base).map_err(|_| invalid())?;
        {
            let mut segments = url.path_segments_mut().map_err(|_| invalid())?;
            segments.pop_if_empty().push("v1beta").push("models");
            if let Some(last) = last {
                segments.push(last);
            }
        }
        Ok(url)
    }
}

fn request_body(req: &ChatRequest) -> Value {
    let contents: Vec<Value> = req
        .messages
        .iter()
        .map(|m| {
            let role = match m.role {
                Role::User => "user",
                Role::Assistant => "model",
            };
            let mut parts = vec![json!({ "text": m.text })];
            for img in &m.images {
                parts.push(json!({
                    "inline_data": { "mime_type": img.mime, "data": img.base64 }
                }));
            }
            json!({ "role": role, "parts": parts })
        })
        .collect();
    let mut body = json!({
        "contents": contents,
        "generationConfig": { "maxOutputTokens": req.max_tokens },
    });
    if let Some(system) = &req.system {
        body["systemInstruction"] = json!({ "parts": [{ "text": system }] });
    }
    body
}

/// Parses one SSE line into its concatenated text parts, if any.
fn parse_line(line: &str) -> Result<Option<String>, AiError> {
    let Some(data) = net::sse_data(line) else {
        return Ok(None);
    };
    let v: Value = serde_json::from_str(data.trim())
        .map_err(|_| AiError::Parse("unreadable event in response stream".to_string()))?;
    if let Some(err) = v.get("error") {
        let message = err.get("message").and_then(Value::as_str);
        return Err(AiError::Http {
            status: 500,
            message: message.unwrap_or("provider reported an error").to_string(),
        });
    }
    let parts = v.pointer("/candidates/0/content/parts").and_then(Value::as_array);
    let text: String = parts
        .into_iter()
        .flatten()
        .filter_map(|p| p.get("text").and_then(Value::as_str))
        .collect();
    Ok(Some(text).filter(|t| !t.is_empty()))
}

#[async_trait]
impl Provider for GeminiProvider {
    async fn chat_stream(
        &self,
        req: &ChatRequest,
        on_delta: &mut (dyn for<'a> FnMut(&'a str) + Send),
    ) -> Result<String, AiError> {
        let mut url = self.url(Some(&format!("{}:streamGenerateContent", req.model)))?;
        url.query_pairs_mut().append_pair("alt", "sse");
        let builder = self
            .client
            .post(url.clone())
            .header("x-goog-api-key", &self.api_key)
            .json(&request_body(req));
        let resp = net::send(builder, &url).await?;
        let mut full = String::new();
        net::read_stream(resp, |line| {
            if let Some(text) = parse_line(line)? {
                full.push_str(&text);
                on_delta(&text);
            }
            Ok(net::LineOutcome::Continue)
        })
        .await?;
        Ok(full)
    }

    async fn list_models(&self) -> Result<Vec<String>, AiError> {
        let mut url = self.url(None)?;
        url.query_pairs_mut().append_pair("pageSize", "1000");
        let builder = self.client.get(url.clone()).header("x-goog-api-key", &self.api_key);
        let resp = net::send(builder, &url).await?;
        let text = resp.text().await.map_err(|e| AiError::Network(e.without_url().to_string()))?;
        let v: Value = serde_json::from_str(&text)
            .map_err(|_| AiError::Parse("unreadable model list".to_string()))?;
        let models = v
            .get("models")
            .and_then(Value::as_array)
            .ok_or_else(|| AiError::Parse("model list missing".to_string()))?;
        Ok(models
            .iter()
            .filter(|m| {
                m.get("supportedGenerationMethods")
                    .and_then(Value::as_array)
                    .is_some_and(|methods| methods.iter().any(|x| x == "generateContent"))
            })
            .filter_map(|m| m.get("name").and_then(Value::as_str))
            .map(|name| name.strip_prefix("models/").unwrap_or(name).to_string())
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

    fn request() -> ChatRequest {
        ChatRequest {
            model: "gemini-test".into(),
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
    async fn gemini_streams_with_key_header_and_inline_image() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1beta/models/gemini-test:streamGenerateContent"))
            .and(query_param("alt", "sse"))
            .and(header("x-goog-api-key", "g-test"))
            .and(body_partial_json(json!({
                "contents": [
                    { "role": "user", "parts": [
                        { "text": "what is this?" },
                        { "inline_data": { "mime_type": "image/png", "data": "QUJD" } }
                    ] },
                    { "role": "model", "parts": [ { "text": "a cat" } ] }
                ],
                "systemInstruction": { "parts": [ { "text": "be brief" } ] },
                "generationConfig": { "maxOutputTokens": 256 }
            })))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"Hel\"},{\"text\":\"l\"}]}}]}\r\n\r\n\
                 data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"o\"}]}}]}\r\n\r\n",
            ))
            .expect(1)
            .mount(&server)
            .await;

        let provider = GeminiProvider::new(server.uri(), "g-test".into());
        let mut deltas: Vec<String> = Vec::new();
        let full = provider
            .chat_stream(&request(), &mut |d: &str| deltas.push(d.to_string()))
            .await
            .unwrap();
        assert_eq!(deltas, vec!["Hell", "o"]);
        assert_eq!(full, "Hello");

        // The key travels only in the header, never in the URL.
        let received = server.received_requests().await.unwrap();
        assert!(!received[0].url.as_str().contains("g-test"), "{}", received[0].url);
    }

    #[tokio::test]
    async fn gemini_maps_assistant_role_to_model() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"ok\"}]}}]}\n\n",
            ))
            .mount(&server)
            .await;
        let provider = GeminiProvider::new(format!("{}/", server.uri()), "g-test".into());
        let mut req = request();
        req.system = None;
        provider.chat_stream(&req, &mut |_: &str| {}).await.unwrap();
        let received = server.received_requests().await.unwrap();
        let body: Value = serde_json::from_slice(&received[0].body).unwrap();
        assert_eq!(body["contents"][0]["role"], "user");
        assert_eq!(body["contents"][1]["role"], "model");
        assert!(body.get("systemInstruction").is_none(), "{body}");
    }

    #[tokio::test]
    async fn gemini_lists_generate_models_only() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1beta/models"))
            .and(header("x-goog-api-key", "g-test"))
            .respond_with(ResponseTemplate::new(200).set_body_string(
                r#"{"models":[
                    {"name":"models/gemini-a","supportedGenerationMethods":["generateContent","countTokens"]},
                    {"name":"models/embed-b","supportedGenerationMethods":["embedContent"]},
                    {"name":"models/gemini-c","supportedGenerationMethods":["generateContent"]}
                ]}"#,
            ))
            .mount(&server)
            .await;
        let provider = GeminiProvider::new(server.uri(), "g-test".into());
        assert_eq!(provider.list_models().await.unwrap(), vec!["gemini-a", "gemini-c"]);
    }

    #[tokio::test]
    async fn gemini_empty_key_is_missing_key_and_sends_nothing() {
        let server = MockServer::start().await;
        let provider = GeminiProvider::new(server.uri(), String::new());
        let err = provider.chat_stream(&request(), &mut |_: &str| {}).await.unwrap_err();
        assert_eq!(err, AiError::MissingKey);
        assert_eq!(provider.list_models().await.unwrap_err(), AiError::MissingKey);
        assert!(server.received_requests().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn gemini_error_and_debug_never_contain_key() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(
                ResponseTemplate::new(400)
                    .set_body_string(r#"{"error":{"message":"API key not valid"}}"#),
            )
            .mount(&server)
            .await;
        let provider = GeminiProvider::new(server.uri(), "g-secret-value".into());
        let err = provider.chat_stream(&request(), &mut |_: &str| {}).await.unwrap_err();
        assert_eq!(err, AiError::Http { status: 400, message: "API key not valid".into() });
        assert!(!format!("{err:?}{err}{provider:?}").contains("g-secret-value"));
    }

    #[tokio::test]
    async fn gemini_model_name_cannot_alter_the_request_path() {
        let server = MockServer::start().await;
        Mock::given(method("POST")).respond_with(ResponseTemplate::new(200)).mount(&server).await;
        let provider = GeminiProvider::new(server.uri(), "g-test".into());
        let mut req = request();
        req.model = "x/../../evil?alt=json".into();
        provider.chat_stream(&req, &mut |_: &str| {}).await.unwrap();
        let received = server.received_requests().await.unwrap();
        assert!(received[0].url.path().starts_with("/v1beta/models/x%2F"), "{}", received[0].url);
        assert_eq!(received[0].url.query(), Some("alt=sse"));
    }
}
