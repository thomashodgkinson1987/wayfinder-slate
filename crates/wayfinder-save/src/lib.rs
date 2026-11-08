//! Parser for *The Legend of Zelda: Breath of the Wild* Switch save files.
//!
//! A BotW Switch `game_data.sav` is a flat table of 32-bit keys and 32-bit
//! values:
//!
//! ```text
//! u32 version        (0x00004730 for the 1.9.0 saves seen so far)
//! u32 marker         (0xFFFFFFFF)
//! u32 unknown        (1 in observed saves)
//! [u32 key, u32 val] * N
//! u32 sentinel       (0xFFFFFFFF)
//! ```
//!
//! Keys are IEEE CRC32 hashes of internal save-field names. Multi-word fields
//! (strings, vectors, arrays) repeat the same key once per 32-bit word. This
//! crate intentionally knows nothing about SQLite, HTTP, or the filesystem: it
//! turns bytes into a queryable save and leaves policy to callers.

pub mod keys;

use std::collections::HashMap;
use std::path::Path;

use binrw::BinRead;
use binrw::io::Cursor;

/// Length of the fixed save header.
pub const HEADER_LEN: usize = 12;
/// Length of one key/value record.
pub const RECORD_LEN: usize = 8;
/// The terminator at the end of the file.
pub const SENTINEL: u32 = 0xFFFF_FFFF;
/// Smallest valid save: header + sentinel.
pub const MIN_FILE_LEN: usize = HEADER_LEN + 4;
/// Save version used by BotW Switch 1.9.0, the primary target.
pub const VERSION_1_9_0: u32 = 0x0000_4730;

/// One raw key/value entry. Duplicate keys are kept in file order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Record {
    pub key: u32,
    pub value: u32,
}

#[derive(Debug, thiserror::Error)]
pub enum SaveError {
    #[error("save is {len} bytes, smaller than the {min} byte minimum")]
    TooSmall { len: usize, min: usize },

    #[error("Wii U big-endian saves are not supported yet")]
    UnsupportedEndianness,

    #[error("not a Breath of the Wild save: header marker is {0:#010x}, expected 0xffffffff")]
    BadMarker(u32),

    #[error(
        "save is truncated: {remaining} bytes after the header do not form 8-byte records and a sentinel"
    )]
    InvalidLength { remaining: usize },

    #[error("save is missing the trailing 0xffffffff sentinel")]
    MissingSentinel,

    #[error("failed to decode save header: {0}")]
    Decode(#[from] binrw::Error),
}

/// A parsed save file and its key index.
#[derive(Debug, Clone)]
pub struct SaveFile {
    version: u32,
    header_unknown: u32,
    records: Vec<Record>,
    index: HashMap<u32, Vec<u32>>,
    raw: Vec<u8>,
}

#[derive(BinRead)]
#[br(little)]
struct Header {
    version: u32,
    marker: u32,
    unknown: u32,
}

impl SaveFile {
    /// Parse save bytes. The input is retained so callers can snapshot or
    /// re-parse it after schema updates.
    pub fn parse(raw: impl Into<Vec<u8>>) -> Result<Self, SaveError> {
        let raw = raw.into();
        if raw.len() < MIN_FILE_LEN {
            return Err(SaveError::TooSmall {
                len: raw.len(),
                min: MIN_FILE_LEN,
            });
        }

        // A Wii U save stores the header big-endian. Detect this before the
        // little-endian header read so the error is actionable.
        let le_version = u32::from_le_bytes(raw[0..4].try_into().expect("four bytes"));
        if le_version == VERSION_1_9_0.swap_bytes() {
            return Err(SaveError::UnsupportedEndianness);
        }

        let header = Header::read(&mut Cursor::new(&raw[..HEADER_LEN]))?;
        if header.marker != SENTINEL {
            return Err(SaveError::BadMarker(header.marker));
        }

        let data_end = raw.len() - 4;
        if raw[data_end..] != SENTINEL.to_le_bytes() {
            return Err(SaveError::MissingSentinel);
        }
        let data_len = data_end - HEADER_LEN;
        if data_len % RECORD_LEN != 0 {
            return Err(SaveError::InvalidLength {
                remaining: data_len % RECORD_LEN,
            });
        }

        let record_count = data_len / RECORD_LEN;
        let mut records = Vec::with_capacity(record_count);
        let mut index: HashMap<u32, Vec<u32>> = HashMap::with_capacity(record_count);

        for chunk in raw[HEADER_LEN..data_end].chunks_exact(RECORD_LEN) {
            let key = u32::from_le_bytes(chunk[0..4].try_into().expect("four bytes"));
            let value = u32::from_le_bytes(chunk[4..8].try_into().expect("four bytes"));
            records.push(Record { key, value });
            index.entry(key).or_default().push(value);
        }

        Ok(Self {
            version: header.version,
            header_unknown: header.unknown,
            records,
            index,
            raw,
        })
    }

    /// Read and parse a save file.
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, std::io::Error> {
        Self::parse(std::fs::read(path)?).map_err(std::io::Error::other)
    }

    /// Return the CRC32 key for an internal save-field name.
    pub fn key_id(name: &str) -> u32 {
        crc32fast::hash(name.as_bytes())
    }

    pub fn version(&self) -> u32 {
        self.version
    }

    pub fn header_unknown(&self) -> u32 {
        self.header_unknown
    }

    pub fn raw(&self) -> &[u8] {
        &self.raw
    }

    pub fn records(&self) -> &[Record] {
        &self.records
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    pub fn unique_key_count(&self) -> usize {
        self.index.len()
    }

    pub fn contains_key(&self, key: u32) -> bool {
        self.index.contains_key(&key)
    }

    /// All 32-bit words for `key`, in file order. Multi-word fields repeat the
    /// same key once per word.
    pub fn values(&self, key: u32) -> &[u32] {
        self.index.get(&key).map(Vec::as_slice).unwrap_or(&[])
    }

    /// First 32-bit word for `key`.
    pub fn value(&self, key: u32) -> Option<u32> {
        self.values(key).first().copied()
    }

    pub fn get_u32(&self, key: u32) -> Option<u32> {
        self.value(key)
    }

    pub fn get_i32(&self, key: u32) -> Option<i32> {
        self.value(key).map(|value| value as i32)
    }

    pub fn get_bool(&self, key: u32) -> Option<bool> {
        self.value(key).map(|value| value != 0)
    }

    pub fn get_f32(&self, key: u32) -> Option<f32> {
        self.value(key).map(f32::from_bits)
    }

    pub fn get_f32s(&self, key: u32) -> Vec<f32> {
        self.values(key)
            .iter()
            .copied()
            .map(f32::from_bits)
            .collect()
    }

    pub fn get_vec3f(&self, key: u32) -> Option<[f32; 3]> {
        let values = self.values(key);
        (values.len() >= 3).then(|| {
            [
                f32::from_bits(values[0]),
                f32::from_bits(values[1]),
                f32::from_bits(values[2]),
            ]
        })
    }

    /// Decode a fixed-width, NUL-padded UTF-8 string stored as repeated words.
    pub fn get_string(&self, key: u32) -> Option<String> {
        let values = self.values(key);
        if values.is_empty() {
            return None;
        }
        let mut bytes = Vec::with_capacity(values.len() * 4);
        for value in values {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        if let Some(nul) = bytes.iter().position(|byte| *byte == 0) {
            bytes.truncate(nul);
        }
        Some(String::from_utf8_lossy(&bytes).into_owned())
    }

    /// Iterate keys whose records contain at least one non-zero word.
    pub fn nonzero_keys(&self) -> impl Iterator<Item = u32> + '_ {
        self.index
            .iter()
            .filter(|(_, values)| values.iter().any(|value| *value != 0))
            .map(|(key, _)| *key)
    }

    /// Compute CRC32 for a completion flag name.
    pub fn completion_key(name: &str) -> u32 {
        Self::key_id(name)
    }
}

/// Total completable shrines tracked by `Clear_Dungeon000..135`.
pub const SHRINE_TOTAL: usize = 136;
/// Total Sheikah towers tracked by `MapTower_01..15`.
pub const TOWER_TOTAL: usize = 15;
/// Total Korok seeds tracked by the counter field.
pub const KOROK_TOTAL: usize = 900;
/// Total picture memories tracked by `IsGet_MemoryPhoto_000..013`.
pub const MEMORY_TOTAL: usize = 14;

/// Completion summary for a save file.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CompletionSummary {
    pub shrines: CompletionCount,
    pub towers: CompletionCount,
    pub koroks: CompletionCount,
    pub memories: CompletionCount,
    pub locations: CompletionCount,
    pub enemies: CompletionCount,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CompletionCount {
    pub total: usize,
    pub completed: usize,
    pub percentage: f32,
}

impl CompletionCount {
    pub fn new(total: usize, completed: usize) -> Self {
        let completed = completed.min(total);
        let percentage = if total > 0 {
            completed as f32 / total as f32 * 100.0
        } else {
            0.0
        };
        Self {
            total,
            completed,
            percentage,
        }
    }
}

impl SaveFile {
    /// Check if a completion flag is set (non-zero).
    pub fn is_flag_set(&self, flag: &str) -> bool {
        let key = Self::completion_key(flag);
        self.get_u32(key).map(|v| v != 0).unwrap_or(false)
    }

    /// Get completion summary for all tracked object types.
    pub fn completion_summary(&self) -> CompletionSummary {
        // Shrines: Clear_Dungeon000..135 (136 completable)
        let shrine_completed = (0..SHRINE_TOTAL)
            .filter(|i| self.is_flag_set(&format!("Clear_Dungeon{:03}", i)))
            .count();
        // Towers: MapTower_01..15
        let tower_completed = (1..=TOWER_TOTAL)
            .filter(|i| self.is_flag_set(&format!("MapTower_{:02}", i)))
            .count();
        // Koroks: MainField_Npc_HiddenKorok* (900)
        let korok_completed = self.korok_completed();
        // Memories: IsGet_MemoryPhoto_000..013 (14)
        let memory_completed = (0..MEMORY_TOTAL)
            .filter(|i| self.is_flag_set(&format!("IsGet_MemoryPhoto_{:03}", i)))
            .count();
        // Locations and enemies use their flag-list lengths as totals so the
        // summary cannot drift from the data.
        let (location_completed, location_total) = self.location_progress();
        let (enemy_completed, enemy_total) = self.enemy_progress();

        CompletionSummary {
            shrines: CompletionCount::new(SHRINE_TOTAL, shrine_completed),
            towers: CompletionCount::new(TOWER_TOTAL, tower_completed),
            koroks: CompletionCount::new(KOROK_TOTAL, korok_completed),
            memories: CompletionCount::new(MEMORY_TOTAL, memory_completed),
            locations: CompletionCount::new(location_total, location_completed),
            enemies: CompletionCount::new(enemy_total, enemy_completed),
        }
    }

    fn korok_completed(&self) -> usize {
        // Use the counter field as a proxy since we don't embed all 900 flag names.
        // The static dataset should be used for precise per-Korok tracking.
        self.get_u32(keys::HIDDEN_KOROK_NUMBER)
            .map(|value| value as usize)
            .unwrap_or(0)
    }

    fn location_progress(&self) -> (usize, usize) {
        // Static list of 187 Location_* flag names from marcrobledo data.
        const LOCATION_FLAGS: &[&str] = &[
            "Location_AdeyaVillage",
            "Location_AkkareBigBridge",
            "Location_AkkareGarrison",
            "Location_AkkareZhai",
            "Location_AkkareZhaiBridge",
            "Location_AncientLabo",
            "Location_Assassin",
            "Location_BigBrotherBridge",
            "Location_BigTree",
            "Location_BraveFountain",
            "Location_BrosRock",
            "Location_CelebrationPark",
            "Location_CentralExchange",
            "Location_ChirakaVillage",
            "Location_Cohorint",
            "Location_Cokiri",
            "Location_Corosseo",
            "Location_DeathMountainHatago",
            "Location_DeathMountain_Entrance",
            "Location_DodoraBridge",
            "Location_DragonMarsh",
            "Location_EastDam",
            "Location_EastGerudoRuins",
            "Location_EastPostTownRuin",
            "Location_Execution",
            "Location_EzzuBridge",
            "Location_FaronHatago000",
            "Location_FaronHatago001",
            "Location_FaronHatago002",
            "Location_FironeForestEntrance",
            "Location_FlamingoSpa",
            "Location_FloriaBridge",
            "Location_FlowerSandbank",
            "Location_ForestHatago",
            "Location_FrontGatePosttown",
            "Location_GameBalling",
            "Location_GameBitalockGolf",
            "Location_GameGutsCriff",
            "Location_GameMarathon",
            "Location_GameShiledSurf",
            "Location_GameYabusame",
            "Location_GameZarashiRace",
            "Location_GarakishiVillage",
            "Location_GatakaraGarrison",
            "Location_Gerudo",
            "Location_GerudoCanyon_Entrance",
            "Location_GerudoDesert_Entrance",
            "Location_GerudoFossil",
            "Location_GerudoHatago",
            "Location_Goron",
            "Location_GoronBridge",
            "Location_GutinizaPlainTomb",
            "Location_HachijoBridge",
            "Location_Hateno",
            "Location_HatenoGate",
            "Location_HatenoLabo",
            "Location_HeburaFossil",
            "Location_HeburaLodge",
            "Location_HopesPlateau",
            "Location_HyralBridge",
            "Location_HyruleCastle",
            "Location_HyruleCastleTown",
            "Location_HyruleCastle_Hall_0",
            "Location_HyruleCastle_Hall_1",
            "Location_HyruleCastle_Hall_2",
            "Location_HyruleCastle_Room_0",
            "Location_HyruleCastle_Room_1",
            "Location_HyruleCastle_Room_10",
            "Location_HyruleCastle_Room_11",
            "Location_HyruleCastle_Room_2",
            "Location_HyruleCastle_Room_3",
            "Location_HyruleCastle_Room_5",
            "Location_HyruleCastle_Room_6",
            "Location_HyruleCastle_Room_7",
            "Location_HyruleCastle_Room_8",
            "Location_HyruleCastle_Room_9",
            "Location_HyruleCentralPlace",
            "Location_HyruleDepthHatago",
            "Location_HyruleGarrison",
            "Location_HyruleGround",
            "Location_HyruleLabo",
            "Location_HyrulePark",
            "Location_HyrulePlace",
            "Location_HyruleTemple",
            "Location_HyruleTower",
            "Location_HyruleWater",
            "Location_HyruleWestHatago",
            "Location_IceHouse",
            "Location_JogoBridge",
            "Location_JokuBridge",
            "Location_Kakariko",
            "Location_KaturaCastle",
            "Location_KingValley",
            "Location_KingfisherSpa",
            "Location_KochuBridge",
            "Location_KumSpa",
            "Location_KunaiBridge",
            "Location_LanayruEastEntrance",
            "Location_LanayruWestEntrance",
            "Location_LeMarbeBridge",
            "Location_LightningPlateau",
            "Location_LittleBrotherBridge",
            "Location_LomeiIslands",
            "Location_LostForest",
            "Location_MapTower01",
            "Location_MapTower02",
            "Location_MapTower03",
            "Location_MapTower04",
            "Location_MapTower05",
            "Location_MapTower06",
            "Location_MapTower07",
            "Location_MapTower08",
            "Location_MapTower09",
            "Location_MapTower10",
            "Location_MapTower11",
            "Location_MapTower12",
            "Location_MapTower13",
            "Location_MapTower14",
            "Location_MapTower15",
            "Location_MaronSpring",
            "Location_MarritaExchange",
            "Location_MasazuBridge",
            "Location_MinakkareBridge",
            "Location_MinakkareBridgeEast",
            "Location_MinakkareBridgeWest",
            "Location_Mine",
            "Location_NantanCastle",
            "Location_NishojiBridge",
            "Location_NorthHatelHatago",
            "Location_Oasis",
            "Location_OrdinBridge",
            "Location_OrdinFossil",
            "Location_OsanjoBridge",
            "Location_PostTownRuin",
            "Location_PowerFountain",
            "Location_RemainsElectric",
            "Location_RemainsFire",
            "Location_RemainsWater",
            "Location_RemainsWind",
            "Location_RirikendoBridge",
            "Location_Rito",
            "Location_RitoHatago",
            "Location_RiverSideHatago",
            "Location_RonronCity",
            "Location_SaihokuCastle",
            "Location_SarjonBridge",
            "Location_ShichijoBridge",
            "Location_ShigonDam",
            "Location_ShijoBridge",
            "Location_ShinyarkiVillage",
            "Location_SnowStatue",
            "Location_SojijiPark",
            "Location_SotonkaBridge",
            "Location_SouthGerudoLegacy",
            "Location_SouthMine",
            "Location_StartPoint",
            "Location_StonePillers",
            "Location_SusukadiBridge",
            "Location_TabantaBridge",
            "Location_TabantaBridgeHatago",
            "Location_TabantaCrater",
            "Location_TabantaHatago",
            "Location_TabantaVillage",
            "Location_TamourHatago",
            "Location_TamourHatago2",
            "Location_TaserakaBridge",
            "Location_Taura",
            "Location_TimesShrine",
            "Location_ToriaBridge",
            "Location_TotsugeBridge",
            "Location_TurasuBridge",
            "Location_UMiiVillage",
            "Location_UMiiVillageShopBougu",
            "Location_UMiiVillageShopJewel",
            "Location_UMiiVillageShopYadoya",
            "Location_UMiiVillageShopYorozu",
            "Location_ValleyVillage",
            "Location_WeaponCureSpring",
            "Location_WeaponCureSpring_02",
            "Location_WeaponCureSpring_03",
            "Location_WeaponCureSpring_04",
            "Location_WetGarrison",
            "Location_WhiteZora",
            "Location_WisdomFountain",
            "Location_XekuBridge",
            "Location_YashinoLegacy",
            "Location_ZonauLegacy",
        ];
        let completed = LOCATION_FLAGS
            .iter()
            .filter(|flag| self.is_flag_set(flag))
            .count();
        (completed, LOCATION_FLAGS.len())
    }

    fn enemy_progress(&self) -> (usize, usize) {
        // Static list of 84 defeated enemy flags from marcrobledo data.
        const ENEMY_FLAGS: &[&str] = &[
            // 40 Hinox
            "MainField_IsDefeat_Enemy_Giant_Bone_1287559097",
            "MainField_IsDefeat_Enemy_Giant_Bone_1698864303",
            "MainField_IsDefeat_Enemy_Giant_Bone_1721655973",
            "MainField_IsDefeat_Enemy_Giant_Bone_2519547405",
            "MainField_IsDefeat_Enemy_Giant_Bone_2815854988",
            "MainField_IsDefeat_Enemy_Giant_Bone_2974080467",
            "MainField_IsDefeat_Enemy_Giant_Bone_3201775889",
            "MainField_IsDefeat_Enemy_Giant_Bone_3700264996",
            "MainField_IsDefeat_Enemy_Giant_Bone_4237208898",
            "MainField_IsDefeat_Enemy_Giant_Bone_AllDay_73882430",
            "MainField_IsDefeat_Enemy_Giant_Junior_1602515170",
            "MainField_IsDefeat_Enemy_Giant_Junior_1833514674",
            "MainField_IsDefeat_Enemy_Giant_Junior_1903632265",
            "MainField_IsDefeat_Enemy_Giant_Junior_3337886442",
            "MainField_IsDefeat_Enemy_Giant_Junior_3350013125",
            "MainField_IsDefeat_Enemy_Giant_Junior_373348007",
            "MainField_IsDefeat_Enemy_Giant_Junior_4099815924",
            "MainField_IsDefeat_Enemy_Giant_Middle_1266282060",
            "MainField_IsDefeat_Enemy_Giant_Middle_1384742674",
            "MainField_IsDefeat_Enemy_Giant_Middle_1925665429",
            "MainField_IsDefeat_Enemy_Giant_Middle_1949698080",
            "MainField_IsDefeat_Enemy_Giant_Middle_2083136285",
            "MainField_IsDefeat_Enemy_Giant_Middle_2472409077",
            "MainField_IsDefeat_Enemy_Giant_Middle_2530315670",
            "MainField_IsDefeat_Enemy_Giant_Middle_3272527560",
            "MainField_IsDefeat_Enemy_Giant_Middle_3572226454",
            "MainField_IsDefeat_Enemy_Giant_Middle_3627901807",
            "MainField_IsDefeat_Enemy_Giant_Middle_3637193649",
            "MainField_IsDefeat_Enemy_Giant_Middle_3706280648",
            "MainField_IsDefeat_Enemy_Giant_Middle_3744176720",
            "MainField_IsDefeat_Enemy_Giant_Middle_3831867213",
            "MainField_IsDefeat_Enemy_Giant_Middle_4080554254",
            "MainField_IsDefeat_Enemy_Giant_Middle_936329231",
            "MainField_IsDefeat_Enemy_Giant_Senior_1028264440",
            "MainField_IsDefeat_Enemy_Giant_Senior_2009832062",
            "MainField_IsDefeat_Enemy_Giant_Senior_2853871544",
            "MainField_IsDefeat_Enemy_Giant_Senior_3410381997",
            "MainField_IsDefeat_Enemy_Giant_Senior_3741497810",
            "MainField_IsDefeat_Enemy_Giant_Senior_3785639248",
            "MainField_IsDefeat_Enemy_Giant_Senior_851170738",
            // 40 Talus
            "MainField_IsDefeat_Enemy_Golem_Fire_1372356658",
            "MainField_IsDefeat_Enemy_Golem_Fire_2080410037",
            "MainField_IsDefeat_Enemy_Golem_Fire_2112489205",
            "MainField_IsDefeat_Enemy_Golem_Fire_3180486693",
            "MainField_IsDefeat_Enemy_Golem_Fire_4143112243",
            "MainField_IsDefeat_Enemy_Golem_Ice_1684711544",
            "MainField_IsDefeat_Enemy_Golem_Ice_189892196",
            "MainField_IsDefeat_Enemy_Golem_Ice_4007690024",
            "MainField_IsDefeat_Enemy_Golem_Ice_4087740554",
            "MainField_IsDefeat_Enemy_Golem_Ice_648092042",
            "MainField_IsDefeat_Enemy_Golem_Junior_139564765",
            "MainField_IsDefeat_Enemy_Golem_Junior_1450825465",
            "MainField_IsDefeat_Enemy_Golem_Junior_2009332164",
            "MainField_IsDefeat_Enemy_Golem_Junior_3610208070",
            "MainField_IsDefeat_Enemy_Golem_Junior_3688513041",
            "MainField_IsDefeat_Enemy_Golem_Junior_3707687090",
            "MainField_IsDefeat_Enemy_Golem_Junior_3817875409",
            "MainField_IsDefeat_Enemy_Golem_Junior_3873221297",
            "MainField_IsDefeat_Enemy_Golem_Junior_4097205930",
            "MainField_IsDefeat_Enemy_Golem_Junior_4234397911",
            "MainField_IsDefeat_Enemy_Golem_Junior_481698249",
            "MainField_IsDefeat_Enemy_Golem_Middle_1063952470",
            "MainField_IsDefeat_Enemy_Golem_Middle_2367737901",
            "MainField_IsDefeat_Enemy_Golem_Middle_2827830463",
            "MainField_IsDefeat_Enemy_Golem_Middle_3273877672",
            "MainField_IsDefeat_Enemy_Golem_Middle_3529818719",
            "MainField_IsDefeat_Enemy_Golem_Middle_3605751469",
            "MainField_IsDefeat_Enemy_Golem_Middle_372172601",
            "MainField_IsDefeat_Enemy_Golem_Middle_3812413946",
            "MainField_IsDefeat_Enemy_Golem_Middle_3900382371",
            "MainField_IsDefeat_Enemy_Golem_Middle_491623064",
            "MainField_IsDefeat_Enemy_Golem_Middle_853217538",
            "MainField_IsDefeat_Enemy_Golem_Middle_975119268",
            "MainField_IsDefeat_Enemy_Golem_Senior_1943962273",
            "MainField_IsDefeat_Enemy_Golem_Senior_2455625785",
            "MainField_IsDefeat_Enemy_Golem_Senior_2530932938",
            "MainField_IsDefeat_Enemy_Golem_Senior_2763213354",
            "MainField_IsDefeat_Enemy_Golem_Senior_321907823",
            "MainField_IsDefeat_Enemy_Golem_Senior_3869714669",
            "MainField_IsDefeat_Enemy_Golem_Senior_464316212",
            // 4 Molduga
            "MainField_IsDefeat_Enemy_Sandworm_3098676163",
            "MainField_IsDefeat_Enemy_Sandworm_3538536935",
            "MainField_IsDefeat_Enemy_Sandworm_3569739054",
            "MainField_IsDefeat_Enemy_Sandworm_494020159",
        ];
        let completed = ENEMY_FLAGS
            .iter()
            .filter(|flag| self.is_flag_set(flag))
            .count();
        (completed, ENEMY_FLAGS.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build(version: u32, records: &[(u32, u32)]) -> Vec<u8> {
        let mut out = Vec::with_capacity(HEADER_LEN + records.len() * RECORD_LEN + 4);
        out.extend_from_slice(&version.to_le_bytes());
        out.extend_from_slice(&SENTINEL.to_le_bytes());
        out.extend_from_slice(&1u32.to_le_bytes());
        for (key, value) in records {
            out.extend_from_slice(&key.to_le_bytes());
            out.extend_from_slice(&value.to_le_bytes());
        }
        out.extend_from_slice(&SENTINEL.to_le_bytes());
        out
    }

    #[test]
    fn parses_header_and_records() {
        let save = SaveFile::parse(build(VERSION_1_9_0, &[(1, 10), (2, 20)])).unwrap();
        assert_eq!(save.version(), VERSION_1_9_0);
        assert_eq!(save.len(), 2);
        assert_eq!(save.unique_key_count(), 2);
        assert_eq!(save.get_u32(1), Some(10));
        assert_eq!(save.records()[1], Record { key: 2, value: 20 });
    }

    #[test]
    fn groups_duplicate_keys_in_order() {
        let save = SaveFile::parse(build(VERSION_1_9_0, &[(7, 1), (7, 2), (7, 0)])).unwrap();
        assert_eq!(save.values(7), &[1, 2, 0]);
        assert_eq!(save.get_u32(7), Some(1));
        assert_eq!(save.nonzero_keys().collect::<Vec<_>>(), vec![7]);
    }

    #[test]
    fn decodes_f32_and_vec3f() {
        let x = 12.5f32.to_bits();
        let y = (-3.25f32).to_bits();
        let z = 0.0f32.to_bits();
        let save = SaveFile::parse(build(VERSION_1_9_0, &[(9, x), (9, y), (9, z)])).unwrap();
        assert_eq!(save.get_vec3f(9), Some([12.5, -3.25, 0.0]));
    }

    #[test]
    fn decodes_nul_padded_strings() {
        let mut words = Vec::new();
        for chunk in b"G-6\0".chunks(4) {
            let mut word = [0u8; 4];
            word[..chunk.len()].copy_from_slice(chunk);
            words.push(u32::from_le_bytes(word));
        }
        let records: Vec<_> = words.into_iter().map(|word| (13, word)).collect();
        let save = SaveFile::parse(build(VERSION_1_9_0, &records)).unwrap();
        assert_eq!(save.get_string(13).as_deref(), Some("G-6"));
    }

    #[test]
    fn crc_matches_known_key() {
        assert_eq!(SaveFile::key_id("CurrentRupee"), 0x2314_9bf8);
    }

    #[test]
    fn completion_totals_match_tracked_lists() {
        let save = SaveFile::parse(build(VERSION_1_9_0, &[])).unwrap();
        let summary = save.completion_summary();
        assert_eq!(summary.shrines.total, SHRINE_TOTAL);
        assert_eq!(summary.towers.total, TOWER_TOTAL);
        assert_eq!(summary.koroks.total, KOROK_TOTAL);
        assert_eq!(summary.memories.total, MEMORY_TOTAL);
        assert_eq!(summary.locations.total, 187);
        assert_eq!(summary.enemies.total, 84);
        assert_eq!(summary.shrines.completed, 0);
        assert_eq!(summary.locations.completed, 0);
        assert_eq!(summary.enemies.completed, 0);
    }

    #[test]
    fn rejects_big_endian_wii_u_header() {
        let mut bytes = build(VERSION_1_9_0, &[]);
        bytes[0..4].copy_from_slice(&VERSION_1_9_0.to_be_bytes());
        assert!(matches!(
            SaveFile::parse(bytes),
            Err(SaveError::UnsupportedEndianness)
        ));
    }

    #[test]
    fn rejects_missing_sentinel() {
        let mut bytes = build(VERSION_1_9_0, &[(1, 1)]);
        let last = bytes.len() - 1;
        bytes[last] = 0;
        assert!(matches!(
            SaveFile::parse(bytes),
            Err(SaveError::MissingSentinel)
        ));
    }

    #[test]
    fn rejects_partial_record() {
        let mut bytes = build(VERSION_1_9_0, &[(1, 1)]);
        bytes.insert(bytes.len() - 4, 0);
        assert!(matches!(
            SaveFile::parse(bytes),
            Err(SaveError::InvalidLength { .. })
        ));
    }

    #[test]
    fn rejects_bad_marker() {
        let mut bytes = build(VERSION_1_9_0, &[]);
        bytes[4..8].copy_from_slice(&0u32.to_le_bytes());
        assert!(matches!(
            SaveFile::parse(bytes),
            Err(SaveError::BadMarker(0))
        ));
    }

    #[test]
    fn rejects_tiny_files() {
        assert!(matches!(
            SaveFile::parse(vec![0u8; 8]),
            Err(SaveError::TooSmall { .. })
        ));
    }
}
