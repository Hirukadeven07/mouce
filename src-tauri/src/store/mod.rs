//! The only module that writes to disk. Everything lives under the app data
//! directory; paths are guarded so nothing outside it can be touched.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use rusqlite::hooks::{AuthAction, AuthContext, Authorization};
use rusqlite::limits::Limit;
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
    // If anything exists at the target (including a dangling symlink, which
    // `exists()` would miss), resolve it too so a link inside the app dir
    // cannot point out of it.
    let resolved = if std::fs::symlink_metadata(&candidate).is_ok() {
        candidate.canonicalize().map_err(|_| outside())?
    } else {
        candidate
    };
    // The root itself is a directory, never a valid file target.
    if resolved != root && resolved.starts_with(root) {
        Ok(resolved)
    } else {
        Err(outside())
    }
}

/// Stops SQL run through `AppStore::connection()` from reaching files outside
/// the app dir: no attached databases (limit) and ATTACH/DETACH denied (authorizer).
fn harden(conn: &Connection) {
    conn.set_limit(Limit::SQLITE_LIMIT_ATTACHED, 0);
    conn.authorizer(Some(|ctx: AuthContext<'_>| match ctx.action {
        AuthAction::Attach { .. } | AuthAction::Detach { .. } => Authorization::Deny,
        _ => Authorization::Allow,
    }));
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
        harden(&conn);
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
    fn guard_rejects_root_itself() {
        let dir = tempfile::tempdir().unwrap();
        let store = AppStore::open(dir.path().to_path_buf()).unwrap();
        assert!(matches!(
            store.guard_path(dir.path()),
            Err(StoreError::OutsideAppDir(_))
        ));
        assert!(matches!(
            store.guard_path(&dir.path().join(".")),
            Err(StoreError::OutsideAppDir(_))
        ));
    }

    #[test]
    fn guard_rejects_dangling_symlink_escape() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let store = AppStore::open(dir.path().to_path_buf()).unwrap();
        let link = dir.path().join("link.txt");
        let target = outside.path().join("not-created.txt");
        let made = std::os::windows::fs::symlink_file(&target, &link).is_ok();
        if !made {
            eprintln!("skipping: cannot create symlinks without privileges");
            return;
        }
        assert!(!target.exists());
        assert!(matches!(
            store.guard_path(&link),
            Err(StoreError::OutsideAppDir(_))
        ));
    }

    #[test]
    fn connection_cannot_attach_or_vacuum_outside_root() {
        let dir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let store = AppStore::open(dir.path().to_path_buf()).unwrap();
        let x = outside.path().join("x.db");
        let y = outside.path().join("y.db");
        let conn = store.connection();
        let attach = format!("ATTACH DATABASE '{}' AS x", x.display());
        assert!(conn.execute_batch(&attach).is_err());
        let vacuum = format!("VACUUM INTO '{}'", y.display());
        assert!(conn.execute_batch(&vacuum).is_err());
        drop(conn);
        assert!(!x.exists(), "ATTACH created a file outside the app dir");
        assert!(!y.exists(), "VACUUM INTO created a file outside the app dir");
        store.set_kv("still", "works").unwrap();
        assert_eq!(store.get_kv("still").unwrap(), Some("works".to_string()));
    }

    #[test]
    fn attach_blocked_by_each_mechanism_alone() {
        let outside = tempfile::tempdir().unwrap();
        let x = outside.path().join("x.db");
        let attach = format!("ATTACH DATABASE '{}' AS x", x.display());

        let limit_only = Connection::open_in_memory().unwrap();
        limit_only.set_limit(Limit::SQLITE_LIMIT_ATTACHED, 0);
        assert!(limit_only.execute_batch(&attach).is_err());
        let auth_only = Connection::open_in_memory().unwrap();
        harden(&auth_only);
        auth_only.set_limit(Limit::SQLITE_LIMIT_ATTACHED, 10);
        assert!(auth_only.execute_batch(&attach).is_err());
        assert!(!x.exists());

        let y = outside.path().join("y.db");
        let vacuum = format!("VACUUM INTO '{}'", y.display());
        let limit_only = Connection::open_in_memory().unwrap();
        limit_only.set_limit(Limit::SQLITE_LIMIT_ATTACHED, 0);
        assert!(limit_only.execute_batch(&vacuum).is_err());
        let auth_only = Connection::open_in_memory().unwrap();
        harden(&auth_only);
        auth_only.set_limit(Limit::SQLITE_LIMIT_ATTACHED, 10);
        assert!(auth_only.execute_batch(&vacuum).is_err());
        assert!(!y.exists());
    }

    #[test]
    fn error_serializes_as_display_string() {
        let e = StoreError::Db("boom".into());
        assert_eq!(serde_json::to_string(&e).unwrap(), format!("{:?}", e.to_string()));
    }
}
