//! Save ingestion: turn save files into `save_slots` + `snapshots`.
//!
//! Ingest is shared by the startup scan, the folder watcher, multipart uploads,
//! and (later) the M7 agent. Parsing and hashing run in `spawn_blocking`;
//! database writes are serialized with one process-wide mutex because SQLite
//! writes serialize anyway and ingestion is infrequent.

pub mod watcher;

use std::io::Write;
use std::path::Path;
use std::time::Duration;

use flate2::Compression;
use flate2::write::GzEncoder;
use sha2::{Digest, Sha256};
use sqlx::{AssertSqlSafe, Sqlite, SqlitePool, Transaction};
use time::OffsetDateTime;
use tokio::sync::Mutex;
use uuid::Uuid;
use wayfinder_save::{CompletionSummary, SaveCaption, SaveError, SaveFile, SaveStats};

use crate::config::{Config, DiscoveredSlot};
use crate::saves::{self, CompletedObjects, MapObjects};

/// Largest accepted `game_data.sav`.
pub const MAX_GAME_DATA_BYTES: usize = 4 * 1024 * 1024;
/// Largest accepted `caption.sav`.
pub const MAX_CAPTION_SAV_BYTES: usize = 1024 * 1024;
/// Largest accepted `caption.jpg`.
pub const MAX_CAPTION_IMAGE_BYTES: usize = 2 * 1024 * 1024;
/// Highest slot index accepted from a path or upload.
pub const MAX_SLOT_INDEX: u32 = 255;

const RETRY_DELAYS_MS: [u64; 3] = [250, 500, 1000];

static INGEST_LOCK: Mutex<()> = Mutex::const_new(());

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IngestSource {
    Local,
    Upload,
    /// The M7 agent will use this source.
    #[allow(dead_code)]
    Agent,
}

impl IngestSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Upload => "upload",
            Self::Agent => "agent",
        }
    }
}

/// Where a save should be stored. `profile_id = None` is the machine-local
/// source; `slot_index = None` allocates the next free index for the owner.
#[derive(Debug, Clone)]
pub struct IngestTarget {
    pub profile_id: Option<String>,
    pub slot_index: Option<u32>,
    pub source: IngestSource,
}

#[derive(Debug)]
pub struct SavePayload {
    pub game_data: Vec<u8>,
    pub caption_sav: Option<Vec<u8>>,
    pub caption_image: Option<Vec<u8>>,
    pub last_modified: i64,
}

#[derive(Debug, Clone, Copy)]
pub struct IngestOptions {
    pub snapshot_limit: u32,
    pub keep_raw_saves: bool,
}

impl From<&Config> for IngestOptions {
    fn from(config: &Config) -> Self {
        Self {
            snapshot_limit: config.snapshot_limit,
            keep_raw_saves: config.keep_raw_saves,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IngestOutcome {
    pub slot_index: u32,
    pub created: bool,
    pub changed: bool,
}

#[derive(Debug)]
pub enum IngestError {
    Io(std::io::Error),
    InvalidSave(SaveError),
    Db(sqlx::Error),
    InvalidTarget(String),
    Join(String),
}

impl IngestError {
    /// Partial writes from the game show up as structural failures; retry those.
    /// A bad marker or big-endian save is definitive, not a race.
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::Io(_) => true,
            Self::InvalidSave(error) => matches!(
                error,
                SaveError::TooSmall { .. }
                    | SaveError::InvalidLength { .. }
                    | SaveError::MissingSentinel
            ),
            _ => false,
        }
    }
}

impl std::fmt::Display for IngestError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "could not read save: {error}"),
            Self::InvalidSave(error) => write!(formatter, "invalid save: {error}"),
            Self::Db(error) => write!(formatter, "database error: {error}"),
            Self::InvalidTarget(message) => formatter.write_str(message),
            Self::Join(message) => write!(formatter, "ingest task failed: {message}"),
        }
    }
}

impl std::error::Error for IngestError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::InvalidSave(error) => Some(error),
            Self::Db(error) => Some(error),
            _ => None,
        }
    }
}

impl From<std::io::Error> for IngestError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<sqlx::Error> for IngestError {
    fn from(error: sqlx::Error) -> Self {
        Self::Db(error)
    }
}

struct ParsedPayload {
    stats: SaveStats,
    caption: SaveCaption,
    completion_summary: CompletionSummary,
    completed: CompletedObjects,
    game_version: String,
    raw_blob: Option<Vec<u8>>,
    raw_size: Option<i64>,
}

/// Store one save state for a slot.
pub async fn ingest(
    pool: &SqlitePool,
    objects: &MapObjects,
    target: IngestTarget,
    payload: SavePayload,
    options: IngestOptions,
) -> Result<IngestOutcome, IngestError> {
    let _guard = INGEST_LOCK.lock().await;

    if payload.game_data.len() > MAX_GAME_DATA_BYTES {
        return Err(IngestError::InvalidTarget(
            "save file is larger than the 4 MiB limit".into(),
        ));
    }
    if payload
        .caption_sav
        .as_ref()
        .is_some_and(|bytes| bytes.len() > MAX_CAPTION_SAV_BYTES)
    {
        return Err(IngestError::InvalidTarget(
            "caption.sav is larger than the 1 MiB limit".into(),
        ));
    }
    if payload
        .caption_image
        .as_ref()
        .is_some_and(|bytes| bytes.len() > MAX_CAPTION_IMAGE_BYTES)
    {
        return Err(IngestError::InvalidTarget(
            "caption image is larger than the 2 MiB limit".into(),
        ));
    }

    let SavePayload {
        game_data,
        caption_sav,
        caption_image,
        last_modified,
    } = payload;

    // Hash off the async runtime; a scan hashes every slot before deciding
    // whether the more expensive parse is needed.
    let hash = {
        let bytes = game_data.clone();
        tokio::task::spawn_blocking(move || hash_bytes(&bytes))
            .await
            .map_err(|error| IngestError::Join(error.to_string()))?
    };

    let now = OffsetDateTime::now_utc().unix_timestamp();
    let mut tx = pool.begin().await?;

    let slot_index = match target.slot_index {
        Some(index) => index,
        None => next_slot_index(&mut tx, target.profile_id.as_deref()).await?,
    };
    if slot_index > MAX_SLOT_INDEX {
        return Err(IngestError::InvalidTarget(format!(
            "slot index {slot_index} exceeds the maximum of {MAX_SLOT_INDEX}"
        )));
    }

    let existing: Option<(String, String)> = sqlx::query_as(
        "SELECT id, raw_hash FROM save_slots WHERE profile_id IS ? AND slot_index = ?",
    )
    .bind(target.profile_id.as_deref())
    .bind(i64::from(slot_index))
    .fetch_optional(&mut *tx)
    .await?;

    let changed = existing
        .as_ref()
        .is_none_or(|(_, stored_hash)| stored_hash != &hash);

    if !changed {
        // Same bytes as the stored save: refresh caption/label/presence only
        // and skip parsing the (often ~1 MB) game save entirely.
        let caption = parse_caption(caption_sav.clone()).await?;
        let label = label_for(&caption, slot_index, objects);
        let (id, _) = existing
            .as_ref()
            .expect("!changed means a slot already exists");
        sqlx::query(
            "UPDATE save_slots SET label = ?, caption_location = ?, caption_map_area = ?, \
             source = ?, last_modified = ?, caption_sav = ?, caption_image = ?, present = 1, \
             updated_at = ? WHERE id = ?",
        )
        .bind(&label)
        .bind(&caption.location)
        .bind(&caption.map_area)
        .bind(target.source.as_str())
        .bind(last_modified)
        .bind(caption_sav.as_deref())
        .bind(caption_image.as_deref())
        .bind(now)
        .bind(id)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        return Ok(IngestOutcome {
            slot_index,
            created: false,
            changed: false,
        });
    }

    // New or changed: parse the save and derive every stored value.
    let objects_for_parse = objects.clone();
    let caption_sav_for_parse = caption_sav.clone();
    let keep_raw = options.keep_raw_saves;
    let parsed = tokio::task::spawn_blocking(move || {
        parse_payload(
            game_data,
            caption_sav_for_parse,
            &objects_for_parse,
            keep_raw,
        )
    })
    .await
    .map_err(|error| IngestError::Join(error.to_string()))??;

    let label = label_for(&parsed.caption, slot_index, objects);
    let captured_at = captured_at(parsed.caption.saved_at, last_modified, now);
    let completion_json =
        serde_json::to_string(&parsed.completed).unwrap_or_else(|_| "{}".to_owned());
    let stats_json = serde_json::to_string(&parsed.stats).unwrap_or_else(|_| "{}".to_owned());
    let summary_json =
        serde_json::to_string(&parsed.completion_summary).unwrap_or_else(|_| "{}".to_owned());

    let slot_id = match &existing {
        Some((id, _)) => {
            sqlx::query(
                "UPDATE save_slots SET label = ?, caption_location = ?, \
                 caption_map_area = ?, source = ?, last_modified = ?, raw_hash = ?, \
                 caption_sav = ?, caption_image = ?, completion_json = ?, present = 1, \
                 updated_at = ? WHERE id = ?",
            )
            .bind(&label)
            .bind(&parsed.caption.location)
            .bind(&parsed.caption.map_area)
            .bind(target.source.as_str())
            .bind(last_modified)
            .bind(&hash)
            .bind(caption_sav.as_deref())
            .bind(caption_image.as_deref())
            .bind(&completion_json)
            .bind(now)
            .bind(id)
            .execute(&mut *tx)
            .await?;
            id.clone()
        }
        None => {
            let id = Uuid::now_v7().to_string();
            sqlx::query(
                "INSERT INTO save_slots (id, profile_id, slot_index, label, \
                 caption_location, caption_map_area, source, last_modified, raw_hash, \
                 caption_sav, caption_image, completion_json, present, created_at, updated_at) \
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?, ?)",
            )
            .bind(&id)
            .bind(target.profile_id.as_deref())
            .bind(i64::from(slot_index))
            .bind(&label)
            .bind(&parsed.caption.location)
            .bind(&parsed.caption.map_area)
            .bind(target.source.as_str())
            .bind(last_modified)
            .bind(&hash)
            .bind(caption_sav.as_deref())
            .bind(caption_image.as_deref())
            .bind(&completion_json)
            .bind(now)
            .bind(now)
            .execute(&mut *tx)
            .await?;
            id
        }
    };

    let snapshot_id = Uuid::now_v7().to_string();
    sqlx::query(
        "INSERT INTO snapshots (id, slot_id, captured_at, ingested_at, game_version, \
         playtime_s, rupees, hearts_current, hearts_max, stamina_current, stamina_max, \
         korok_count, position_x, position_y, position_z, map_name, map_type, \
         stats_json, completion_summary_json, raw_blob, raw_size, created_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&snapshot_id)
    .bind(&slot_id)
    .bind(captured_at)
    .bind(now)
    .bind(&parsed.game_version)
    .bind(parsed.stats.play_time_seconds as i64)
    .bind(i64::from(parsed.stats.rupees))
    .bind(f64::from(parsed.stats.hearts.current))
    .bind(f64::from(parsed.stats.hearts.max))
    .bind(f64::from(parsed.stats.stamina.current))
    .bind(f64::from(parsed.stats.stamina.max))
    .bind(parsed.completion_summary.koroks.completed as i64)
    .bind(f64::from(parsed.stats.location.x))
    .bind(f64::from(parsed.stats.location.y))
    .bind(f64::from(parsed.stats.location.z))
    .bind(&parsed.stats.location.map)
    .bind(&parsed.stats.location.map_type)
    .bind(&stats_json)
    .bind(&summary_json)
    .bind(parsed.raw_blob.as_deref())
    .bind(parsed.raw_size)
    .bind(now)
    .execute(&mut *tx)
    .await?;

    if options.snapshot_limit > 0 {
        // Prune oldest observations by `ingested_at`, never the row just
        // inserted (its `id` is protected explicitly so equal timestamps
        // cannot evict it).
        sqlx::query(
            "DELETE FROM snapshots WHERE slot_id = ? AND id <> ? AND id NOT IN ( \
             SELECT id FROM snapshots WHERE slot_id = ? \
             ORDER BY ingested_at DESC, id DESC LIMIT ?)",
        )
        .bind(&slot_id)
        .bind(&snapshot_id)
        .bind(&slot_id)
        .bind(i64::from(options.snapshot_limit))
        .execute(&mut *tx)
        .await?;
    }

    tx.commit().await?;

    Ok(IngestOutcome {
        slot_index,
        created: existing.is_none(),
        changed: true,
    })
}

fn label_for(caption: &SaveCaption, slot_index: u32, objects: &MapObjects) -> String {
    objects
        .for_location(&caption.location)
        .map(str::to_owned)
        .unwrap_or_else(|| format!("Slot {slot_index}"))
}

async fn parse_caption(caption_sav: Option<Vec<u8>>) -> Result<SaveCaption, IngestError> {
    tokio::task::spawn_blocking(move || match caption_sav {
        Some(bytes) => SaveFile::parse(bytes)
            .map(|save| save.caption())
            .map_err(IngestError::InvalidSave),
        None => Ok(SaveCaption::default()),
    })
    .await
    .map_err(|error| IngestError::Join(error.to_string()))?
}

fn parse_payload(
    game_data: Vec<u8>,
    caption_sav: Option<Vec<u8>>,
    objects: &MapObjects,
    keep_raw: bool,
) -> Result<ParsedPayload, IngestError> {
    let save = SaveFile::parse(game_data).map_err(IngestError::InvalidSave)?;
    let stats = save.stats();
    let caption = match caption_sav {
        Some(bytes) => SaveFile::parse(bytes)
            .map_err(IngestError::InvalidSave)?
            .caption(),
        None => SaveCaption::default(),
    };
    let completion_summary = save.completion_summary();
    let completed = saves::completed_objects(&save, objects);
    let game_version = game_version_label(save.version());

    let (raw_blob, raw_size) = if keep_raw {
        let raw = save.raw();
        let compressed = gzip(raw)?;
        (Some(compressed), Some(raw.len() as i64))
    } else {
        (None, None)
    };

    Ok(ParsedPayload {
        stats,
        caption,
        completion_summary,
        completed,
        game_version,
        raw_blob,
        raw_size,
    })
}

async fn next_slot_index(
    tx: &mut Transaction<'_, Sqlite>,
    profile_id: Option<&str>,
) -> Result<u32, IngestError> {
    let max: Option<i64> =
        sqlx::query_scalar("SELECT MAX(slot_index) FROM save_slots WHERE profile_id IS ?")
            .bind(profile_id)
            .fetch_one(&mut **tx)
            .await?;
    let next = max.map(|value| value + 1).unwrap_or(0);
    u32::try_from(next).map_err(|_| IngestError::InvalidTarget("slot index is out of range".into()))
}

fn captured_at(saved_at: u64, last_modified: i64, now: i64) -> i64 {
    let saved_at = i64::try_from(saved_at).unwrap_or(0);
    let future_limit = now.saturating_add(86_400);
    if saved_at > 0 && saved_at <= future_limit {
        saved_at
    } else if last_modified > 0 && last_modified <= future_limit {
        last_modified
    } else {
        now
    }
}

fn game_version_label(version: u32) -> String {
    if version == wayfinder_save::VERSION_1_9_0 {
        "1.9.0".to_owned()
    } else {
        format!("0x{version:08x}")
    }
}

fn gzip(data: &[u8]) -> Result<Vec<u8>, IngestError> {
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(data)?;
    Ok(encoder.finish()?)
}

pub fn hash_bytes(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

/// Read the three files for one slot directory.
pub async fn read_slot_payload(dir: &Path) -> Result<SavePayload, IngestError> {
    let game_data_path = dir.join("game_data.sav");
    let metadata = tokio::fs::metadata(&game_data_path).await?;
    let last_modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0);
    let game_data = tokio::fs::read(&game_data_path).await?;
    let caption_sav = read_optional(&dir.join("caption.sav"), MAX_CAPTION_SAV_BYTES).await;
    let caption_image = read_optional(&dir.join("caption.jpg"), MAX_CAPTION_IMAGE_BYTES).await;

    Ok(SavePayload {
        game_data,
        caption_sav,
        caption_image,
        last_modified,
    })
}

async fn read_optional(path: &Path, max_bytes: usize) -> Option<Vec<u8>> {
    match tokio::fs::read(path).await {
        Ok(bytes) if bytes.len() <= max_bytes => Some(bytes),
        _ => None,
    }
}

/// Read and ingest one slot, retrying partial writes from the game.
pub async fn ingest_slot_dir(
    pool: &SqlitePool,
    objects: &MapObjects,
    target: IngestTarget,
    dir: &Path,
    options: IngestOptions,
) -> Result<IngestOutcome, IngestError> {
    let mut last_error = None;

    for (attempt, delay) in std::iter::once(0_u64).chain(RETRY_DELAYS_MS).enumerate() {
        if delay > 0 {
            tokio::time::sleep(Duration::from_millis(delay)).await;
        }

        let payload = match read_slot_payload(dir).await {
            Ok(payload) => payload,
            Err(error) if error.is_retryable() => {
                tracing::debug!(attempt, %error, "save read failed; retrying");
                last_error = Some(error);
                continue;
            }
            Err(error) => return Err(error),
        };

        match ingest(pool, objects, target.clone(), payload, options).await {
            Ok(outcome) => return Ok(outcome),
            Err(error) if error.is_retryable() => {
                tracing::debug!(attempt, %error, "save parse failed; retrying");
                last_error = Some(error);
            }
            Err(error) => return Err(error),
        }
    }

    Err(last_error.unwrap_or_else(|| {
        IngestError::InvalidTarget("save could not be read after retries".into())
    }))
}

#[derive(Debug, Default)]
pub struct ScanReport {
    pub slots: usize,
    pub ingested: usize,
    pub unchanged: usize,
    pub errors: usize,
}

/// Scan the machine-local save folder and ingest every changed slot.
///
/// Only a successful directory listing may mark missing slots absent; a
/// transient I/O error leaves the previous presence state untouched.
pub async fn scan_machine_slots(
    config: &Config,
    pool: &SqlitePool,
    objects: &MapObjects,
) -> ScanReport {
    let mut report = ScanReport::default();
    let Some(root) = config.save_root() else {
        return report;
    };

    let slots: Vec<DiscoveredSlot> = match crate::config::try_discover_slots(&root) {
        Ok(slots) => slots,
        Err(error) => {
            tracing::warn!(%error, "could not list the save folder");
            return report;
        }
    };

    let options = IngestOptions::from(config);
    let mut indexes = Vec::with_capacity(slots.len());

    for slot in slots {
        indexes.push(slot.index);
        report.slots += 1;
        let target = IngestTarget {
            profile_id: None,
            slot_index: Some(slot.index as u32),
            source: IngestSource::Local,
        };
        match ingest_slot_dir(pool, objects, target, &slot.dir, options).await {
            Ok(outcome) if outcome.changed => report.ingested += 1,
            Ok(_) => report.unchanged += 1,
            Err(error) => {
                report.errors += 1;
                tracing::warn!(slot = slot.index, %error, "could not ingest save slot");
            }
        }
    }

    if let Err(error) = mark_presence(pool, &indexes).await {
        tracing::warn!(%error, "could not update save slot presence");
    }

    report
}

async fn mark_presence(pool: &SqlitePool, present: &[usize]) -> Result<(), sqlx::Error> {
    let now = OffsetDateTime::now_utc().unix_timestamp();
    if present.is_empty() {
        sqlx::query(
            "UPDATE save_slots SET present = 0, updated_at = ? \
             WHERE profile_id IS NULL AND present = 1",
        )
        .bind(now)
        .execute(pool)
        .await?;
        return Ok(());
    }

    let indexes = present
        .iter()
        .map(usize::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let statement = format!(
        "UPDATE save_slots SET present = 0, updated_at = ? \
         WHERE profile_id IS NULL AND present = 1 AND slot_index NOT IN ({indexes})"
    );
    sqlx::query(AssertSqlSafe(statement))
        .bind(now)
        .execute(pool)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Read;
    use std::path::PathBuf;

    use flate2::read::GzDecoder;
    use sqlx::Row;
    use wayfinder_save::keys;

    use super::*;
    use crate::config::Mode;
    use crate::saves::test_support::{build_save, caption_save, temp_root, valid_save, write_slot};

    fn objects() -> &'static MapObjects {
        crate::http::map_dataset()
    }

    fn payload(game_data: Vec<u8>) -> SavePayload {
        SavePayload {
            game_data,
            caption_sav: Some(caption_save()),
            caption_image: None,
            last_modified: 0,
        }
    }

    fn machine_target(index: u32) -> IngestTarget {
        IngestTarget {
            profile_id: None,
            slot_index: Some(index),
            source: IngestSource::Local,
        }
    }

    fn options(snapshot_limit: u32, keep_raw_saves: bool) -> IngestOptions {
        IngestOptions {
            snapshot_limit,
            keep_raw_saves,
        }
    }

    fn local_config(save_dir: PathBuf) -> Config {
        Config {
            mode: Mode::Local,
            bind: "127.0.0.1:0".parse().unwrap(),
            save_dir: Some(save_dir),
            control_addr: "127.0.0.1:7777".parse().unwrap(),
            data_dir: PathBuf::from("data"),
            cookie_secure: false,
            snapshot_limit: 500,
            keep_raw_saves: false,
            watch_saves: false,
            max_upload_bytes: 8 * 1024 * 1024,
        }
    }

    async fn snapshot_count(pool: &SqlitePool) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM snapshots")
            .fetch_one(pool)
            .await
            .unwrap()
    }

    /// A minimal valid save whose hash differs per playtime.
    fn save_with_playtime(seconds: u32) -> Vec<u8> {
        build_save(&[
            (keys::PLAY_TIME, seconds),
            (keys::CURRENT_RUPEE, 100),
            (keys::CURRENT_HART, 4),
            (keys::MAX_HART, 4),
            (SaveFile::completion_key("Clear_Dungeon015"), 1),
        ])
    }

    #[test]
    fn hashes_are_stable_and_hex() {
        let hash = hash_bytes(b"wayfinder");
        assert_eq!(hash.len(), 64);
        assert!(hash.chars().all(|character| character.is_ascii_hexdigit()));
        assert_eq!(hash, hash_bytes(b"wayfinder"));
    }

    #[test]
    fn captured_at_prefers_sane_caption_times() {
        let now = 1_700_000_000;
        assert_eq!(captured_at(1_600_000_000, 0, now), 1_600_000_000);
        assert_eq!(captured_at(0, 1_650_000_000, now), 1_650_000_000);
        assert_eq!(captured_at(0, 0, now), now);
        // Absurd future caption times fall back.
        assert_eq!(
            captured_at(now as u64 + 999_999, 1_650_000_000, now),
            1_650_000_000
        );
    }

    #[test]
    fn version_labels_are_readable() {
        assert_eq!(game_version_label(wayfinder_save::VERSION_1_9_0), "1.9.0");
        assert_eq!(game_version_label(0x1234), "0x00001234");
    }

    #[test]
    fn only_structural_failures_are_retried() {
        assert!(
            IngestError::Io(std::io::Error::new(std::io::ErrorKind::WouldBlock, "busy"))
                .is_retryable()
        );
        assert!(
            IngestError::InvalidSave(wayfinder_save::SaveError::TooSmall { len: 1, min: 16 })
                .is_retryable()
        );
        assert!(!IngestError::InvalidSave(wayfinder_save::SaveError::BadMarker(0)).is_retryable());
        assert!(
            !IngestError::InvalidSave(wayfinder_save::SaveError::UnsupportedEndianness)
                .is_retryable()
        );
        assert!(!IngestError::InvalidTarget("nope".into()).is_retryable());
    }

    #[tokio::test]
    async fn ingest_creates_slot_and_snapshot() {
        let (_dir, pool) = crate::db::temp_db().await;
        let outcome = ingest(
            &pool,
            objects(),
            machine_target(0),
            payload(valid_save()),
            options(500, false),
        )
        .await
        .unwrap();
        assert!(outcome.created);
        assert!(outcome.changed);
        assert_eq!(outcome.slot_index, 0);

        let row = sqlx::query(
            "SELECT label, caption_location, raw_hash, completion_json, present \
             FROM save_slots WHERE profile_id IS NULL AND slot_index = 0",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(row.get::<String, _>("label"), "Dueling Peaks Stable");
        assert_eq!(row.get::<String, _>("caption_location"), "FaronHatago000");
        assert_eq!(row.get::<String, _>("raw_hash"), hash_bytes(&valid_save()));
        assert_eq!(row.get::<i64, _>("present"), 1);

        let completed: CompletedObjects =
            serde_json::from_str(&row.get::<String, _>("completion_json")).unwrap();
        assert_eq!(completed.shrines, vec!["Clear_Dungeon015".to_owned()]);
        assert!(completed.towers.is_empty());

        assert_eq!(snapshot_count(&pool).await, 1);
        let snap = sqlx::query(
            "SELECT playtime_s, hearts_current, map_name, game_version, raw_blob \
             FROM snapshots",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(snap.get::<i64, _>("playtime_s"), 50_166);
        assert_eq!(snap.get::<f64, _>("hearts_current"), 4.0);
        assert_eq!(snap.get::<String, _>("map_name"), "G-6");
        assert_eq!(snap.get::<String, _>("game_version"), "1.9.0");
        assert!(snap.get::<Option<Vec<u8>>, _>("raw_blob").is_none());
    }

    #[tokio::test]
    async fn reingesting_identical_bytes_adds_no_snapshot() {
        let (_dir, pool) = crate::db::temp_db().await;
        let first = ingest(
            &pool,
            objects(),
            machine_target(0),
            payload(valid_save()),
            options(500, false),
        )
        .await
        .unwrap();
        assert!(first.changed);

        let second = ingest(
            &pool,
            objects(),
            machine_target(0),
            payload(valid_save()),
            options(500, false),
        )
        .await
        .unwrap();
        assert!(!second.created, "the slot already exists");
        assert!(!second.changed, "identical bytes are not a new state");
        assert_eq!(snapshot_count(&pool).await, 1);
    }

    #[tokio::test]
    async fn changed_bytes_add_a_second_snapshot() {
        let (_dir, pool) = crate::db::temp_db().await;
        ingest(
            &pool,
            objects(),
            machine_target(0),
            payload(save_with_playtime(100)),
            options(500, false),
        )
        .await
        .unwrap();
        let second = ingest(
            &pool,
            objects(),
            machine_target(0),
            payload(save_with_playtime(200)),
            options(500, false),
        )
        .await
        .unwrap();
        assert!(second.changed);
        assert!(!second.created);
        assert_eq!(snapshot_count(&pool).await, 2);
    }

    #[tokio::test]
    async fn retention_keeps_the_newest_snapshots() {
        let (_dir, pool) = crate::db::temp_db().await;
        for (index, playtime) in [100_u32, 200, 300].into_iter().enumerate() {
            if index > 0 {
                // Give each ingest a distinct `ingested_at` second so the
                // prune order is exact rather than id-tie-broken.
                tokio::time::sleep(Duration::from_millis(1_100)).await;
            }
            let outcome = ingest(
                &pool,
                objects(),
                machine_target(0),
                payload(save_with_playtime(playtime)),
                options(2, false),
            )
            .await
            .unwrap();
            assert!(outcome.changed);
        }

        assert_eq!(snapshot_count(&pool).await, 2);
        let remaining: Vec<i64> =
            sqlx::query_scalar("SELECT playtime_s FROM snapshots ORDER BY playtime_s")
                .fetch_all(&pool)
                .await
                .unwrap();
        assert_eq!(remaining, vec![200, 300], "the two newest survive");
    }

    #[tokio::test]
    async fn a_zero_limit_keeps_all_snapshots() {
        let (_dir, pool) = crate::db::temp_db().await;
        for playtime in [100_u32, 200, 300] {
            ingest(
                &pool,
                objects(),
                machine_target(0),
                payload(save_with_playtime(playtime)),
                options(0, false),
            )
            .await
            .unwrap();
        }
        assert_eq!(snapshot_count(&pool).await, 3);
    }

    #[tokio::test]
    async fn raw_saves_are_opt_in_and_gzip_encoded() {
        let (_dir, pool) = crate::db::temp_db().await;
        let bytes = valid_save();
        ingest(
            &pool,
            objects(),
            machine_target(0),
            payload(bytes.clone()),
            options(500, false),
        )
        .await
        .unwrap();
        let stored: Option<Vec<u8>> = sqlx::query_scalar("SELECT raw_blob FROM snapshots")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert!(stored.is_none(), "raw saves are off by default");

        let (_dir2, pool2) = crate::db::temp_db().await;
        ingest(
            &pool2,
            objects(),
            machine_target(0),
            payload(bytes.clone()),
            options(500, true),
        )
        .await
        .unwrap();
        let blob: Vec<u8> = sqlx::query_scalar("SELECT raw_blob FROM snapshots")
            .fetch_one(&pool2)
            .await
            .unwrap();
        let mut decoder = GzDecoder::new(&blob[..]);
        let mut decoded = Vec::new();
        decoder.read_to_end(&mut decoded).unwrap();
        assert_eq!(decoded, bytes);

        let raw_size: i64 = sqlx::query_scalar("SELECT raw_size FROM snapshots")
            .fetch_one(&pool2)
            .await
            .unwrap();
        assert_eq!(raw_size, bytes.len() as i64);
    }

    #[tokio::test]
    async fn deleted_machine_slots_are_marked_absent_but_keep_history() {
        let root = temp_root("ingest-presence");
        write_slot(
            &root.join("save_0"),
            0,
            &valid_save(),
            Some(&caption_save()),
            None,
        );
        write_slot(
            &root.join("save_0"),
            1,
            &valid_save(),
            Some(&caption_save()),
            None,
        );
        let config = local_config(root.clone());
        let (_dir, pool) = crate::db::temp_db().await;

        let report = scan_machine_slots(&config, &pool, objects()).await;
        assert_eq!(report.slots, 2);
        assert_eq!(report.ingested, 2);
        assert_eq!(report.errors, 0);
        assert_eq!(saves::list_machine_slots(&pool).await.unwrap().len(), 2);

        std::fs::remove_dir_all(root.join("save_0").join("1")).unwrap();
        let report = scan_machine_slots(&config, &pool, objects()).await;
        assert_eq!(report.slots, 1);

        let present: i64 = sqlx::query_scalar(
            "SELECT present FROM save_slots WHERE profile_id IS NULL AND slot_index = 1",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(present, 0);

        // History survives even though the slot is hidden from listings.
        let retained: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM snapshots s JOIN save_slots l ON l.id = s.slot_id \
             WHERE l.slot_index = 1",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(retained, 1);

        let listed = saves::list_machine_slots(&pool).await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].slot, 0);

        std::fs::remove_dir_all(root).unwrap();
    }

    /// Ingest the private development save when `WAYFINDER_TEST_SAVE` points at
    /// one, and confirm stored snapshots match the parser exactly.
    #[tokio::test]
    async fn real_save_ingests_when_configured() {
        let Some(path) = std::env::var_os("WAYFINDER_TEST_SAVE") else {
            eprintln!("skipping: WAYFINDER_TEST_SAVE is not set");
            return;
        };
        let path = PathBuf::from(path);
        let game_data = std::fs::read(&path).unwrap();
        let expected = SaveFile::parse(game_data.clone()).unwrap();
        let caption_sav = std::fs::read(path.with_file_name("caption.sav")).ok();

        let (_dir, pool) = crate::db::temp_db().await;
        let outcome = ingest(
            &pool,
            objects(),
            machine_target(0),
            SavePayload {
                game_data,
                caption_sav,
                caption_image: None,
                last_modified: 0,
            },
            options(500, false),
        )
        .await
        .unwrap();
        assert!(outcome.created);

        let detail = saves::load_machine_slot(&pool, 0).await.unwrap();
        assert_eq!(detail.stats, expected.stats());
        assert_eq!(
            serde_json::to_string(&detail.completion).unwrap(),
            serde_json::to_string(&expected.completion_summary()).unwrap()
        );
    }
}
