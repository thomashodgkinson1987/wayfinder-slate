use std::path::PathBuf;

use wayfinder_save::SaveFile;
use wayfinder_save::keys;

/// The optional private save used by the real-save tests.
fn test_save_path() -> Option<PathBuf> {
    std::env::var_os("WAYFINDER_TEST_SAVE")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// Parse a real 1.9.0 save when `WAYFINDER_TEST_SAVE` points at one.
///
/// The supplied save is private and is never committed. CI can provide its own
/// fixture through the same variable.
#[test]
fn parses_real_save_when_configured() {
    let Some(path) = test_save_path() else {
        eprintln!("skipping: WAYFINDER_TEST_SAVE is not set");
        return;
    };

    let save = SaveFile::from_path(&path).expect("parse 1.9.0 save");
    assert_eq!(save.version(), wayfinder_save::VERSION_1_9_0);
    assert!(save.len() > 100_000);
    assert!(save.get_u32(keys::CURRENT_RUPEE).is_some());
    assert!(save.get_f32(keys::STAMINA_MAX).is_some());
    assert!(save.get_vec3f(keys::PLAYER_SAVE_POS).is_some());
    assert!(save.get_string(keys::PLAYER_SAVE_POS_MAP_NAME).is_some());
}

/// Verify completion summary matches expected values from the development save.
#[test]
fn real_save_completion_summary() {
    let Some(path) = test_save_path() else {
        eprintln!("skipping: WAYFINDER_TEST_SAVE is not set");
        return;
    };

    let save = SaveFile::from_path(&path).expect("parse 1.9.0 save");
    let summary = save.completion_summary();

    // Expected values from the known save (Faron Stable area, early game)
    assert_eq!(summary.shrines.total, 136);
    assert_eq!(summary.shrines.completed, 12); // Clear_Dungeon 7,9,15,17,29,38,41,44,45,47,65,99
    assert_eq!(summary.towers.total, 15);
    assert_eq!(summary.towers.completed, 2); // MapTower_07, MapTower_08
    assert_eq!(summary.koroks.total, 900);
    assert_eq!(summary.koroks.completed, 72); // from HiddenKorok_Number counter
    assert_eq!(summary.memories.total, 14);
    assert_eq!(summary.memories.completed, 1); // IsGet_MemoryPhoto_008
    assert_eq!(summary.locations.total, 187);
    assert_eq!(summary.locations.completed, 18); // 18 known locations discovered
    assert_eq!(summary.enemies.total, 84);
    assert_eq!(summary.enemies.completed, 1); // 1 Talus Junior defeated
    assert_eq!(summary.treasures.total, 1312);
    assert_eq!(summary.treasures.completed, 112); // 112 treasure chests opened
}

/// Verify the player stats and caption metadata match the development save.
#[test]
fn real_save_stats_and_caption() {
    let Some(path) = test_save_path() else {
        eprintln!("skipping: WAYFINDER_TEST_SAVE is not set");
        return;
    };

    let save = SaveFile::from_path(&path).expect("parse 1.9.0 save");
    let stats = save.stats();

    assert_eq!(stats.play_time_seconds, 50_166);
    assert_eq!(stats.rupees, 1_084);
    assert_eq!(stats.hearts.current, 4.0);
    assert_eq!(stats.hearts.max, 4.0);
    assert!((stats.stamina.current - 476.309_33).abs() < 0.01);
    assert_eq!(stats.stamina.max, 1_200.0);
    assert_eq!(stats.location.map, "G-6");
    assert_eq!(stats.location.map_type, "MainField");

    let caption_path = path.with_file_name("caption.sav");
    assert!(
        caption_path.is_file(),
        "expected caption.sav beside {}",
        path.display()
    );
    let caption = SaveFile::from_path(caption_path)
        .expect("parse caption.sav")
        .caption();
    assert_eq!(caption.saved_at, 1_794_087_241);
    assert_eq!(caption.location, "FaronHatago000");
    assert_eq!(caption.map_area, "MapArea_WestHateru");
}
