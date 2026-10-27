//! AI provider types. All HTTP happens in Rust, restricted by `net::check_allowed`.

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

/// Builds the provider for `id`. Implemented in a later task.
pub fn provider_for(
    _id: ProviderId,
    _base_url: Option<String>,
    _api_key: Option<String>,
) -> Result<Box<dyn Provider>, AiError> {
    todo!("provider construction")
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
}
