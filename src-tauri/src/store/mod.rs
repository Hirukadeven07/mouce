//! The only module that writes to disk. Everything lives under the app data
//! directory; paths are guarded so nothing outside it can be touched.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use rusqlite::{params, Connection, OptionalExtension};

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("path is outside the app data directory: {}", .0.display())]
    OutsideAppDir(PathBuf),
    #[error("io error: {0}")]
    Io(String),
    #[error("database error: {0}")]
    Db(String),
}

impl serde::Serialize for StoreError {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl From<rusqlite::Error> for StoreError {
    fn from(e: rusqlite::Error) -> Self {
        StoreError::Db(e.to_string())
    }
}

impl From<std::io::Error> for StoreError {
    fn from(e: std::io::Error) -> Self {
        StoreError::Io(e.to_string())
    }
}

/// `root` must already be canonical.
fn guard_in(root: &Path, p: &Path) -> Result<PathBuf, StoreError> {
    let outside = || StoreError::OutsideAppDir(p.to_path_buf());
    let parent = p.parent().ok_or_else(outside)?;
    let name = p.file_name().ok_or_else(outside)?;
    let parent = parent.canonicalize().map_err(|_| outside())?;
    let candidate = parent.join(name);
    // If the target already exists, resolve it too so a link inside the
    // app dir cannot point out of it.
    let resolved = if candidate.exists() {
        candidate.canonicalize().map_err(|_| outside())?
    } else {
        candidate
    };
    if resolved.starts_with(root) {
        Ok(resolved)
    } else {
        Err(outside())
    }
}

pub struct AppStore {
    root: PathBuf,
    conn: Mutex<Connection>,
}

impl AppStore {
    /// Creates `root` if needed, then opens `root/star.db` with the `kv` table.
    pub fn open(root: PathBuf) -> Result<AppStore, StoreError> {
        std::fs::create_dir_all(&root)?;
        let root = root.canonicalize()?;
        let db_path = guard_in(&root, &root.join("star.db"))?;
        let conn = Connection::open(db_path)?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS kv (key TEXT PRIMARY KEY, value TEXT NOT NULL)",
            [],
        )?;
        Ok(AppStore {
            root,
            conn: Mutex::new(conn),
        })
    }

    /// Returns the canonical form of `p` only if it is inside the app dir.
    /// `p` need not exist, but its parent directory must.
    pub fn guard_path(&self, p: &Path) -> Result<PathBuf, StoreError> {
        guard_in(&self.root, p)
    }

    pub fn get_kv(&self, key: &str) -> Result<Option<String>, StoreError> {
        Ok(self
            .connection()
            .query_row("SELECT value FROM kv WHERE key = ?1", params![key], |r| {
                r.get(0)
            })
            .optional()?)
    }

    pub fn set_kv(&self, key: &str, value: &str) -> Result<(), StoreError> {
        self.connection().execute(
            "INSERT INTO kv (key, value) VALUES (?1, ?2) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    /// Shared connection for later milestones' tables.
    pub fn connection(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn kv_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let store = AppStore::open(dir.path().to_path_buf()).unwrap();
        store.set_kv("a", "1").unwrap();
        assert_eq!(store.get_kv("a").unwrap(), Some("1".to_string()));
        store.set_kv("a", "2").unwrap();
        assert_eq!(store.get_kv("a").unwrap(), Some("2".to_string()));
        assert_eq!(store.get_kv("missing").unwrap(), None);
    }

    #[test]
    fn guard_accepts_inside_root() {
        let dir = tempfile::tempdir().unwrap();
        let store = AppStore::open(dir.path().to_path_buf()).unwrap();
        assert!(store.guard_path(&dir.path().join("star.db")).is_ok());
        assert!(store.guard_path(&dir.path().join("not-yet-created.txt")).is_ok());
    }

    #[test]
    fn guard_rejects_outside_and_traversal() {
        let dir = tempfile::tempdir().unwrap();
        let store = AppStore::open(dir.path().to_path_buf()).unwrap();
        let traversal = dir.path().join("..").join("evil.txt");
        assert!(matches!(
            store.guard_path(&traversal),
            Err(StoreError::OutsideAppDir(_))
        ));
        assert!(matches!(
            store.guard_path(Path::new(r"C:\Windows\win.ini")),
            Err(StoreError::OutsideAppDir(_))
        ));
        // Parent directory does not exist: rejected rather than created.
        assert!(matches!(
            store.guard_path(&dir.path().join("no-such-dir").join("x.txt")),
            Err(StoreError::OutsideAppDir(_))
        ));
    }

    #[test]
    fn reopen_persists() {
        let dir = tempfile::tempdir().unwrap();
        let store = AppStore::open(dir.path().to_path_buf()).unwrap();
        store.set_kv("k", "v").unwrap();
        drop(store);
        let again = AppStore::open(dir.path().to_path_buf()).unwrap();
        assert_eq!(again.get_kv("k").unwrap(), Some("v".to_string()));
    }

    #[test]
    fn error_serializes_as_display_string() {
        let e = StoreError::Db("boom".into());
        assert_eq!(serde_json::to_string(&e).unwrap(), format!("{:?}", e.to_string()));
    }
}
