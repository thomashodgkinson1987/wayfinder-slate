//! SQLite database access: pool setup, embedded migrations, and housekeeping.
//!
//! Wayfinder Slate uses a single SQLite file under the configured data
//! directory (default `./data/wayfinder.sqlite3`) in WAL mode. All queries are
//! runtime queries (`sqlx::query`/`query_as`), so no `DATABASE_URL` or offline
//! artifacts are needed and CI stays database-free at compile time.

use std::path::Path;
use std::time::Duration;

use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous};

/// Name of the SQLite database file within the data directory.
pub const DB_FILE_NAME: &str = "wayfinder.sqlite3";

/// SQLite writes serialize, so keep the pool small to avoid
/// `database is locked` pile-ups even with WAL + a busy timeout.
const MAX_CONNECTIONS: u32 = 5;

/// Open (or create) the database pool for `data_dir`, run the embedded
/// migrations, and purge already-expired sessions left from previous runs.
pub async fn open_pool(data_dir: &Path) -> anyhow::Result<SqlitePool> {
    std::fs::create_dir_all(data_dir).map_err(|error| {
        anyhow::anyhow!(
            "could not create data directory {}: {error}",
            data_dir.display()
        )
    })?;

    let options = connect_options(data_dir);
    let pool = SqlitePoolOptions::new()
        .max_connections(MAX_CONNECTIONS)
        .connect_with(options)
        .await
        .map_err(|error| anyhow::anyhow!("could not open wayfinder.sqlite3: {error}"))?;

    run_migrations(&pool).await?;
    let purged = purge_expired_sessions(&pool).await?;

    // Trailing empty record is safe; the file-backed pool starts at one
    // connection and never drops to zero under normal operation.
    tracing::info!(?data_dir, purged_sessions = purged, "database ready");
    Ok(pool)
}

fn connect_options(data_dir: &Path) -> SqliteConnectOptions {
    SqliteConnectOptions::new()
        .filename(data_dir.join(DB_FILE_NAME))
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(5))
}

/// Apply the embedded migrations from the repository-root `migrations/`
/// directory. Idempotent.
pub async fn run_migrations(pool: &SqlitePool) -> anyhow::Result<()> {
    sqlx::migrate!("../../migrations")
        .run(pool)
        .await
        .map_err(|error| anyhow::anyhow!("database migration failed: {error}"))?;
    Ok(())
}

/// Delete sessions whose stored unix-seconds expiry is in the past.
/// Returns the number of rows removed.
pub async fn purge_expired_sessions(pool: &SqlitePool) -> anyhow::Result<u64> {
    let result =
        sqlx::query("DELETE FROM sessions WHERE expiry < CAST(strftime('%s','now') AS INTEGER)")
            .execute(pool)
            .await
            .map_err(|error| anyhow::anyhow!("could not purge expired sessions: {error}"))?;
    Ok(result.rows_affected())
}

#[cfg(test)]
pub(crate) async fn temp_db() -> (tempfile::TempDir, SqlitePool) {
    let dir = tempfile::tempdir().unwrap();
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(connect_options(dir.path()))
        .await
        .unwrap();
    run_migrations(&pool).await.unwrap();
    (dir, pool)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::Row;

    #[tokio::test]
    async fn migrations_run_idempotently_and_create_schema() {
        let dir = tempfile::tempdir().unwrap();

        let pool = open_pool(dir.path()).await.unwrap();

        let names =
            sqlx::query("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
                .fetch_all(&pool)
                .await
                .unwrap()
                .into_iter()
                .map(|row| row.get::<String, _>("name"))
                .collect::<Vec<_>>();
        for expected in ["profiles", "sessions", "users", "save_slots", "snapshots"] {
            assert!(
                names.iter().any(|name| name == expected),
                "table {expected} missing, got {names:?}"
            );
        }

        for index in [
            "users_username_unique",
            "idx_profiles_user",
            "idx_sessions_expiry",
            "idx_save_slots_machine",
            "idx_save_slots_profile",
            "idx_save_slots_profile_created",
            "idx_snapshots_slot_captured",
            "idx_snapshots_slot_ingested",
        ] {
            let exists: i64 = sqlx::query_scalar(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = ?",
            )
            .bind(index)
            .fetch_one(&pool)
            .await
            .unwrap();
            assert_eq!(exists, 1, "index {index} missing");
        }

        // Re-opening the same database must be a no-op (no re-migration,
        // no error) and must reuse the exact file we created.
        drop(pool);
        let pool = open_pool(dir.path()).await.unwrap();
        assert!(dir.path().join(DB_FILE_NAME).is_file());
        assert!(dir.path().join("wayfinder.sqlite3-wal").exists());
        let _ = pool;
    }

    #[tokio::test]
    async fn file_database_runs_in_wal_mode_with_foreign_keys_on() {
        let (dir, pool) = open_pool_dir().await;

        let row = sqlx::query("PRAGMA journal_mode")
            .fetch_one(&pool)
            .await
            .unwrap();
        let mode: String = row.get(0);
        assert_eq!(mode.to_ascii_lowercase(), "wal");

        let row = sqlx::query("PRAGMA foreign_keys")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(row.get::<i64, _>(0), 1);

        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn purge_expired_sessions_only_removes_past_expiry() {
        let (dir, pool) = open_pool_dir().await;

        let now: i64 = sqlx::query_scalar("SELECT CAST(strftime('%s','now') AS INTEGER)")
            .fetch_one(&pool)
            .await
            .unwrap();

        sqlx::query("INSERT INTO sessions (id, data, expiry) VALUES ('old', X'01', ?)")
            .bind(now - 60)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO sessions (id, data, expiry) VALUES ('fresh', X'01', ?)")
            .bind(now + 3600)
            .execute(&pool)
            .await
            .unwrap();

        let removed = purge_expired_sessions(&pool).await.unwrap();
        assert_eq!(removed, 1);

        let remaining: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM sessions")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(remaining, 1);

        std::fs::remove_dir_all(dir).unwrap();
    }

    async fn open_pool_dir() -> (std::path::PathBuf, SqlitePool) {
        let dir = tempfile::tempdir().unwrap();
        let pool = open_pool(dir.path()).await.unwrap();
        (dir.keep(), pool)
    }
}
