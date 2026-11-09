//! Save-slot discovery and summaries for the local/LAN API.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::config::Config;
use wayfinder_save::{CompletionSummary, SaveCaption, SaveFile, SaveStats};

/// List entry returned by `GET /api/slots`.
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
#[derive(Debug, Default, Serialize)]
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
    /// No discovered slot has this number.
    SlotNotFound,
    /// The slot exists but has no readable caption screenshot.
    CaptionNotFound,
    /// The `game_data.sav` exists but could not be read or parsed.
    Unreadable(std::io::Error),
}

impl std::fmt::Display for SlotError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SlotNotFound => formatter.write_str("save slot not found"),
            Self::CaptionNotFound => formatter.write_str("caption not found"),
            Self::Unreadable(error) => write!(formatter, "save file could not be parsed: {error}"),
        }
    }
}

impl std::error::Error for SlotError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Unreadable(error) => Some(error),
            _ => None,
        }
    }
}

/// Parsed map-object dataset, reduced to what the server needs.
///
/// The dataset joins save flags to Zelda Dungeon markers. The server keeps the
/// `flag -> friendly name` join for caption labels and, per object type, the
/// dataset flags (and treasure hashes) used for per-object completion.
#[derive(Debug, Default)]
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

/// Enumerate saved slots with their caption metadata.
pub fn list_slots(config: &Config, objects: &MapObjects) -> Vec<SlotSummary> {
    config
        .save_slots()
        .into_iter()
        .map(|slot| summary_for(config, slot, objects))
        .collect()
}

/// Load a slot's metadata, player stats, and completion summary.
pub fn load_slot(
    config: &Config,
    slot: usize,
    objects: &MapObjects,
) -> Result<SlotDetail, SlotError> {
    let save = parse_save(config, slot)?;
    Ok(SlotDetail {
        summary: summary_for(config, slot, objects),
        stats: save.stats(),
        completion: save.completion_summary(),
    })
}

/// Load the dataset identifiers of every object completed in a slot.
pub fn load_map_completion(
    config: &Config,
    slot: usize,
    objects: &MapObjects,
) -> Result<MapCompletion, SlotError> {
    let save = parse_save(config, slot)?;
    Ok(MapCompletion {
        slot,
        completed: CompletedObjects {
            shrines: completed_flags(&save, &objects.shrines),
            towers: completed_flags(&save, &objects.towers),
            memories: completed_flags(&save, &objects.memories),
            koroks: completed_flags(&save, &objects.koroks),
            locations: completed_flags(&save, &objects.locations),
            enemies: completed_flags(&save, &objects.enemies),
            divine_beasts: completed_flags(&save, &objects.divine_beasts),
            treasures: objects
                .treasures
                .iter()
                .copied()
                .filter(|&hash| save.is_key_set(hash))
                .collect(),
        },
    })
}

fn completed_flags(save: &SaveFile, flags: &[String]) -> Vec<String> {
    flags
        .iter()
        .filter(|flag| save.is_flag_set(flag))
        .cloned()
        .collect()
}

/// Load only the completion summary, kept for `/api/completion?slot=N`.
pub fn load_completion(config: &Config, slot: usize) -> Result<CompletionSummary, SlotError> {
    Ok(parse_save(config, slot)?.completion_summary())
}

/// Read the caption screenshot for a discovered slot.
pub fn load_caption(config: &Config, slot: usize) -> Result<Vec<u8>, SlotError> {
    let path = config
        .save_caption_image_path(slot)
        .ok_or(SlotError::SlotNotFound)?;
    std::fs::read(path).map_err(|_| SlotError::CaptionNotFound)
}

fn parse_save(config: &Config, slot: usize) -> Result<SaveFile, SlotError> {
    let path = config.save_slot_path(slot).ok_or(SlotError::SlotNotFound)?;
    SaveFile::from_path(path).map_err(SlotError::Unreadable)
}

fn summary_for(config: &Config, slot: usize, objects: &MapObjects) -> SlotSummary {
    let caption = config
        .save_caption_path(slot)
        .and_then(read_caption)
        .unwrap_or_default();
    let label = objects
        .for_location(&caption.location)
        .map(str::to_owned)
        .unwrap_or_else(|| format!("Slot {slot}"));

    SlotSummary {
        slot,
        label,
        location: caption.location,
        map_area: caption.map_area,
        saved_at: caption.saved_at,
        caption_url: format!("/api/slots/{slot}/caption"),
    }
}

fn read_caption(path: std::path::PathBuf) -> Option<SaveCaption> {
    SaveFile::from_path(path).ok().map(|save| save.caption())
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::path::{Path, PathBuf};

    use wayfinder_save::{HEADER_LEN, RECORD_LEN, SENTINEL, SaveFile, VERSION_1_9_0, keys};

    pub fn temp_root(name: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("wayfinder-saves-{name}-{}", std::process::id()));
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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::test_support::{build_save, caption_save, temp_root, valid_save, write_slot};
    use super::*;
    use crate::config::Mode;

    fn test_config(mode: Mode, save_dir: Option<PathBuf>) -> Config {
        Config {
            mode,
            bind: "127.0.0.1:0".parse().unwrap(),
            save_dir,
            control_addr: "127.0.0.1:7777".parse().unwrap(),
            data_dir: PathBuf::from("data"),
            cookie_secure: false,
        }
    }

    #[test]
    fn resolves_friendly_labels() {
        let labels = MapObjects::from_json(
            br#"{
                "locations": [
                    {"flag": "Location_FaronHatago000", "match_name": "Dueling Peaks Stable"}
                ],
                "shrines": [
                    {"flag": "Clear_Dungeon015", "name": "Toto Sah Shrine"}
                ]
            }"#,
        )
        .unwrap();

        assert_eq!(
            labels.for_location("FaronHatago000"),
            Some("Dueling Peaks Stable")
        );
        assert_eq!(labels.for_location("Dungeon015"), Some("Toto Sah Shrine"));
        assert_eq!(
            labels.for_location("Location_FaronHatago000"),
            Some("Dueling Peaks Stable")
        );
        assert_eq!(labels.for_location(""), None);
        assert_eq!(labels.for_location("Missing000"), None);
    }

    #[test]
    fn lists_slots_with_caption_metadata_and_fallbacks() {
        let root = temp_root("list");
        write_slot(
            &root.join("save_0"),
            0,
            &valid_save(),
            Some(&caption_save()),
            Some(b"jpeg"),
        );
        write_slot(&root.join("save_0"), 2, &valid_save(), None, None);

        let config = test_config(Mode::Local, Some(root.clone()));
        let labels = MapObjects::from_json(
            br#"{"locations":[{"flag":"Location_FaronHatago000","match_name":"Dueling Peaks Stable"}]}"#,
        )
        .unwrap();
        let slots = list_slots(&config, &labels);

        assert_eq!(slots.len(), 2);
        assert_eq!(slots[0].slot, 0);
        assert_eq!(slots[0].label, "Dueling Peaks Stable");
        assert_eq!(slots[0].location, "FaronHatago000");
        assert_eq!(slots[0].map_area, "MapArea_WestHateru");
        assert_eq!(slots[0].saved_at, 1_794_087_241);
        assert_eq!(slots[0].caption_url, "/api/slots/0/caption");
        assert_eq!(slots[1].slot, 2);
        assert_eq!(slots[1].label, "Slot 2");
        assert_eq!(slots[1].saved_at, 0);

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn loads_stats_and_completion_for_a_slot() {
        let root = temp_root("detail");
        write_slot(
            &root.join("save_0"),
            0,
            &valid_save(),
            Some(&caption_save()),
            Some(b"jpeg"),
        );

        let config = test_config(Mode::Local, Some(root.clone()));
        let detail = load_slot(&config, 0, &MapObjects::default()).unwrap();

        assert_eq!(detail.summary.slot, 0);
        assert_eq!(detail.stats.play_time_seconds, 50_166);
        assert_eq!(detail.stats.rupees, 1_084);
        assert_eq!(detail.stats.hearts.current, 4.0);
        assert_eq!(detail.stats.stamina.max, 1_200.0);
        assert_eq!(detail.stats.location.map, "G-6");
        assert_eq!(detail.completion.shrines.completed, 1);
        assert_eq!(load_completion(&config, 0).unwrap().shrines.completed, 1);

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn loads_per_object_map_completion() {
        const KOROK: &str = "MainField_Npc_HiddenKorokGround_3148789636";
        const CHEST: u32 = 2_304_095;

        let records = vec![
            (SaveFile::completion_key("Clear_Dungeon015"), 1),
            (SaveFile::completion_key("MapTower_07"), 1),
            (SaveFile::completion_key(KOROK), 1),
            (SaveFile::completion_key("Clear_RemainsWater"), 1),
            (CHEST, 1),
            // Present but zero: not complete.
            (SaveFile::completion_key("Clear_Dungeon016"), 0),
        ];

        let root = temp_root("map-completion");
        write_slot(&root.join("save_0"), 0, &build_save(&records), None, None);
        let config = test_config(Mode::Local, Some(root.clone()));

        let objects = MapObjects::from_json(
            br#"{
                "shrines": [
                    {"flag": "Clear_Dungeon015", "name": "Toto Sah Shrine"},
                    {"flag": "Clear_Dungeon016", "name": "Other Shrine"}
                ],
                "towers": [{"flag": "MapTower_07", "name": "Tower"}],
                "koroks": [{"flag": "MainField_Npc_HiddenKorokGround_3148789636"}],
                "divineBeasts": [
                    {"flag": "Clear_RemainsWater", "name": "Divine Beast Vah Ruta"},
                    {"flag": "Clear_RemainsFire", "name": "Divine Beast Vah Rudania"}
                ],
                "treasures": [{"id": "Treasure0001", "hash": 2304095}]
            }"#,
        )
        .unwrap();

        let completion = load_map_completion(&config, 0, &objects).unwrap();
        assert_eq!(completion.slot, 0);
        assert_eq!(completion.completed.shrines, vec!["Clear_Dungeon015"]);
        assert_eq!(completion.completed.towers, vec!["MapTower_07"]);
        assert_eq!(completion.completed.koroks, vec![KOROK]);
        assert_eq!(
            completion.completed.divine_beasts,
            vec!["Clear_RemainsWater"]
        );
        assert_eq!(completion.completed.treasures, vec![CHEST]);
        assert!(completion.completed.memories.is_empty());
        assert!(completion.completed.locations.is_empty());
        assert!(completion.completed.enemies.is_empty());

        assert!(matches!(
            load_map_completion(&config, 9, &objects),
            Err(SlotError::SlotNotFound)
        ));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn distinguishes_missing_and_corrupt_slots() {
        let root = temp_root("errors");
        write_slot(&root.join("save_0"), 1, b"not a save", None, None);
        let config = test_config(Mode::Local, Some(root.clone()));

        assert!(matches!(
            load_slot(&config, 9, &MapObjects::default()),
            Err(SlotError::SlotNotFound)
        ));
        assert!(matches!(
            load_slot(&config, 1, &MapObjects::default()),
            Err(SlotError::Unreadable(_))
        ));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn serves_caption_images_for_discovered_slots() {
        let root = temp_root("caption");
        write_slot(
            &root.join("save_0"),
            0,
            &build_save(&[]),
            Some(&caption_save()),
            Some(b"jpeg-bytes"),
        );
        write_slot(&root.join("save_0"), 1, &valid_save(), None, None);
        let config = test_config(Mode::Local, Some(root.clone()));

        assert_eq!(load_caption(&config, 0).unwrap(), b"jpeg-bytes");
        assert!(matches!(
            load_caption(&config, 1),
            Err(SlotError::CaptionNotFound)
        ));
        assert!(matches!(
            load_caption(&config, 9),
            Err(SlotError::SlotNotFound)
        ));

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn hub_mode_exposes_no_slots() {
        let root = temp_root("hub");
        write_slot(
            &root.join("save_0"),
            0,
            &valid_save(),
            Some(&caption_save()),
            Some(b"jpeg"),
        );
        let config = test_config(Mode::Hub, Some(root.clone()));

        assert!(list_slots(&config, &MapObjects::default()).is_empty());
        assert!(matches!(
            load_slot(&config, 0, &MapObjects::default()),
            Err(SlotError::SlotNotFound)
        ));
        assert!(matches!(
            load_caption(&config, 0),
            Err(SlotError::SlotNotFound)
        ));

        std::fs::remove_dir_all(root).unwrap();
    }
}
