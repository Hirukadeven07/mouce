use crate::settings::ProviderId;
use keyring::Entry;

#[derive(Debug, thiserror::Error)]
pub enum SecretError {
    #[error("API key is blank")]
    Blank,
    #[error("credential store error: {0}")]
    Keyring(String),
}

impl serde::Serialize for SecretError {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.to_string())
    }
}

impl From<keyring::Error> for SecretError {
    fn from(e: keyring::Error) -> Self {
        SecretError::Keyring(e.to_string())
    }
}

/// API keys live in Windows Credential Manager, never in the app database.
pub struct Secrets {
    service: String,
}

impl Secrets {
    pub fn new(service: &str) -> Secrets {
        Secrets { service: service.to_string() }
    }

    fn entry(&self, p: ProviderId) -> Result<Entry, SecretError> {
        let user = serde_json::to_value(p)
            .ok()
            .and_then(|v| v.as_str().map(str::to_string))
            .ok_or_else(|| SecretError::Keyring("unserializable provider".into()))?;
        Ok(Entry::new(&self.service, &user)?)
    }

    pub fn set_key(&self, p: ProviderId, key: &str) -> Result<(), SecretError> {
        let key = key.trim();
        if key.is_empty() {
            return Err(SecretError::Blank);
        }
        Ok(self.entry(p)?.set_password(key)?)
    }

    pub fn get_key(&self, p: ProviderId) -> Result<Option<String>, SecretError> {
        match self.entry(p)?.get_password() {
            Ok(k) => Ok(Some(k)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    pub fn has_key(&self, p: ProviderId) -> bool {
        matches!(self.get_key(p), Ok(Some(_)))
    }

    pub fn clear_key(&self, p: ProviderId) -> Result<(), SecretError> {
        match self.entry(p)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::ProviderId;

    struct Cleanup(Secrets);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = self.0.clear_key(ProviderId::Groq);
        }
    }

    fn guard(name: &str) -> Cleanup {
        Cleanup(Secrets::new(&format!("com.desktopstar.app.test.{name}")))
    }

    #[test]
    fn set_key_trims_and_rejects_blank() {
        let g = guard("set_key_trims_and_rejects_blank");
        let s = &g.0;
        s.set_key(ProviderId::Groq, "  gsk_abc\n").unwrap();
        assert_eq!(s.get_key(ProviderId::Groq).unwrap().as_deref(), Some("gsk_abc"));
        assert!(matches!(s.set_key(ProviderId::Groq, "   "), Err(SecretError::Blank)));
        assert_eq!(s.get_key(ProviderId::Groq).unwrap().as_deref(), Some("gsk_abc"));
        s.clear_key(ProviderId::Groq).unwrap();
    }

    #[test]
    fn has_and_clear() {
        let g = guard("has_and_clear");
        let s = &g.0;
        assert!(!s.has_key(ProviderId::Groq));
        s.set_key(ProviderId::Groq, "k").unwrap();
        assert!(s.has_key(ProviderId::Groq));
        s.clear_key(ProviderId::Groq).unwrap();
        assert!(!s.has_key(ProviderId::Groq));
        s.clear_key(ProviderId::Groq).unwrap();
    }
}
