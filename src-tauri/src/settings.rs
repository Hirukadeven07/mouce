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
pub enum MouceSize {
    Small,
    Medium,
    Large,
}

impl MouceSize {
    pub fn px(self) -> u32 {
        match self {
            MouceSize::Small => 28,
            MouceSize::Medium => 36,
            MouceSize::Large => 48,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq, Debug)]
#[serde(default)]
pub struct Settings {
    pub provider: ProviderId,
    pub models: HashMap<ProviderId, String>,
    pub ollama_base_url: String,
    pub mouce_size: MouceSize,
    pub mouce_rotation: bool,
    pub mouce_position: Option<(i32, i32)>,
    pub circle_hotkey: String,
    /// Opens the chat from any app; blank turns it off.
    pub chat_hotkey: String,
    pub history_enabled: bool,
    pub fallback_to_local: bool,
    pub first_run_done: bool,
    /// Draws the floating controls as Liquid Glass over a Mica window; off is solid.
    pub liquid_glass: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            provider: ProviderId::Ollama,
            models: HashMap::from([(ProviderId::Ollama, "gemma3:4b".to_string())]),
            ollama_base_url: "http://localhost:11434".to_string(),
            mouce_size: MouceSize::Medium,
            mouce_rotation: true,
            mouce_position: None,
            circle_hotkey: "Ctrl+Alt+Space".to_string(),
            chat_hotkey: crate::hotkey::DEFAULT_CHAT_HOTKEY.to_string(),
            history_enabled: false,
            fallback_to_local: true,
            first_run_done: false,
            liquid_glass: true,
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
        assert_eq!(d.mouce_size, MouceSize::Medium);
        assert!(d.mouce_rotation);
        assert_eq!(d.mouce_position, None);
        assert_eq!(d.circle_hotkey, "Ctrl+Alt+Space");
        assert_eq!(d.chat_hotkey, "Ctrl+Alt+M");
        assert!(!d.history_enabled);
        assert!(d.fallback_to_local);
        assert!(!d.first_run_done);
        assert!(d.liquid_glass);
        assert_eq!(MouceSize::Small.px(), 28);
        assert_eq!(MouceSize::Medium.px(), 36);
        assert_eq!(MouceSize::Large.px(), 48);
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
    fn settings_saved_before_liquid_glass_turn_it_on() {
        let (_d, s) = store();
        s.set_kv(SETTINGS_KEY, r#"{"provider":"groq","first_run_done":true}"#).unwrap();
        let loaded = load(&s);
        assert_eq!(loaded.provider, ProviderId::Groq);
        assert!(loaded.liquid_glass);
    }

    #[test]
    fn save_then_load_roundtrip() {
        let (_d, s) = store();
        let mut x = Settings::default();
        x.provider = ProviderId::Anthropic;
        x.mouce_size = MouceSize::Small;
        x.mouce_position = Some((10, 20));
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
