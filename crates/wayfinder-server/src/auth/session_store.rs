//! Session persistence against our own sqlx SQLite pool.
//!
//! `tower-sessions-sqlx-store` 0.15.1 still targets `sqlx ^0.8` plus
//! `tower-sessions-core ^0.15`, which axum-login 0.18 pins away (plan
//! §2.1). Until upstream re-publication, this custom store implements the
//! 0.14 `SessionStore` trait directly. It deliberately mirrors the official
//! crate's schema (`id`/`data`/`expiry` columns, MessagePack blob) so a
//! later swap costs as little as possible.

use async_trait::async_trait;
use sqlx::{Row, SqlitePool};
use time::OffsetDateTime;
use tower_sessions::session::Id;
use tower_sessions::session_store::{Error as SessionError, Result as SessionResult};
use tower_sessions::{SessionStore, session};

/// Store implementing `tower_sessions::SessionStore` over the shared pool.
/// Cheap to clone; the auth layer owns one.
#[derive(Debug, Clone)]
pub struct SqlxSessionStore(SqlitePool);

impl SqlxSessionStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self(pool)
    }

    /// Best-effort removal of expired rows; the startup path already purges
    /// in `db.rs`.
    pub async fn purge_expired(&self) -> SessionResult<u64> {
        let result = sqlx::query(
            "DELETE FROM sessions WHERE expiry < CAST(strftime('%s','now') AS INTEGER)",
        )
        .execute(&self.0)
        .await
        .map_err(backend)?;
        Ok(result.rows_affected())
    }

    async fn expiry_of(&self, session_id: &Id) -> SessionResult<Option<i64>> {
        let row = sqlx::query("SELECT expiry FROM sessions WHERE id = ?")
            .bind(session_id.to_string())
            .fetch_optional(&self.0)
            .await
            .map_err(backend)?;
        Ok(row.map(|row| row.try_get(0).map_err(backend)).transpose()?)
    }
}

fn backend(message: impl std::fmt::Display) -> SessionError {
    SessionError::Backend(message.to_string())
}

fn decode(message: impl std::fmt::Display) -> SessionError {
    SessionError::Decode(message.to_string())
}

#[async_trait]
impl SessionStore for SqlxSessionStore {
    /// Insert a fresh record with a retry loop for the (astronomically
    /// unlikely) random session-ID collision, mirroring the official store.
    async fn create(&self, session_record: &mut session::Record) -> SessionResult<()> {
        const MAX_ID_RETRIES: usize = 5;

        let mut attempts = 0;
        loop {
            let expiry = session_record.expiry_date.unix_timestamp();
            let result = sqlx::query("INSERT INTO sessions (id, data, expiry) VALUES (?, ?, ?)")
                .bind(session_record.id.to_string())
                .bind(rmp_serde::to_vec(&session_record).map_err(decode)?)
                .bind(expiry)
                .execute(&self.0)
                .await;

            match result {
                Ok(_) => return Ok(()),
                Err(error)
                    if error
                        .as_database_error()
                        .is_some_and(|error| error.is_unique_violation())
                        && attempts < MAX_ID_RETRIES =>
                {
                    attempts += 1;
                    session_record.id = Id::default();
                }
                Err(error) => return Err(backend(error)),
            }
        }
    }

    /// Upsert an existing record (`INSERT OR REPLACE` for SQLite), exactly
    /// like the official store.
    async fn save(&self, session_record: &session::Record) -> SessionResult<()> {
        let expiry = session_record.expiry_date.unix_timestamp();
        sqlx::query("INSERT OR REPLACE INTO sessions (id, data, expiry) VALUES (?, ?, ?)")
            .bind(session_record.id.to_string())
            .bind(rmp_serde::to_vec(&session_record).map_err(decode)?)
            .bind(expiry)
            .execute(&self.0)
            .await
            .map_err(backend)?;
        Ok(())
    }

    /// Load a live (non-expired) record. Expired rows are filtered out and
    /// left for the periodic/purge query to delete.
    async fn load(&self, session_id: &Id) -> SessionResult<Option<session::Record>> {
        let Some(expiry) = self.expiry_of(session_id).await? else {
            return Ok(None);
        };

        let now = OffsetDateTime::now_utc().unix_timestamp();
        if expiry <= now {
            return Ok(None);
        }

        let row = sqlx::query("SELECT data, expiry FROM sessions WHERE id = ? AND expiry > ?")
            .bind(session_id.to_string())
            .bind(now)
            .fetch_optional(&self.0)
            .await
            .map_err(backend)?;

        let Some(row) = row else {
            return Ok(None);
        };

        let data: Vec<u8> = row.try_get(0).map_err(backend)?;
        let record = rmp_serde::from_slice(&data).map_err(decode)?;
        Ok(Some(record))
    }

    async fn delete(&self, session_id: &Id) -> SessionResult<()> {
        sqlx::query("DELETE FROM sessions WHERE id = ?")
            .bind(session_id.to_string())
            .execute(&self.0)
            .await
            .map_err(backend)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::temp_db;
    use serde_json::json;
    use time::Duration;

    async fn store() -> (tempfile::TempDir, SqlxSessionStore) {
        let (dir, pool) = temp_db().await;
        (dir, SqlxSessionStore::new(pool))
    }

    fn record(days: i64, value: &str) -> session::Record {
        let mut record = session::Record {
            id: Id::default(),
            data: Default::default(),
            expiry_date: OffsetDateTime::now_utc() + Duration::days(days),
        };
        record.data.insert(value.to_owned(), json!(value));
        record
    }

    #[tokio::test]
    async fn create_then_load_round_trips_the_record() {
        let (dir, store) = store().await;

        let mut created = record(1, "identity");
        store.create(&mut created).await.unwrap();

        let loaded = store.load(&created.id).await.unwrap().unwrap();
        assert_eq!(loaded, created); // Record derives PartialEq

        drop(store);
        drop(dir);
    }

    #[tokio::test]
    async fn save_uploads_an_existing_record() {
        let (dir, store) = store().await;

        let mut created = record(1, "one");
        store.create(&mut created).await.unwrap();
        created.data.insert("one".into(), json!("two"));
        store.save(&created).await.unwrap();

        let loaded = store.load(&created.id).await.unwrap().unwrap();
        assert_eq!(loaded.data["one"], json!("two"));
        assert_eq!(loaded, created);

        // Saving a brand-new record through `save` also stores it (upsert).
        let fresh = record(2, "fresh");
        store.save(&fresh).await.unwrap();
        assert!(store.load(&fresh.id).await.unwrap().is_some());

        drop(store);
        drop(dir);
    }

    #[tokio::test]
    async fn create_regenerates_the_id_on_unique_violation() {
        let (dir, store) = store().await;

        // Insert a row, then create a record with the same id: the store
        // must retry with a fresh id instead of failing.
        let existing = record(1, "collision");
        let mut duplicate = record(1, "collision");
        duplicate.id = existing.id;
        store.save(&existing).await.unwrap();

        store.create(&mut duplicate).await.unwrap();
        assert_ne!(duplicate.id, existing.id);
        assert!(store.load(&duplicate.id).await.unwrap().is_some());

        drop(store);
        drop(dir);
    }

    #[tokio::test]
    async fn delete_removes_the_session() {
        let (dir, store) = store().await;

        let mut created = record(1, "gone");
        store.create(&mut created).await.unwrap();
        store.delete(&created.id).await.unwrap();
        assert!(store.load(&created.id).await.unwrap().is_none());

        // Deleting an unknown id is fine.
        store.delete(&Id::default()).await.unwrap();

        drop(store);
        drop(dir);
    }

    #[tokio::test]
    async fn expired_sessions_are_not_returned() {
        let (dir, store) = store().await;

        let expired = record(-1, "stale");
        store.save(&expired).await.unwrap();
        assert!(store.load(&expired.id).await.unwrap().is_none());

        // And the periodic purge removes the row entirely.
        let purged = store.purge_expired().await.unwrap();
        assert_eq!(purged, 1);
        let remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sessions")
            .fetch_one(&store.0)
            .await
            .unwrap();
        assert_eq!(remaining, 0);

        drop(store);
        drop(dir);
    }
}
