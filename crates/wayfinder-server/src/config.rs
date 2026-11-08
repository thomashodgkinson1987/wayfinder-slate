use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use serde::Serialize;

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
}

#[derive(Debug, Clone)]
pub struct Config {
    pub mode: Mode,
    pub bind: SocketAddr,
    pub save_dir: Option<PathBuf>,
    pub control_addr: SocketAddr,
    pub data_dir: PathBuf,
}

impl Config {
    pub fn from_env() -> Result<Self, String> {
        let mode = match std::env::var("WAYFINDER_MODE") {
            Ok(value) => Mode::parse(&value)?,
            Err(_) => Mode::Local,
        };

        let bind = std::env::var("WAYFINDER_BIND")
            .unwrap_or_else(|_| "127.0.0.1:8080".into())
            .parse()
            .map_err(|error| format!("invalid WAYFINDER_BIND: {error}"))?;

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

        Ok(Self {
            mode,
            bind,
            save_dir,
            control_addr,
            data_dir,
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

    /// Resolve the `game_data.sav` for a slot in local/LAN mode.
    ///
    /// `WAYFINDER_SAVE_DIR` may point at the save root (`.../save`) or at the
    /// `save_0` folder itself. Both layouts are accepted, and hub mode never
    /// exposes a filesystem save.
    pub fn save_slot_path(&self, slot: usize) -> Option<PathBuf> {
        if self.mode == Mode::Hub {
            return None;
        }
        let dir = self.save_dir.as_ref()?;
        let slot = slot.to_string();
        [
            dir.join("save_0").join(&slot).join("game_data.sav"),
            dir.join(&slot).join("game_data.sav"),
            dir.join("game_data.sav"),
        ]
        .into_iter()
        .find(|path| path.is_file())
    }
}

fn wayfinder_control_addr() -> String {
    "127.0.0.1:7777".into()
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
    // A save root contains save_0 or is itself a profile folder with numbered
    // slots. Accept either so users can point at ~/.../savagewind/save or at
    // the save_0 folder itself.
    if path.join("save_0").is_dir() {
        return true;
    }
    std::fs::read_dir(path)
        .ok()
        .into_iter()
        .flatten()
        .flatten()
        .any(|entry| entry.file_name().to_string_lossy() == "0" && entry.path().is_dir())
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
        }
    }

    #[test]
    fn hub_has_no_filesystem_save_exposure() {
        let config = test_config(Mode::Hub, Some(PathBuf::from("/private/save")));
        assert_eq!(config.save_dir_for_display(), None);
        assert_eq!(config.save_slot_path(0), None);
    }

    #[test]
    fn resolves_save_root_and_save_0_layouts() {
        let root = std::env::temp_dir().join(format!("wayfinder-save-slot-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("save_0/3")).unwrap();
        std::fs::write(root.join("save_0/3/game_data.sav"), b"test").unwrap();

        let root_config = test_config(Mode::Local, Some(root.clone()));
        assert_eq!(
            root_config.save_slot_path(3),
            Some(root.join("save_0/3/game_data.sav"))
        );

        let save_0_config = test_config(Mode::Local, Some(root.join("save_0")));
        assert_eq!(
            save_0_config.save_slot_path(3),
            Some(root.join("save_0/3/game_data.sav"))
        );

        std::fs::create_dir_all(root.join("direct")).unwrap();
        std::fs::write(root.join("direct/game_data.sav"), b"test").unwrap();
        let direct_config = test_config(Mode::Local, Some(root.join("direct")));
        assert_eq!(
            direct_config.save_slot_path(3),
            Some(root.join("direct/game_data.sav"))
        );

        std::fs::remove_dir_all(root).unwrap();
    }
}
