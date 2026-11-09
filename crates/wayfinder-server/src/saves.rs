//! Save-slot reads backed by stored snapshots.
//!
//! Ingestion (the folder watcher, startup scan, upload, import) lives in
//! [`crate::ingest`]; this module only queries `save_slots` and `snapshots`.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use sqlx::{AssertSqlSafe, Row, SqlitePool};
use wayfinder_save::{CompletionSummary, SaveFile, SaveStats};

/// List entry returned by the slot endpoints.
#[derive(Debug, Clone, Serialize)]
pub struct SlotSummary {
    pub slot: usize,
    pub label: String,
    pub location: String,
    pub map_area: String,
    pub saved_at: u64,
    pub caption_url: String,
}

/// Detail returned by `GET /api/slots/{slot}`.
#[derive(Debug, Serialize)]
pub struct SlotDetail {
    #[serde(flatten)]
    pub summary: SlotSummary,
    pub stats: SaveStats,
    pub completion: CompletionSummary,
}

/// Response returned by `GET /api/slots/{slot}/map`.
#[derive(Debug, Serialize)]
pub struct MapCompletion {
    pub slot: usize,
    pub completed: CompletedObjects,
}

/// Dataset identifiers for the objects completed in a save slot.
///
/// Named flags and Divine Beasts are returned by their dataset `flag` string;
/// treasure chests are returned by their CRC32 save-flag hash.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct CompletedObjects {
    pub shrines: Vec<String>,
    pub towers: Vec<String>,
    pub memories: Vec<String>,
    pub koroks: Vec<String>,
    pub locations: Vec<String>,
    pub enemies: Vec<String>,
    pub divine_beasts: Vec<String>,
    pub treasures: Vec<u32>,
}

#[derive(Debug)]
pub enum SlotError {
    /// No stored slot has this number.
    SlotNotFound,
    /// The slot exists but has no readable caption screenshot.
    CaptionNotFound,
    /// The stored snapshot metadata could not be decoded.
    Unreadable(String),
}

impl std::fmt::Display for SlotError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SlotNotFound => formatter.write_str("save slot not found"),
            Self::CaptionNotFound => formatter.write_str("caption not found"),
            Self::Unreadable(error) => write!(formatter, "stored save data is unreadable: {error}"),
        }
    }
}

impl std::error::Error for SlotError {}

impl From<sqlx::Error> for SlotError {
    fn from(error: sqlx::Error) -> Self {
        Self::Unreadable(error.to_string())
    }
}

/// Which owner's slots to read.
#[derive(Debug, Clone, Copy)]
pub enum SlotScope<'a> {
    /// The machine-local source (`profile_id IS NULL`).
    Machine,
    /// A profile-owned source.
    Profile(&'a str),
}

impl<'a> SlotScope<'a> {
    fn clause(self) -> &'static str {
        match self {
            Self::Machine => "l.profile_id IS NULL",
            Self::Profile(_) => "l.profile_id = ?",
        }
    }

    fn profile_id(self) -> Option<&'a str> {
        match self {
            Self::Machine => None,
            Self::Profile(id) => Some(id),
        }
    }

    fn caption_url(self, slot: usize) -> String {
        match self {
            Self::Machine => format!("/api/slots/{slot}/caption"),
            Self::Profile(id) => format!("/api/profiles/{id}/slots/{slot}/caption"),
        }
    }
}

/// Parsed map-object dataset, reduced to what the server needs.
///
/// The dataset joins save flags to Zelda Dungeon markers. The server keeps the
/// `flag -> friendly name` join for caption labels and, per object type, the
/// dataset flags (and treasure hashes) used for per-object completion.
#[derive(Debug, Default, Clone)]
pub struct MapObjects {
    labels: HashMap<String, String>,
    shrines: Vec<String>,
    towers: Vec<String>,
    memories: Vec<String>,
    koroks: Vec<String>,
    locations: Vec<String>,
    enemies: Vec<String>,
    divine_beasts: Vec<String>,
    treasures: Vec<u32>,
}

impl MapObjects {
    pub fn from_json(bytes: &[u8]) -> serde_json::Result<Self> {
        #[derive(Deserialize)]
        struct Dataset {
            #[serde(default)]
            locations: Vec<LabelledObject>,
            #[serde(default)]
            shrines: Vec<LabelledObject>,
            #[serde(default)]
            towers: Vec<FlagObject>,
            #[serde(default)]
            memories: Vec<FlagObject>,
            #[serde(default)]
            koroks: Vec<FlagObject>,
            #[serde(default)]
            enemies: Vec<FlagObject>,
            #[serde(default, rename = "divineBeasts")]
            divine_beasts: Vec<FlagObject>,
            #[serde(default)]
            treasures: Vec<TreasureObject>,
        }

        #[derive(Deserialize)]
        struct LabelledObject {
            flag: String,
            #[serde(default)]
            name: Option<String>,
            #[serde(default)]
            match_name: Option<String>,
        }

        #[derive(Deserialize)]
        struct FlagObject {
            #[serde(default)]
            flag: String,
        }

        #[derive(Deserialize)]
        struct TreasureObject {
            #[serde(default)]
            hash: Option<u32>,
        }

        fn flags(objects: &[FlagObject]) -> Vec<String> {
            objects
                .iter()
                .filter(|object| !object.flag.is_empty())
                .map(|object| object.flag.clone())
                .collect()
        }

        fn labelled_flags(objects: &[LabelledObject]) -> Vec<String> {
            objects
                .iter()
                .filter(|object| !object.flag.is_empty())
                .map(|object| object.flag.clone())
                .collect()
        }

        let dataset: Dataset = serde_json::from_slice(bytes)?;
        let mut labels = HashMap::new();

        for object in &dataset.locations {
            if let Some(name) = &object.match_name {
                labels.insert(object.flag.clone(), name.clone());
            }
        }
        for object in &dataset.shrines {
            if let Some(name) = &object.name {
                labels
                    .entry(object.flag.clone())
                    .or_insert_with(|| name.clone());
            }
        }

        Ok(Self {
            labels,
            shrines: labelled_flags(&dataset.shrines),
            towers: flags(&dataset.towers),
            memories: flags(&dataset.memories),
            koroks: flags(&dataset.koroks),
            locations: labelled_flags(&dataset.locations),
            enemies: flags(&dataset.enemies),
            divine_beasts: flags(&dataset.divine_beasts),
            treasures: dataset
                .treasures
                .iter()
                .filter_map(|object| object.hash)
                .collect(),
        })
    }

    /// Resolve a caption location such as `FaronHatago000` or `Dungeon015`.
    pub fn for_location(&self, location: &str) -> Option<&str> {
        let location = location.trim();
        if location.is_empty() {
            return None;
        }

        let flag = if let Some(dungeon) = location.strip_prefix("Dungeon") {
            format!("Clear_Dungeon{dungeon}")
        } else if location.starts_with("Location_") || location.starts_with("Clear_") {
            location.to_owned()
        } else {
            format!("Location_{location}")
        };

        self.labels.get(&flag).map(String::as_str)
    }
}

fn flags_set(save: &SaveFile, flags: &[String]) -> Vec<String> {
    flags
        .iter()
        .filter(|flag| save.is_flag_set(flag))
        .cloned()
        .collect()
}

/// Compute the current per-object completion for a parsed save.
pub(crate) fn completed_objects(save: &SaveFile, objects: &MapObjects) -> CompletedObjects {
    CompletedObjects {
        shrines: flags_set(save, &objects.shrines),
        towers: flags_set(save, &objects.towers),
        memories: flags_set(save, &objects.memories),
        koroks: flags_set(save, &objects.koroks),
        locations: flags_set(save, &objects.locations),
        enemies: flags_set(save, &objects.enemies),
        divine_beasts: flags_set(save, &objects.divine_beasts),
        treasures: objects
            .treasures
            .iter()
            .copied()
            .filter(|&hash| save.is_key_set(hash))
            .collect(),
    }
}

/// List the machine-local slots that are currently present on disk.
pub async fn list_machine_slots(pool: &SqlitePool) -> Result<Vec<SlotSummary>, SlotError> {
    list_slots(pool, SlotScope::Machine).await
}

/// List a profile's stored slots.
pub async fn list_profile_slots(
    pool: &SqlitePool,
    profile_id: &str,
) -> Result<Vec<SlotSummary>, SlotError> {
    list_slots(pool, SlotScope::Profile(profile_id)).await
}

async fn list_slots(
    pool: &SqlitePool,
    scope: SlotScope<'_>,
) -> Result<Vec<SlotSummary>, SlotError> {
    let statement = format!(
        "SELECT l.slot_index, l.label, l.caption_location, l.caption_map_area, \
         COALESCE((SELECT s.captured_at FROM snapshots s WHERE s.slot_id = l.id \
         ORDER BY s.captured_at DESC, s.ingested_at DESC LIMIT 1), 0) AS saved_at \
         FROM save_slots l WHERE {} AND l.present = 1 ORDER BY l.slot_index",
        scope.clause()
    );

    let mut query = sqlx::query(AssertSqlSafe(statement));
    if let Some(profile_id) = scope.profile_id() {
        query = query.bind(profile_id);
    }
    let rows = query.fetch_all(pool).await?;

    Ok(rows
        .into_iter()
        .map(|row| {
            let slot = row.get::<i64, _>("slot_index") as usize;
            SlotSummary {
                slot,
                label: row.get("label"),
                location: row.get("caption_location"),
                map_area: row.get("caption_map_area"),
                saved_at: row.get::<i64, _>("saved_at").max(0) as u64,
                caption_url: scope.caption_url(slot),
            }
        })
        .collect())
}

pub async fn load_machine_slot(pool: &SqlitePool, slot: usize) -> Result<SlotDetail, SlotError> {
    load_slot(pool, SlotScope::Machine, slot).await
}

pub async fn load_profile_slot(
    pool: &SqlitePool,
    profile_id: &str,
    slot: usize,
) -> Result<SlotDetail, SlotError> {
    load_slot(pool, SlotScope::Profile(profile_id), slot).await
}

async fn load_slot(
    pool: &SqlitePool,
    scope: SlotScope<'_>,
    slot: usize,
) -> Result<SlotDetail, SlotError> {
    let statement = format!(
        "SELECT l.slot_index, l.label, l.caption_location, l.caption_map_area, \
         sn.stats_json, sn.completion_summary_json, sn.captured_at \
         FROM save_slots l JOIN snapshots sn ON sn.id = ( \
             SELECT id FROM snapshots WHERE slot_id = l.id \
             ORDER BY captured_at DESC, ingested_at DESC LIMIT 1) \
         WHERE {} AND l.slot_index = ? AND l.present = 1",
        scope.clause()
    );

    let mut query = sqlx::query(AssertSqlSafe(statement));
    if let Some(profile_id) = scope.profile_id() {
        query = query.bind(profile_id);
    }
    let row = query
        .bind(slot as i64)
        .fetch_optional(pool)
        .await?
        .ok_or(SlotError::SlotNotFound)?;

    let stats: SaveStats = serde_json::from_str(row.get("stats_json"))
        .map_err(|error| SlotError::Unreadable(error.to_string()))?;
    let completion: CompletionSummary = serde_json::from_str(row.get("completion_summary_json"))
        .map_err(|error| SlotError::Unreadable(error.to_string()))?;
    let slot_index = row.get::<i64, _>("slot_index") as usize;

    Ok(SlotDetail {
        summary: SlotSummary {
            slot: slot_index,
            label: row.get("label"),
            location: row.get("caption_location"),
            map_area: row.get("caption_map_area"),
            saved_at: row.get::<i64, _>("captured_at").max(0) as u64,
            caption_url: scope.caption_url(slot_index),
        },
        stats,
        completion,
    })
}

pub async fn load_machine_map(pool: &SqlitePool, slot: usize) -> Result<MapCompletion, SlotError> {
    load_map(pool, SlotScope::Machine, slot).await
}

pub async fn load_profile_map(
    pool: &SqlitePool,
    profile_id: &str,
    slot: usize,
) -> Result<MapCompletion, SlotError> {
    load_map(pool, SlotScope::Profile(profile_id), slot).await
}

async fn load_map(
    pool: &SqlitePool,
    scope: SlotScope<'_>,
    slot: usize,
) -> Result<MapCompletion, SlotError> {
    let completion_json = completion_json(pool, scope, slot).await?;
    // Current per-object completion is derived data; a decode failure is a
    // stored-data problem, not a reason to fail the whole map request.
    let completed: CompletedObjects = match serde_json::from_str(&completion_json) {
        Ok(completed) => completed,
        Err(error) => {
            tracing::warn!(slot, %error, "could not decode stored completion; serving empty");
            CompletedObjects::default()
        }
    };
    Ok(MapCompletion { slot, completed })
}

pub async fn load_machine_completion(
    pool: &SqlitePool,
    slot: usize,
) -> Result<CompletionSummary, SlotError> {
    load_completion(pool, SlotScope::Machine, slot).await
}

pub async fn load_profile_completion(
    pool: &SqlitePool,
    profile_id: &str,
    slot: usize,
) -> Result<CompletionSummary, SlotError> {
    load_completion(pool, SlotScope::Profile(profile_id), slot).await
}

async fn load_completion(
    pool: &SqlitePool,
    scope: SlotScope<'_>,
    slot: usize,
) -> Result<CompletionSummary, SlotError> {
    let statement = format!(
        "SELECT sn.completion_summary_json FROM save_slots l JOIN snapshots sn ON sn.id = ( \
             SELECT id FROM snapshots WHERE slot_id = l.id \
             ORDER BY captured_at DESC, ingested_at DESC LIMIT 1) \
         WHERE {} AND l.slot_index = ? AND l.present = 1",
        scope.clause()
    );
    let mut query = sqlx::query(AssertSqlSafe(statement));
    if let Some(profile_id) = scope.profile_id() {
        query = query.bind(profile_id);
    }
    let json: String = query
        .bind(slot as i64)
        .fetch_optional(pool)
        .await?
        .map(|row| row.get("completion_summary_json"))
        .ok_or(SlotError::SlotNotFound)?;

    serde_json::from_str(&json).map_err(|error| SlotError::Unreadable(error.to_string()))
}

async fn completion_json(
    pool: &SqlitePool,
    scope: SlotScope<'_>,
    slot: usize,
) -> Result<String, SlotError> {
    let statement = format!(
        "SELECT completion_json FROM save_slots l \
         WHERE {} AND l.slot_index = ? AND l.present = 1",
        scope.clause()
    );
    let mut query = sqlx::query(AssertSqlSafe(statement));
    if let Some(profile_id) = scope.profile_id() {
        query = query.bind(profile_id);
    }
    query
        .bind(slot as i64)
        .fetch_optional(pool)
        .await?
        .map(|row| row.get("completion_json"))
        .ok_or(SlotError::SlotNotFound)
}

pub async fn load_machine_caption(pool: &SqlitePool, slot: usize) -> Result<Vec<u8>, SlotError> {
    load_caption(pool, SlotScope::Machine, slot).await
}

pub async fn load_profile_caption(
    pool: &SqlitePool,
    profile_id: &str,
    slot: usize,
) -> Result<Vec<u8>, SlotError> {
    load_caption(pool, SlotScope::Profile(profile_id), slot).await
}

async fn load_caption(
    pool: &SqlitePool,
    scope: SlotScope<'_>,
    slot: usize,
) -> Result<Vec<u8>, SlotError> {
    let statement = format!(
        "SELECT caption_image FROM save_slots l \
         WHERE {} AND l.slot_index = ? AND l.present = 1",
        scope.clause()
    );
    let mut query = sqlx::query(AssertSqlSafe(statement));
    if let Some(profile_id) = scope.profile_id() {
        query = query.bind(profile_id);
    }
    let row = query
        .bind(slot as i64)
        .fetch_optional(pool)
        .await?
        .ok_or(SlotError::SlotNotFound)?;
    row.get::<Option<Vec<u8>>, _>("caption_image")
        .ok_or(SlotError::CaptionNotFound)
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::path::{Path, PathBuf};

    use wayfinder_save::{HEADER_LEN, RECORD_LEN, SENTINEL, SaveFile, VERSION_1_9_0, keys};

    pub fn temp_root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "wayfinder-saves-{name}-{}-{}",
            std::process::id(),
            uuid::Uuid::now_v7()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    pub fn string_words(value: &str) -> Vec<u32> {
        let mut bytes = value.as_bytes().to_vec();
        while bytes.len() % 4 != 0 {
            bytes.push(0);
        }
        bytes
            .chunks_exact(4)
            .map(|chunk| u32::from_le_bytes(chunk.try_into().expect("four bytes")))
            .collect()
    }

    pub fn build_save(records: &[(u32, u32)]) -> Vec<u8> {
        let mut out = Vec::with_capacity(HEADER_LEN + records.len() * RECORD_LEN + 4);
        out.extend_from_slice(&VERSION_1_9_0.to_le_bytes());
        out.extend_from_slice(&SENTINEL.to_le_bytes());
        out.extend_from_slice(&1u32.to_le_bytes());
        for (key, value) in records {
            out.extend_from_slice(&key.to_le_bytes());
            out.extend_from_slice(&value.to_le_bytes());
        }
        out.extend_from_slice(&SENTINEL.to_le_bytes());
        out
    }

    pub fn valid_save() -> Vec<u8> {
        let mut records = vec![
            (keys::PLAY_TIME, 50_166),
            (keys::CURRENT_RUPEE, 1_084),
            (keys::CURRENT_HART, 16),
            (keys::MAX_HART, 16),
            (keys::STAMINA_MAX, 1_200.0f32.to_bits()),
            (keys::STAMINA_CURRENT_MAX, 476.309_33f32.to_bits()),
            (keys::PLAYER_SAVE_POS, 12.5f32.to_bits()),
            (keys::PLAYER_SAVE_POS, (-3.25f32).to_bits()),
            (keys::PLAYER_SAVE_POS, 0.0f32.to_bits()),
            (SaveFile::completion_key("Clear_Dungeon015"), 1),
        ];
        records.extend(
            string_words("G-6")
                .into_iter()
                .map(|word| (keys::PLAYER_SAVE_POS_MAP_NAME, word)),
        );
        records.extend(
            string_words("MainField")
                .into_iter()
                .map(|word| (keys::PLAYER_SAVE_POS_MAP_TYPE, word)),
        );
        build_save(&records)
    }

    pub fn caption_save() -> Vec<u8> {
        let mut records = vec![(keys::CAPTION_SAVED_AT, 1_794_087_241)];
        records.extend(
            string_words("FaronHatago000")
                .into_iter()
                .map(|word| (keys::CAPTION_LOCATION, word)),
        );
        records.extend(
            string_words("MapArea_WestHateru")
                .into_iter()
                .map(|word| (keys::CAPTION_MAP_AREA, word)),
        );
        build_save(&records)
    }

    pub fn write_slot(
        profile: &Path,
        slot: usize,
        save: &[u8],
        caption: Option<&[u8]>,
        image: Option<&[u8]>,
    ) -> PathBuf {
        let slot_dir = profile.join(slot.to_string());
        std::fs::create_dir_all(&slot_dir).unwrap();
        std::fs::write(slot_dir.join("game_data.sav"), save).unwrap();
        if let Some(caption) = caption {
            std::fs::write(slot_dir.join("caption.sav"), caption).unwrap();
        }
        if let Some(image) = image {
            std::fs::write(slot_dir.join("caption.jpg"), image).unwrap();
        }
        slot_dir
    }
}
