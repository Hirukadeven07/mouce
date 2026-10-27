use crate::store::{AppStore, StoreError};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

const SETTINGS_KEY: &str = "settings";

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[serde(rename_all = "lowercase")]
pub enum ProviderId {
    Ollama,
    OpenAi,
    Groq,
    Gemini,
    Anthropic,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum StarSize {
    Small,
    Medium,
    Large,
}

impl StarSize {
    pub fn px(self) -> u32 {
        match self {
            StarSize::Small => 28,
            StarSize::Medium => 36,
            StarSize::Large => 48,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(default)]
pub struct Settings {
    pub provider: ProviderId,
    pub models: HashMap<ProviderId, String>,
    pub ollama_base_url: String,
    pub star_size: StarSize,
    pub star_rotation: bool,
    pub star_position: Option<(i32, i32)>,
    pub circle_hotkey: String,
    pub history_enabled: bool,
    pub fallback_to_local: bool,
    pub first_run_done: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            provider: ProviderId::Ollama,
            models: HashMap::from([(ProviderId::Ollama, "gemma3:4b".to_string())]),
            ollama_base_url: "http://localhost:11434".to_string(),
            star_size: StarSize::Medium,
            star_rotation: true,
            star_position: None,
            circle_hotkey: "Ctrl+Alt+Space".to_string(),
            history_enabled: false,
            fallback_to_local: true,
            first_run_done: false,
        }
    }
}

/// Missing or unreadable settings fall back to defaults.
pub fn load(store: &AppStore) -> Settings {
    store
        .get_kv(SETTINGS_KEY)
        .ok()
        .flatten()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

pub fn save(store: &AppStore, s: &Settings) -> Result<(), StoreError> {
    let raw = serde_json::to_string(s).map_err(|e| StoreError::Io(e.to_string()))?;
    store.set_kv(SETTINGS_KEY, &raw)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::AppStore;

    fn store() -> (tempfile::TempDir, AppStore) {
        let dir = tempfile::tempdir().unwrap();
        let s = AppStore::open(dir.path().to_path_buf()).unwrap();
        (dir, s)
    }

    #[test]
    fn defaults_match_spec() {
        let d = Settings::default();
        assert_eq!(d.provider, ProviderId::Ollama);
        assert_eq!(d.models.len(), 1);
        assert_eq!(d.models.get(&ProviderId::Ollama).map(String::as_str), Some("gemma3:4b"));
        assert_eq!(d.ollama_base_url, "http://localhost:11434");
        assert_eq!(d.star_size, StarSize::Medium);
        assert!(d.star_rotation);
        assert_eq!(d.star_position, None);
        assert_eq!(d.circle_hotkey, "Ctrl+Alt+Space");
        assert!(!d.history_enabled);
        assert!(d.fallback_to_local);
        assert!(!d.first_run_done);
        assert_eq!(StarSize::Small.px(), 28);
        assert_eq!(StarSize::Medium.px(), 36);
        assert_eq!(StarSize::Large.px(), 48);
        assert_eq!(serde_json::to_string(&ProviderId::OpenAi).unwrap(), "\"openai\"");
    }

    #[test]
    fn load_returns_defaults_when_missing_or_corrupt() {
        let (_d, s) = store();
        assert_eq!(load(&s), Settings::default());
        s.set_kv("settings", "{not json").unwrap();
        assert_eq!(load(&s), Settings::default());
    }

    #[test]
    fn save_then_load_roundtrip() {
        let (_d, s) = store();
        let mut x = Settings::default();
        x.provider = ProviderId::Anthropic;
        x.star_size = StarSize::Small;
        x.star_position = Some((10, 20));
        x.models.insert(ProviderId::OpenAi, "gpt-x".into());
        save(&s, &x).unwrap();
        assert_eq!(load(&s), x);
    }

    #[test]
    fn partial_json_fills_defaults() {
        let (_d, s) = store();
        s.set_kv("settings", r#"{"provider":"groq"}"#).unwrap();
        let mut expected = Settings::default();
        expected.provider = ProviderId::Groq;
        assert_eq!(load(&s), expected);
    }
}
