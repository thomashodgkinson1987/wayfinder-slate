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

        Ok(Self {
            mode,
            bind,
            save_dir,
            control_addr,
            data_dir,
            cookie_secure,
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

    /// Discover saved slots in local/LAN mode.
    ///
    /// `WAYFINDER_SAVE_DIR` may point at the save root (`.../save`), at the
    /// `save_0` profile folder itself, or directly at one slot folder. Hub mode
    /// never exposes a filesystem save.
    pub fn save_slots(&self) -> Vec<usize> {
        if self.mode == Mode::Hub {
            return Vec::new();
        }
        let Some(dir) = self.save_dir.as_ref() else {
            return Vec::new();
        };

        let profile = if dir.join("save_0").is_dir() {
            dir.join("save_0")
        } else if dir.join("game_data.sav").is_file() {
            return vec![0];
        } else {
            dir.clone()
        };

        let Ok(entries) = std::fs::read_dir(profile) else {
            return Vec::new();
        };
        let mut slots: Vec<usize> = entries
            .flatten()
            .filter_map(|entry| {
                let name = entry.file_name();
                let name = name.to_str()?;
                if name.is_empty() || !name.chars().all(|character| character.is_ascii_digit()) {
                    return None;
                }
                let slot = name.parse().ok()?;
                entry.path().join("game_data.sav").is_file().then_some(slot)
            })
            .collect();
        slots.sort_unstable();
        slots.dedup();
        slots
    }

    /// Resolve a discovered slot directory in local/LAN mode.
    pub fn save_slot_dir(&self, slot: usize) -> Option<PathBuf> {
        if self.mode == Mode::Hub {
            return None;
        }
        let dir = self.save_dir.as_ref()?;

        if dir.join("save_0").is_dir() {
            let slot_dir = dir.join("save_0").join(slot.to_string());
            return slot_dir.join("game_data.sav").is_file().then_some(slot_dir);
        }
        if dir.join("game_data.sav").is_file() {
            return (slot == 0).then(|| dir.clone());
        }

        let slot_dir = dir.join(slot.to_string());
        slot_dir.join("game_data.sav").is_file().then_some(slot_dir)
    }

    /// Resolve the `game_data.sav` for a discovered slot.
    pub fn save_slot_path(&self, slot: usize) -> Option<PathBuf> {
        self.save_slot_dir(slot)
            .map(|directory| directory.join("game_data.sav"))
    }

    /// Resolve the summary caption for a discovered slot.
    pub fn save_caption_path(&self, slot: usize) -> Option<PathBuf> {
        self.save_slot_dir(slot)
            .map(|directory| directory.join("caption.sav"))
    }

    /// Resolve the screenshot for a discovered slot.
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
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "" | "0" | "false" | "no" | "off" => Ok(false),
        other => Err(format!(
            "unknown WAYFINDER_COOKIE_SECURE {other:?}; expected a boolean"
        )),
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
