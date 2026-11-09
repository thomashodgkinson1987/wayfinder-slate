use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use serde::Serialize;
use time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Local,
    Lan,
    Hub,
}

impl Mode {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value.trim().to_ascii_lowercase().as_str() {
            "local" => Ok(Self::Local),
            "lan" => Ok(Self::Lan),
            "hub" => Ok(Self::Hub),
            other => Err(format!(
                "unknown WAYFINDER_MODE {other:?}; expected local, lan, or hub"
            )),
        }
    }

    /// Live Savage Wind polling is only possible when the server can reach the
    /// player's loopback control socket.
    pub fn supports_direct_live(self) -> bool {
        matches!(self, Self::Local | Self::Lan)
    }

    /// Local mode serves one user on the gaming PC, so bind to loopback.
    /// LAN and hub modes must accept connections from other devices.
    pub fn default_bind(self) -> SocketAddr {
        match self {
            Self::Local => "127.0.0.1:8080",
            Self::Lan | Self::Hub => "0.0.0.0:8080",
        }
        .parse()
        .expect("hard-coded bind address")
    }
}

/// Session lifetime for issued cookies and stored session records.
/// A module constant keeps the environment surface small.
pub const SESSION_TTL: Duration = Duration::days(7);

#[derive(Debug, Clone)]
pub struct Config {
    pub mode: Mode,
    pub bind: SocketAddr,
    pub save_dir: Option<PathBuf>,
    pub control_addr: SocketAddr,
    pub data_dir: PathBuf,
    /// Mark the session cookie `Secure` for TLS deployments. Only set this for
    /// HTTPS; a plain-HTTP deployment would drop the cookie entirely.
    pub cookie_secure: bool,
    /// Maximum snapshots retained per save slot; `0` keeps all history.
    pub snapshot_limit: u32,
    /// Store a gzip-compressed copy of each saved `game_data.sav`.
    pub keep_raw_saves: bool,
    /// Watch the local save folder for changes (local/LAN only).
    pub watch_saves: bool,
    /// Maximum accepted multipart upload size in bytes.
    pub max_upload_bytes: usize,
}

/// A numbered slot folder discovered on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredSlot {
    pub index: usize,
    pub dir: PathBuf,
}

/// Discover numbered save slots below `root`.
///
/// Accepts the three layouts Savage Wind can produce: a save root containing
/// `save_0`, a profile folder containing numbered slot folders, or a single
/// slot folder containing `game_data.sav` directly.
///
/// Returns an error when the directory cannot be listed so callers can avoid
/// marking every slot absent after a transient failure.
pub fn try_discover_slots(root: &Path) -> std::io::Result<Vec<DiscoveredSlot>> {
    if root.join("save_0").is_dir() {
        return discover_numbered(root.join("save_0"));
    }
    if root.join("game_data.sav").is_file() {
        return Ok(vec![DiscoveredSlot {
            index: 0,
            dir: root.to_path_buf(),
        }]);
    }
    discover_numbered(root.to_path_buf())
}

fn discover_numbered(profile: PathBuf) -> std::io::Result<Vec<DiscoveredSlot>> {
    let mut slots: Vec<DiscoveredSlot> = std::fs::read_dir(&profile)?
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name();
            let name = name.to_str()?;
            if name.is_empty() || !name.chars().all(|character| character.is_ascii_digit()) {
                return None;
            }
            let index = name.parse().ok()?;
            let dir = entry.path();
            dir.join("game_data.sav")
                .is_file()
                .then_some(DiscoveredSlot { index, dir })
        })
        .collect();
    slots.sort_by_key(|slot| slot.index);
    slots.dedup_by_key(|slot| slot.index);
    Ok(slots)
}

impl Config {
    pub fn from_env() -> Result<Self, String> {
        let mode = match std::env::var("WAYFINDER_MODE") {
            Ok(value) => Mode::parse(&value)?,
            Err(_) => Mode::Local,
        };

        let bind = match std::env::var("WAYFINDER_BIND") {
            Ok(value) => value
                .parse()
                .map_err(|error| format!("invalid WAYFINDER_BIND: {error}"))?,
            Err(_) => mode.default_bind(),
        };

        let control_addr = std::env::var("WAYFINDER_CONTROL_ADDR")
            .unwrap_or_else(|_| wayfinder_control_addr())
            .parse()
            .map_err(|error| format!("invalid WAYFINDER_CONTROL_ADDR: {error}"))?;

        let save_dir = match std::env::var_os("WAYFINDER_SAVE_DIR") {
            Some(path) => Some(PathBuf::from(path)),
            None => auto_detect_save_dir(),
        };

        let data_dir = std::env::var_os("WAYFINDER_DATA_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("data"));

        let cookie_secure = match std::env::var("WAYFINDER_COOKIE_SECURE") {
            Ok(value) => parse_cookie_secure(&value)?,
            Err(_) => false,
        };

        let snapshot_limit = match std::env::var("WAYFINDER_SNAPSHOT_LIMIT") {
            Ok(value) => value
                .trim()
                .parse()
                .map_err(|error| format!("invalid WAYFINDER_SNAPSHOT_LIMIT: {error}"))?,
            Err(_) => 500,
        };

        let keep_raw_saves = match std::env::var("WAYFINDER_KEEP_RAW_SAVES") {
            Ok(value) => parse_bool("WAYFINDER_KEEP_RAW_SAVES", &value)?,
            Err(_) => false,
        };

        let watch_saves = match std::env::var("WAYFINDER_WATCH") {
            Ok(value) => parse_bool("WAYFINDER_WATCH", &value)?,
            Err(_) => true,
        };

        let max_upload_bytes = match std::env::var("WAYFINDER_MAX_UPLOAD_BYTES") {
            Ok(value) => {
                let parsed: usize = value
                    .trim()
                    .parse()
                    .map_err(|error| format!("invalid WAYFINDER_MAX_UPLOAD_BYTES: {error}"))?;
                parsed.max(512 * 1024)
            }
            Err(_) => 8 * 1024 * 1024,
        };

        Ok(Self {
            mode,
            bind,
            save_dir,
            control_addr,
            data_dir,
            cookie_secure,
            snapshot_limit,
            keep_raw_saves,
            watch_saves,
            max_upload_bytes,
        })
    }

    pub fn save_dir_for_display(&self) -> Option<String> {
        if self.mode == Mode::Hub {
            return None;
        }
        self.save_dir
            .as_ref()
            .map(|path| path.display().to_string())
    }

    /// Directory the watcher should watch: the profile folder when the save
    /// root contains `save_0`, otherwise the configured directory itself.
    pub fn save_root(&self) -> Option<PathBuf> {
        if self.mode == Mode::Hub {
            return None;
        }
        let dir = self.save_dir.as_ref()?;
        if dir.join("save_0").is_dir() {
            Some(dir.join("save_0"))
        } else {
            Some(dir.clone())
        }
    }

    /// Discover saved slots in local/LAN mode. Test-only helper now that the
    /// API reads stored snapshots.
    #[cfg(test)]
    pub fn save_slots(&self) -> Vec<usize> {
        if self.mode == Mode::Hub {
            return Vec::new();
        }
        self.save_dir
            .as_ref()
            .map(|dir| {
                try_discover_slots(dir)
                    .unwrap_or_default()
                    .into_iter()
                    .map(|slot| slot.index)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Resolve a discovered slot directory in local/LAN mode.
    #[cfg(test)]
    pub fn save_slot_dir(&self, slot: usize) -> Option<PathBuf> {
        if self.mode == Mode::Hub {
            return None;
        }
        let dir = self.save_dir.as_ref()?;
        try_discover_slots(dir)
            .ok()?
            .into_iter()
            .find(|discovered| discovered.index == slot)
            .map(|discovered| discovered.dir)
    }

    /// Resolve the `game_data.sav` for a discovered slot.
    #[cfg(test)]
    pub fn save_slot_path(&self, slot: usize) -> Option<PathBuf> {
        self.save_slot_dir(slot)
            .map(|directory| directory.join("game_data.sav"))
    }

    /// Resolve the summary caption for a discovered slot.
    #[cfg(test)]
    pub fn save_caption_path(&self, slot: usize) -> Option<PathBuf> {
        self.save_slot_dir(slot)
            .map(|directory| directory.join("caption.sav"))
    }

    /// Resolve the screenshot for a discovered slot.
    #[cfg(test)]
    pub fn save_caption_image_path(&self, slot: usize) -> Option<PathBuf> {
        self.save_slot_dir(slot)
            .map(|directory| directory.join("caption.jpg"))
    }
}

fn wayfinder_control_addr() -> String {
    "127.0.0.1:7777".into()
}

/// Parse `WAYFINDER_COOKIE_SECURE`. Accepts common boolean spellings; an
/// explicit `true` is required so that accidental Secure cookies on HTTP
/// loopback deployments cannot silently break login.
fn parse_cookie_secure(value: &str) -> Result<bool, String> {
    parse_bool("WAYFINDER_COOKIE_SECURE", value)
}

fn parse_bool(name: &str, value: &str) -> Result<bool, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "" | "0" | "false" | "no" | "off" => Ok(false),
        other => Err(format!("unknown {name} {other:?}; expected a boolean")),
    }
}

/// Best-effort discovery of the Savage Wind save directory.
///
/// Users can always override this with `WAYFINDER_SAVE_DIR`. On Windows,
/// Savage Wind keeps its save next to the executable, so running the server
/// from the same folder usually works without configuration.
pub fn auto_detect_save_dir() -> Option<PathBuf> {
    let mut candidates = Vec::new();

    #[cfg(target_os = "linux")]
    {
        if let Some(data_home) = std::env::var_os("XDG_DATA_HOME") {
            candidates.push(PathBuf::from(data_home).join("savagewind/save"));
        }
        if let Some(home) = std::env::var_os("HOME") {
            candidates.push(PathBuf::from(home).join(".local/share/savagewind/save"));
        }
    }

    #[cfg(target_os = "macos")]
    {
        if let Some(home) = std::env::var_os("HOME") {
            candidates
                .push(PathBuf::from(home).join("Library/Application Support/savagewind/save"));
        }
    }

    #[cfg(target_os = "windows")]
    {
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                candidates.push(dir.join("save"));
                if let Some(parent) = dir.parent() {
                    candidates.push(parent.join("save"));
                }
            }
        }
        if let Some(profile) = std::env::var_os("USERPROFILE") {
            candidates.push(PathBuf::from(profile).join("savagewind/save"));
        }
    }

    candidates.into_iter().find(|path| is_save_root(path))
}

fn is_save_root(path: &Path) -> bool {
    if !path.is_dir() {
        return false;
    }
    // A save root contains save_0; a profile folder contains numbered slots;
    // the innermost layout is a single slot folder with game_data.sav.
    if path.join("save_0").is_dir() {
        return true;
    }
    if path.join("game_data.sav").is_file() {
        return true;
    }
    std::fs::read_dir(path)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .any(|entry| {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            !name.is_empty()
                && name.chars().all(|character| character.is_ascii_digit())
                && entry.path().join("game_data.sav").is_file()
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_modes_case_insensitively() {
        assert_eq!(Mode::parse("Local").unwrap(), Mode::Local);
        assert_eq!(Mode::parse(" LAN ").unwrap(), Mode::Lan);
        assert_eq!(Mode::parse("hub").unwrap(), Mode::Hub);
        assert!(Mode::parse("public").is_err());
    }

    fn test_config(mode: Mode, save_dir: Option<PathBuf>) -> Config {
        Config {
            mode,
            bind: "127.0.0.1:0".parse().unwrap(),
            save_dir,
            control_addr: "127.0.0.1:7777".parse().unwrap(),
            data_dir: PathBuf::from("data"),
            cookie_secure: false,
            snapshot_limit: 500,
            keep_raw_saves: false,
            watch_saves: true,
            max_upload_bytes: 8 * 1024 * 1024,
        }
    }

    #[test]
    fn binds_to_loopback_for_local_and_all_interfaces_otherwise() {
        assert_eq!(Mode::Local.default_bind().to_string(), "127.0.0.1:8080");
        assert_eq!(Mode::Lan.default_bind().to_string(), "0.0.0.0:8080");
        assert_eq!(Mode::Hub.default_bind().to_string(), "0.0.0.0:8080");
    }

    #[test]
    fn parses_cookie_secure_booleans() {
        for value in ["true", "TRUE", "1", "yes", "on", " yes "] {
            assert!(parse_cookie_secure(value).unwrap(), "{value:?}");
        }
        for value in ["false", "0", "no", "off", ""] {
            assert!(!parse_cookie_secure(value).unwrap(), "{value:?}");
        }
        assert!(parse_cookie_secure("maybe").is_err());
    }

    fn temp_root(name: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("wayfinder-save-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn write_slot(directory: &Path) {
        std::fs::create_dir_all(directory).unwrap();
        std::fs::write(directory.join("game_data.sav"), b"test").unwrap();
    }

    #[test]
    fn hub_has_no_filesystem_save_exposure() {
        let config = test_config(Mode::Hub, Some(PathBuf::from("/private/save")));
        assert_eq!(config.save_dir_for_display(), None);
        assert!(config.save_slots().is_empty());
        assert_eq!(config.save_slot_path(0), None);
        assert_eq!(config.save_caption_path(0), None);
    }

    #[test]
    fn discovers_and_sorts_numbered_slots() {
        let root = temp_root("slots");
        for slot in ["10", "0", "3"] {
            write_slot(&root.join("save_0").join(slot));
        }
        std::fs::create_dir_all(root.join("save_0/not-a-slot")).unwrap();
        std::fs::write(root.join("save_0/not-a-slot/game_data.sav"), b"test").unwrap();
        std::fs::create_dir_all(root.join("save_0/7")).unwrap();

        let root_config = test_config(Mode::Local, Some(root.clone()));
        assert_eq!(root_config.save_slots(), vec![0, 3, 10]);
        assert_eq!(
            root_config.save_slot_path(3),
            Some(root.join("save_0/3/game_data.sav"))
        );
        assert_eq!(root_config.save_slot_path(7), None);
        assert_eq!(
            root_config.save_caption_path(3),
            Some(root.join("save_0/3/caption.sav"))
        );
        assert_eq!(
            root_config.save_caption_image_path(3),
            Some(root.join("save_0/3/caption.jpg"))
        );

        let save_0_config = test_config(Mode::Local, Some(root.join("save_0")));
        assert_eq!(save_0_config.save_slots(), vec![0, 3, 10]);
        assert_eq!(
            save_0_config.save_slot_path(3),
            Some(root.join("save_0/3/game_data.sav"))
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn treats_a_direct_slot_folder_as_slot_zero() {
        let root = temp_root("direct");
        write_slot(&root);

        let config = test_config(Mode::Local, Some(root.clone()));
        assert_eq!(config.save_slots(), vec![0]);
        assert_eq!(config.save_slot_path(0), Some(root.join("game_data.sav")));
        assert_eq!(config.save_slot_path(1), None);

        std::fs::remove_dir_all(root).unwrap();
    }
}
