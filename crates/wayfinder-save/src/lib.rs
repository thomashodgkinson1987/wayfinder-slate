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
