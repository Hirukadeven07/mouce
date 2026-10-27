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

    /// Whether a key is stored. "Not found" is a quiet `false`; any other credential
    /// store error is reported on stderr (its message only, never a key) and also
    /// reads as `false`.
    pub fn has_key(&self, p: ProviderId) -> bool {
        key_present(self.get_key(p), |msg| eprintln!("could not check the {p:?} key: {msg}"))
    }

    pub fn clear_key(&self, p: ProviderId) -> Result<(), SecretError> {
        match self.entry(p)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}

/// Turns a lookup result into "is a key stored", reporting real errors through `report`.
fn key_present(found: Result<Option<String>, SecretError>, report: impl FnOnce(String)) -> bool {
    match found {
        Ok(key) => key.is_some(),
        Err(e) => {
            report(e.to_string());
            false
        }
    }
}

/// Windows Credential Manager can answer "not found" for a credential that was just
/// written while other threads read or write credentials. Every test that touches the
/// real store holds this lock so such tests never overlap.
#[cfg(test)]
pub(crate) mod test_support {
    use std::sync::{Mutex, MutexGuard};

    static CREDENTIAL_STORE: Mutex<()> = Mutex::new(());

    pub(crate) fn lock_credential_store() -> MutexGuard<'static, ()> {
        CREDENTIAL_STORE.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::lock_credential_store;
    use super::*;
    use crate::settings::ProviderId;

    struct Cleanup(Secrets);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = self.0.clear_key(ProviderId::Groq);
        }
    }

    fn guard(name: &str) -> Cleanup {
        Cleanup(Secrets::new(&format!("com.mouce.app.test.{name}")))
    }

    #[test]
    fn set_key_trims_and_rejects_blank() {
        let _lock = lock_credential_store();
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
        let _lock = lock_credential_store();
        let g = guard("has_and_clear");
        let s = &g.0;
        assert!(!s.has_key(ProviderId::Groq));
        s.set_key(ProviderId::Groq, "k").unwrap();
        assert!(s.has_key(ProviderId::Groq));
        s.clear_key(ProviderId::Groq).unwrap();
        assert!(!s.has_key(ProviderId::Groq));
        s.clear_key(ProviderId::Groq).unwrap();
    }

    #[test]
    fn key_present_is_quiet_for_missing_and_reports_real_errors() {
        let quiet = |m: String| panic!("unexpected report: {m}");
        assert!(key_present(Ok(Some("k".into())), quiet));
        assert!(!key_present(Ok(None), quiet));

        let mut reported = Vec::new();
        let err = SecretError::Keyring("access denied".into());
        assert!(!key_present(Err(err), |m| reported.push(m)));
        assert_eq!(reported, vec!["credential store error: access denied"]);
    }
}
