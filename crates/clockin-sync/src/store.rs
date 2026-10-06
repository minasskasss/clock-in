//! The local SQLCipher database (ARCHITECTURE §5.8): the snapshot cache, the
//! pending-marks queue, marks the server refused and per-device settings.
//!
//! It is encrypted with a random 256-bit key that the platform layer keeps
//! in the OS secret store, so copying the files is useless. The real
//! protection of admin actions is the server-side check.

use crate::api::{MarkRequest, ServerSnapshot};
use clockin_core::MarkKind;
use jiff::Timestamp;
use jiff::civil::Date;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::path::Path;
use uuid::Uuid;

/// Bumped when the local schema changes; see [`migrate`].
const SCHEMA_VERSION: i32 = 2;

/// A queued mark the server refused for good (e.g. its date is outside
/// what the server accepts). Kept until someone on this device has seen the
/// notice, so a refused mark never disappears silently.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefusedMark {
    pub client_id: Uuid,
    pub staff_id: Uuid,
    pub business_date: Date,
    pub kind: MarkKind,
    /// The name when it was refused (empty if this device didn't know it).
    pub first_name: String,
    pub last_name: String,
    pub refused_at: Timestamp,
}

const KEY_SNAPSHOT: &str = "snapshot";
const KEY_LAST_SYNC: &str = "last_sync";

/// The 256-bit database key. `Debug` is redacted.
#[derive(Clone, PartialEq, Eq)]
pub struct StoreKey([u8; 32]);

impl StoreKey {
    /// A fresh key from the OS's cryptographic random generator.
    ///
    /// # Errors
    ///
    /// If the OS generator fails.
    pub fn generate() -> Result<Self, StoreError> {
        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes).map_err(|e| StoreError::Random(e.to_string()))?;
        Ok(Self(bytes))
    }

    #[must_use]
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Parses the 64-character hex form kept in the OS secret store.
    #[must_use]
    pub fn from_hex(hex_key: &str) -> Option<Self> {
        let mut bytes = [0u8; 32];
        hex::decode_to_slice(hex_key.trim(), &mut bytes).ok()?;
        Some(Self(bytes))
    }

    /// The 64-character hex form, for the OS secret store. Never log it.
    #[must_use]
    pub fn expose_hex(&self) -> String {
        hex::encode(self.0)
    }
}

impl fmt::Debug for StoreKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("StoreKey(<redacted>)")
    }
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// The key doesn't open this database (or the file isn't one).
    #[error("the local database could not be opened with this key")]
    WrongKey,
    /// The SQLite build has no encryption (must never happen in a release).
    #[error("the local database is not encrypted (SQLCipher missing)")]
    NotEncrypted,
    #[error("random generator failed: {0}")]
    Random(String),
    #[error("local database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("local data could not be decoded: {0}")]
    Decode(#[from] serde_json::Error),
}

/// The cached server snapshot and when it was fetched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CachedSnapshot {
    pub snapshot: ServerSnapshot,
    pub fetched_at: Timestamp,
}

/// The encrypted local database.
pub struct Store {
    conn: Connection,
}

impl fmt::Debug for Store {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Store").finish_non_exhaustive()
    }
}

impl Store {
    /// Opens (or creates) the database at `path`.
    ///
    /// # Errors
    ///
    /// [`StoreError::WrongKey`] if `key` doesn't open an existing file.
    pub fn open(path: &Path, key: &StoreKey) -> Result<Self, StoreError> {
        Self::init(Connection::open(path)?, key)
    }

    /// An encrypted in-memory database, for tests.
    ///
    /// # Errors
    ///
    /// As [`Store::open`].
    pub fn open_in_memory(key: &StoreKey) -> Result<Self, StoreError> {
        Self::init(Connection::open_in_memory()?, key)
    }

    fn init(conn: Connection, key: &StoreKey) -> Result<Self, StoreError> {
        // A raw 256-bit key: SQLCipher skips its key derivation.
        conn.execute_batch(&format!("PRAGMA key = \"x'{}'\";", key.expose_hex()))?;
        let cipher: Option<String> = conn
            .query_row("PRAGMA cipher_version", [], |row| row.get(0))
            .optional()?;
        if cipher.is_none_or(|v| v.is_empty()) {
            return Err(StoreError::NotEncrypted);
        }
        // The first read fails with "file is not a database" on a wrong key.
        conn.query_row("SELECT count(*) FROM sqlite_master", [], |row| {
            row.get::<_, i64>(0)
        })
        .map_err(|_| StoreError::WrongKey)?;
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")?;
        migrate(&conn)?;
        Ok(Self { conn })
    }

    // --- Snapshot cache -------------------------------------------------

    /// The last snapshot fetched from the server, if any.
    ///
    /// # Errors
    ///
    /// On a database or decoding error.
    pub fn snapshot(&self) -> Result<Option<CachedSnapshot>, StoreError> {
        let row: Option<(String, String)> = self
            .conn
            .query_row(
                "SELECT value, updated_at FROM kv WHERE key = ?1",
                [KEY_SNAPSHOT],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((json, fetched_at)) = row else {
            return Ok(None);
        };
        Ok(Some(CachedSnapshot {
            snapshot: serde_json::from_str(&json)?,
            fetched_at: parse_timestamp(&fetched_at)?,
        }))
    }

    /// # Errors
    ///
    /// On a database error.
    pub fn save_snapshot(
        &self,
        snapshot: &ServerSnapshot,
        fetched_at: Timestamp,
    ) -> Result<(), StoreError> {
        self.put(KEY_SNAPSHOT, &serde_json::to_string(snapshot)?, fetched_at)
    }

    /// When this device last talked to the server successfully.
    ///
    /// # Errors
    ///
    /// On a database error.
    pub fn last_sync(&self) -> Result<Option<Timestamp>, StoreError> {
        self.get(KEY_LAST_SYNC)?
            .map(|s| parse_timestamp(&s))
            .transpose()
    }

    /// # Errors
    ///
    /// On a database error.
    pub fn set_last_sync(&self, at: Timestamp) -> Result<(), StoreError> {
        self.put(KEY_LAST_SYNC, &at.to_string(), at)
    }

    // --- Pending marks --------------------------------------------------

    /// Queues a mark until the server confirms it. Queuing the same
    /// `client_id` twice keeps one entry.
    ///
    /// # Errors
    ///
    /// On a database error.
    pub fn queue_mark(&self, mark: &MarkRequest, queued_at: Timestamp) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT OR IGNORE INTO pending_marks (client_id, payload, queued_at)
             VALUES (?1, ?2, ?3)",
            params![
                mark.client_id.to_string(),
                serde_json::to_string(mark)?,
                queued_at.to_string()
            ],
        )?;
        Ok(())
    }

    /// Queued marks, oldest first.
    ///
    /// # Errors
    ///
    /// On a database or decoding error.
    pub fn pending_marks(&self) -> Result<Vec<MarkRequest>, StoreError> {
        let mut stmt = self
            .conn
            .prepare("SELECT payload FROM pending_marks ORDER BY seq")?;
        let payloads = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        payloads
            .iter()
            .map(|p| serde_json::from_str(p).map_err(StoreError::from))
            .collect()
    }

    /// Removes a mark from the queue once the server has it.
    ///
    /// # Errors
    ///
    /// On a database error.
    pub fn remove_pending_mark(&self, client_id: Uuid) -> Result<(), StoreError> {
        self.conn.execute(
            "DELETE FROM pending_marks WHERE client_id = ?1",
            [client_id.to_string()],
        )?;
        Ok(())
    }

    // --- Refused marks --------------------------------------------------

    /// Moves a queued mark the server refused out of the queue and into the
    /// refused list, in one transaction.
    ///
    /// # Errors
    ///
    /// On a database error.
    pub fn refuse_pending_mark(&self, refused: &RefusedMark) -> Result<(), StoreError> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "INSERT OR IGNORE INTO refused_marks (client_id, payload) VALUES (?1, ?2)",
            params![
                refused.client_id.to_string(),
                serde_json::to_string(refused)?
            ],
        )?;
        tx.execute(
            "DELETE FROM pending_marks WHERE client_id = ?1",
            [refused.client_id.to_string()],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// Refused marks nobody has dismissed yet, oldest first.
    ///
    /// # Errors
    ///
    /// On a database or decoding error.
    pub fn refused_marks(&self) -> Result<Vec<RefusedMark>, StoreError> {
        let mut stmt = self
            .conn
            .prepare("SELECT payload FROM refused_marks ORDER BY seq")?;
        let payloads = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        payloads
            .iter()
            .map(|p| serde_json::from_str(p).map_err(StoreError::from))
            .collect()
    }

    /// The notice for this refused mark was read («Εντάξει»).
    ///
    /// # Errors
    ///
    /// On a database error.
    pub fn dismiss_refused_mark(&self, client_id: Uuid) -> Result<(), StoreError> {
        self.conn.execute(
            "DELETE FROM refused_marks WHERE client_id = ?1",
            [client_id.to_string()],
        )?;
        Ok(())
    }

    // --- Per-device settings (alert mode, language, theme, ...) ---------

    /// # Errors
    ///
    /// On a database error.
    pub fn device_setting(&self, name: &str) -> Result<Option<String>, StoreError> {
        self.get(&format!("device.{name}"))
    }

    /// # Errors
    ///
    /// On a database error.
    pub fn set_device_setting(
        &self,
        name: &str,
        value: &str,
        now: Timestamp,
    ) -> Result<(), StoreError> {
        self.put(&format!("device.{name}"), value, now)
    }

    /// Forgets everything that belongs to the server: snapshot, last sync,
    /// queued and refused marks (after unpairing). Device settings stay.
    ///
    /// # Errors
    ///
    /// On a database error.
    pub fn clear_server_data(&self) -> Result<(), StoreError> {
        self.conn.execute_batch(&format!(
            "DELETE FROM kv WHERE key IN ('{KEY_SNAPSHOT}', '{KEY_LAST_SYNC}');
             DELETE FROM pending_marks;
             DELETE FROM refused_marks;"
        ))?;
        Ok(())
    }

    // --- kv helpers -----------------------------------------------------

    fn get(&self, key: &str) -> Result<Option<String>, StoreError> {
        Ok(self
            .conn
            .query_row("SELECT value FROM kv WHERE key = ?1", [key], |row| {
                row.get(0)
            })
            .optional()?)
    }

    fn put(&self, key: &str, value: &str, at: Timestamp) -> Result<(), StoreError> {
        self.conn.execute(
            "INSERT INTO kv (key, value, updated_at) VALUES (?1, ?2, ?3)
             ON CONFLICT (key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
            params![key, value, at.to_string()],
        )?;
        Ok(())
    }
}

fn parse_timestamp(s: &str) -> Result<Timestamp, StoreError> {
    s.parse()
        .map_err(|e: jiff::Error| StoreError::Decode(serde::de::Error::custom(e.to_string())))
}

fn migrate(conn: &Connection) -> Result<(), StoreError> {
    let version: i32 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if version < 1 {
        conn.execute_batch(
            "BEGIN;
             CREATE TABLE kv (
                 key        TEXT PRIMARY KEY,
                 value      TEXT NOT NULL,
                 updated_at TEXT NOT NULL
             );
             CREATE TABLE pending_marks (
                 seq        INTEGER PRIMARY KEY AUTOINCREMENT,
                 client_id  TEXT NOT NULL UNIQUE,
                 payload    TEXT NOT NULL,
                 queued_at  TEXT NOT NULL
             );
             PRAGMA user_version = 1;
             COMMIT;",
        )?;
    }
    if version < 2 {
        conn.execute_batch(
            "BEGIN;
             CREATE TABLE refused_marks (
                 seq        INTEGER PRIMARY KEY AUTOINCREMENT,
                 client_id  TEXT NOT NULL UNIQUE,
                 payload    TEXT NOT NULL
             );
             PRAGMA user_version = 2;
             COMMIT;",
        )?;
    }
    debug_assert_eq!(
        conn.query_row("PRAGMA user_version", [], |row| row.get::<_, i32>(0))?,
        SCHEMA_VERSION
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{ServerSettings, ServerSnapshot};
    use clockin_core::MarkKind;
    use jiff::civil::{Time, date};

    fn ts(s: &str) -> Timestamp {
        s.parse().unwrap()
    }

    fn snapshot() -> ServerSnapshot {
        ServerSnapshot {
            staff: vec![],
            blocks: vec![],
            overrides: vec![],
            settings: ServerSettings {
                checkin_offset_min: 0,
                checkout_offset_min: 0,
                rollover: Time::constant(5, 0, 0, 0),
                autostart: true,
                quit_code: None,
            },
            marks: vec![],
            devices: vec![],
            this_device_id: Uuid::from_u128(1),
            config_version: 3,
            data_version: 8,
            plan_config_version: 3,
            plan_horizon_end: None,
            server_now: ts("2026-10-05T10:00:00Z"),
        }
    }

    fn mark(n: u128) -> MarkRequest {
        MarkRequest {
            client_id: Uuid::from_u128(n),
            staff_id: Uuid::from_u128(100),
            source_block_id: Uuid::from_u128(200),
            business_date: date(2026, 10, 5),
            kind: MarkKind::In,
        }
    }

    #[test]
    fn key_hex_round_trip_and_redacted_debug() {
        let key = StoreKey::generate().unwrap();
        assert_ne!(key, StoreKey::generate().unwrap());
        assert_eq!(StoreKey::from_hex(&key.expose_hex()), Some(key.clone()));
        assert_eq!(StoreKey::from_hex("abc"), None);
        assert!(!format!("{key:?}").contains(&key.expose_hex()));
    }

    #[test]
    fn database_is_encrypted_and_needs_the_key() {
        let dir = std::env::temp_dir().join(format!("clockin-store-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("local.db");
        let key = StoreKey::generate().unwrap();
        {
            let store = Store::open(&path, &key).unwrap();
            store
                .save_snapshot(&snapshot(), ts("2026-10-05T10:00:00Z"))
                .unwrap();
        }
        // The file holds no plain-text SQLite header or data.
        let bytes = std::fs::read(&path).unwrap();
        assert!(!bytes.starts_with(b"SQLite format 3"));
        assert!(!bytes.windows(8).any(|w| w == b"snapshot"));

        let wrong = StoreKey::generate().unwrap();
        assert!(matches!(
            Store::open(&path, &wrong),
            Err(StoreError::WrongKey)
        ));

        let store = Store::open(&path, &key).unwrap();
        assert_eq!(store.snapshot().unwrap().unwrap().snapshot, snapshot());
        drop(store);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn snapshot_and_last_sync_round_trip() {
        let store = Store::open_in_memory(&StoreKey::generate().unwrap()).unwrap();
        assert_eq!(store.snapshot().unwrap(), None);
        assert_eq!(store.last_sync().unwrap(), None);

        let at = ts("2026-10-05T10:00:05Z");
        store.save_snapshot(&snapshot(), at).unwrap();
        store.set_last_sync(at).unwrap();
        let cached = store.snapshot().unwrap().unwrap();
        assert_eq!(cached.snapshot, snapshot());
        assert_eq!(cached.fetched_at, at);
        assert_eq!(store.last_sync().unwrap(), Some(at));

        let mut newer = snapshot();
        newer.data_version = 9;
        store.save_snapshot(&newer, at).unwrap();
        assert_eq!(store.snapshot().unwrap().unwrap().snapshot.data_version, 9);
    }

    #[test]
    fn pending_marks_queue_in_order_without_duplicates() {
        let store = Store::open_in_memory(&StoreKey::generate().unwrap()).unwrap();
        let at = ts("2026-10-05T10:00:00Z");
        store.queue_mark(&mark(3), at).unwrap();
        store.queue_mark(&mark(1), at).unwrap();
        store.queue_mark(&mark(3), at).unwrap();
        store.queue_mark(&mark(2), at).unwrap();
        let ids: Vec<_> = store
            .pending_marks()
            .unwrap()
            .iter()
            .map(|m| m.client_id)
            .collect();
        assert_eq!(
            ids,
            [Uuid::from_u128(3), Uuid::from_u128(1), Uuid::from_u128(2)]
        );

        store.remove_pending_mark(Uuid::from_u128(1)).unwrap();
        assert_eq!(store.pending_marks().unwrap().len(), 2);
        assert_eq!(store.pending_marks().unwrap()[0], mark(3));
    }

    fn refused(n: u128) -> RefusedMark {
        RefusedMark {
            client_id: Uuid::from_u128(n),
            staff_id: Uuid::from_u128(100),
            business_date: date(2026, 10, 24),
            kind: MarkKind::In,
            first_name: "Μαρία".into(),
            last_name: "Παππά".into(),
            refused_at: ts("2026-10-24T19:01:00Z"),
        }
    }

    #[test]
    fn a_refused_mark_leaves_the_queue_and_stays_until_dismissed() {
        let store = Store::open_in_memory(&StoreKey::generate().unwrap()).unwrap();
        let at = ts("2026-10-05T10:00:00Z");
        store.queue_mark(&mark(1), at).unwrap();
        store.queue_mark(&mark(2), at).unwrap();

        store.refuse_pending_mark(&refused(1)).unwrap();
        assert_eq!(store.pending_marks().unwrap(), [mark(2)]);
        assert_eq!(store.refused_marks().unwrap(), [refused(1)]);
        // Refusing it again keeps one notice.
        store.refuse_pending_mark(&refused(1)).unwrap();
        assert_eq!(store.refused_marks().unwrap().len(), 1);

        store.dismiss_refused_mark(Uuid::from_u128(1)).unwrap();
        assert!(store.refused_marks().unwrap().is_empty());

        store.refuse_pending_mark(&refused(2)).unwrap();
        store.clear_server_data().unwrap();
        assert!(store.refused_marks().unwrap().is_empty());
    }

    #[test]
    fn a_version_1_database_gains_the_refused_list() {
        let dir = std::env::temp_dir().join(format!("clockin-store-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("local.db");
        let key = StoreKey::generate().unwrap();
        {
            // What Phase 2–4 builds created.
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(&format!("PRAGMA key = \"x'{}'\";", key.expose_hex()))
                .unwrap();
            conn.execute_batch(
                "CREATE TABLE kv (key TEXT PRIMARY KEY, value TEXT NOT NULL, updated_at TEXT NOT NULL);
                 CREATE TABLE pending_marks (seq INTEGER PRIMARY KEY AUTOINCREMENT,
                     client_id TEXT NOT NULL UNIQUE, payload TEXT NOT NULL, queued_at TEXT NOT NULL);
                 PRAGMA user_version = 1;",
            )
            .unwrap();
        }
        let store = Store::open(&path, &key).unwrap();
        store
            .queue_mark(&mark(1), ts("2026-10-05T10:00:00Z"))
            .unwrap();
        store.refuse_pending_mark(&refused(1)).unwrap();
        assert_eq!(store.refused_marks().unwrap().len(), 1);
        drop(store);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn device_settings_survive_clearing_server_data() {
        let store = Store::open_in_memory(&StoreKey::generate().unwrap()).unwrap();
        let at = ts("2026-10-05T10:00:00Z");
        store.set_device_setting("alert_mode", "ring", at).unwrap();
        store.save_snapshot(&snapshot(), at).unwrap();
        store.set_last_sync(at).unwrap();
        store.queue_mark(&mark(1), at).unwrap();

        store.clear_server_data().unwrap();
        assert_eq!(store.snapshot().unwrap(), None);
        assert_eq!(store.last_sync().unwrap(), None);
        assert!(store.pending_marks().unwrap().is_empty());
        assert_eq!(
            store.device_setting("alert_mode").unwrap().as_deref(),
            Some("ring")
        );
        assert_eq!(store.device_setting("missing").unwrap(), None);
    }
}
