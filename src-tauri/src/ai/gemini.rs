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

    /// Adds the key as a sensitive header; it never goes in the URL.
    fn keyed(&self, builder: reqwest::RequestBuilder) -> Result<reqwest::RequestBuilder, AiError> {
        Ok(builder.header("x-goog-api-key", net::secret_header(&self.api_key)?))
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
            let mut parts = Vec::new();
            // No empty text part for an image-only message.
            if !(m.text.trim().is_empty() && !m.images.is_empty()) {
                parts.push(json!({ "text": m.text }));
            }
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

/// Finish reasons that mean Gemini refused to produce an answer.
const DECLINING_FINISH_REASONS: &[&str] =
    &["SAFETY", "RECITATION", "BLOCKLIST", "PROHIBITED_CONTENT", "SPII"];

/// What one SSE line carried.
#[derive(Default)]
struct ParsedLine {
    text: Option<String>,
    /// Set when the prompt was blocked or a candidate ended with a declining reason.
    declined: Option<String>,
}

/// Parses one SSE line into its concatenated text parts and any decline reason.
fn parse_line(line: &str) -> Result<ParsedLine, AiError> {
    let Some(data) = net::sse_data(line) else {
        return Ok(ParsedLine::default());
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
    let text: String =
        parts.into_iter().flatten().filter_map(|p| p.get("text").and_then(Value::as_str)).collect();
    let block = v.pointer("/promptFeedback/blockReason").and_then(Value::as_str);
    let finish = v
        .get("candidates")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|c| c.get("finishReason").and_then(Value::as_str))
        .find(|r| DECLINING_FINISH_REASONS.contains(r));
    Ok(ParsedLine {
        text: Some(text).filter(|t| !t.is_empty()),
        declined: block.or(finish).map(str::to_string),
    })
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
        let builder = self.keyed(self.client.post(url.clone()))?.json(&request_body(req));
        let resp = net::send(builder, &url).await?;
        let mut full = String::new();
        let mut declined: Option<String> = None;
        net::read_stream(resp, |line| {
            let parsed = parse_line(line)?;
            if let Some(text) = parsed.text {
                full.push_str(&text);
                on_delta(&text);
            }
            if declined.is_none() {
                declined = parsed.declined;
            }
            Ok(net::LineOutcome::Continue)
        })
        .await?;
        match declined {
            Some(reason) if full.is_empty() => Err(AiError::Http {
                status: 400,
                message: format!("Gemini declined to answer ({reason})"),
            }),
            _ => Ok(full),
        }
    }

    async fn list_models(&self) -> Result<Vec<String>, AiError> {
        let mut url = self.url(None)?;
        url.query_pairs_mut().append_pair("pageSize", "1000");
        let builder = self.keyed(self.client.get(url.clone()))?;
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

    #[test]
    fn gemini_key_header_is_sensitive() {
        let provider = GeminiProvider::new(GEMINI_BASE.into(), "g-test".into());
        let built = provider
            .keyed(provider.client.get("https://generativelanguage.googleapis.com/v1beta/models"))
            .unwrap()
            .build()
            .unwrap();
        assert!(built.headers().get("x-goog-api-key").expect("key header").is_sensitive());
    }

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
            .and(query_param("pageSize", "1000"))
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
    async fn gemini_image_only_message_has_no_text_part() {
        let server = MockServer::start().await;
        Mock::given(method("POST")).respond_with(ResponseTemplate::new(200)).mount(&server).await;
        let provider = GeminiProvider::new(server.uri(), "g-test".into());
        let img = || ImageInput { mime: "image/png".into(), base64: "QUJD".into() };
        let req = ChatRequest {
            model: "gemini-test".into(),
            system: None,
            messages: vec![
                ChatMessage { role: Role::User, text: String::new(), images: vec![img()] },
                ChatMessage { role: Role::User, text: "  ".into(), images: vec![img()] },
                ChatMessage { role: Role::User, text: "hi".into(), images: vec![] },
            ],
            max_tokens: 10,
        };
        provider.chat_stream(&req, &mut |_: &str| {}).await.unwrap();
        let received = server.received_requests().await.unwrap();
        let body: Value = serde_json::from_slice(&received[0].body).unwrap();
        let image = json!({ "inline_data": { "mime_type": "image/png", "data": "QUJD" } });
        assert_eq!(body["contents"][0]["parts"], json!([image.clone()]), "{body}");
        assert_eq!(body["contents"][1]["parts"], json!([image]), "{body}");
        assert_eq!(body["contents"][2]["parts"], json!([{ "text": "hi" }]), "{body}");
    }

    async fn stream_result(sse: String) -> Result<String, AiError> {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200).set_body_string(sse))
            .mount(&server)
            .await;
        let provider = GeminiProvider::new(server.uri(), "g-test".into());
        provider.chat_stream(&request(), &mut |_: &str| {}).await
    }

    fn declined(reason: &str) -> AiError {
        AiError::Http { status: 400, message: format!("Gemini declined to answer ({reason})") }
    }

    #[tokio::test]
    async fn gemini_prompt_block_with_no_text_is_an_error() {
        let err =
            stream_result("data: {\"promptFeedback\":{\"blockReason\":\"OTHER\"}}\n\n".into())
                .await
                .unwrap_err();
        assert_eq!(err, declined("OTHER"));
    }

    #[tokio::test]
    async fn gemini_declining_finish_reasons_with_no_text_are_errors() {
        for reason in ["SAFETY", "RECITATION", "BLOCKLIST", "PROHIBITED_CONTENT", "SPII"] {
            let sse = format!("data: {{\"candidates\":[{{\"finishReason\":\"{reason}\"}}]}}\n\n");
            assert_eq!(stream_result(sse).await.unwrap_err(), declined(reason), "{reason}");
        }
    }

    #[tokio::test]
    async fn gemini_empty_stop_stream_is_ok_and_streamed_text_is_kept() {
        let stop = "data: {\"candidates\":[{\"finishReason\":\"STOP\"}]}\n\n".to_string();
        assert_eq!(stream_result(stop).await, Ok(String::new()));
        // Text was streamed before a late SAFETY finish: keep the text.
        let late = "data: {\"candidates\":[{\"content\":{\"parts\":[{\"text\":\"par\"}]}}]}\n\n\
                    data: {\"candidates\":[{\"finishReason\":\"SAFETY\"}]}\n\n"
            .to_string();
        assert_eq!(stream_result(late).await, Ok("par".to_string()));
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
