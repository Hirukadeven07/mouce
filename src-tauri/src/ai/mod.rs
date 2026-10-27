//! AI provider types. All HTTP happens in Rust, restricted by `net::check_allowed`.

pub mod anthropic;
pub mod gemini;
pub mod net;
pub mod ollama;
pub mod openai_compat;

use crate::settings::ProviderId;
use async_trait::async_trait;
use serde::{Serialize, Serializer};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Role {
    User,
    Assistant,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ImageInput {
    pub mime: String,
    pub base64: String,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ChatMessage {
    pub role: Role,
    pub text: String,
    pub images: Vec<ImageInput>,
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct ChatRequest {
    pub model: String,
    pub system: Option<String>,
    pub messages: Vec<ChatMessage>,
    pub max_tokens: u32,
}

#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
pub enum AiError {
    #[error("Host not allowed: {0}")]
    NotAllowed(String),
    #[error("Missing API key")]
    MissingKey,
    #[error("Network error: {0}")]
    Network(String),
    #[error("HTTP {status}: {message}")]
    Http { status: u16, message: String },
    #[error("Could not parse response: {0}")]
    Parse(String),
}

impl Serialize for AiError {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

#[async_trait]
pub trait Provider: Send + Sync {
    /// Streams the reply, calling `on_delta` per text chunk; returns the full text.
    async fn chat_stream(
        &self,
        req: &ChatRequest,
        on_delta: &mut (dyn for<'a> FnMut(&'a str) + Send),
    ) -> Result<String, AiError>;

    async fn list_models(&self) -> Result<Vec<String>, AiError>;
}

const OLLAMA_DEFAULT_BASE: &str = "http://localhost:11434";

/// Builds the provider for `id`. `base_url` overrides the provider's default
/// (Ollama's comes from settings); every request is still checked against the
/// host allowlist. Providers other than Ollama need a non-blank `api_key`.
pub fn provider_for(
    id: ProviderId,
    base_url: Option<String>,
    api_key: Option<String>,
) -> Result<Box<dyn Provider>, AiError> {
    let key = api_key.map(|k| k.trim().to_string()).filter(|k| !k.is_empty());
    let base = |default: &str| base_url.unwrap_or_else(|| default.to_string());
    Ok(match id {
        ProviderId::Ollama => Box::new(ollama::OllamaProvider::new(base(OLLAMA_DEFAULT_BASE))),
        ProviderId::OpenAi => Box::new(openai_compat::OpenAiCompat::new(
            base(openai_compat::OPENAI_BASE),
            key.ok_or(AiError::MissingKey)?,
        )),
        ProviderId::Groq => Box::new(openai_compat::OpenAiCompat::new(
            base(openai_compat::GROQ_BASE),
            key.ok_or(AiError::MissingKey)?,
        )),
        ProviderId::Gemini => Box::new(gemini::GeminiProvider::new(
            base(gemini::GEMINI_BASE),
            key.ok_or(AiError::MissingKey)?,
        )),
        ProviderId::Anthropic => Box::new(anthropic::AnthropicProvider::new(
            base(anthropic::ANTHROPIC_BASE),
            key.ok_or(AiError::MissingKey)?,
        )),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_serializes_as_display_string() {
        let e = AiError::Http { status: 401, message: "bad key".into() };
        assert_eq!(serde_json::to_string(&e).unwrap(), "\"HTTP 401: bad key\"");
        assert_eq!(e.to_string(), "HTTP 401: bad key");
    }

    #[test]
    fn provider_for_requires_key_except_ollama() {
        for id in [ProviderId::OpenAi, ProviderId::Groq, ProviderId::Gemini, ProviderId::Anthropic]
        {
            assert_eq!(provider_for(id, None, None).err(), Some(AiError::MissingKey), "{id:?}");
            assert_eq!(
                provider_for(id, None, Some("  ".into())).err(),
                Some(AiError::MissingKey),
                "{id:?} blank"
            );
            assert!(provider_for(id, None, Some("key".into())).is_ok(), "{id:?}");
        }
        assert!(
            provider_for(ProviderId::Ollama, Some("http://localhost:11434".into()), None).is_ok()
        );
        assert!(provider_for(ProviderId::Ollama, None, None).is_ok());
    }

    #[tokio::test]
    async fn provider_for_rejects_disallowed_override() {
        for id in
            [ProviderId::Ollama, ProviderId::OpenAi, ProviderId::Gemini, ProviderId::Anthropic]
        {
            let provider =
                provider_for(id, Some("https://evil.com".into()), Some("key".into())).unwrap();
            let err = provider.list_models().await.unwrap_err();
            assert!(matches!(err, AiError::NotAllowed(_)), "{id:?}: {err:?}");
        }
    }

    #[tokio::test]
    async fn provider_for_uses_override_base_url() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(ResponseTemplate::new(200).set_body_string(r#"{"data":[{"id":"m"}]}"#))
            .mount(&server)
            .await;
        let provider =
            provider_for(ProviderId::Anthropic, Some(server.uri()), Some("key".into())).unwrap();
        assert_eq!(provider.list_models().await.unwrap(), vec!["m"]);
    }
}
