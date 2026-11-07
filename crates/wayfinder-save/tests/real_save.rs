use std::path::PathBuf;

use wayfinder_save::SaveFile;
use wayfinder_save::keys;

/// Parse a real 1.9.0 save when `WAYFINDER_TEST_SAVE` points at one.
///
/// The supplied save is private and is never committed. CI can provide its own
/// fixture through the same variable.
#[test]
fn parses_real_save_when_configured() {
    let Some(path) = std::env::var_os("WAYFINDER_TEST_SAVE")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
    else {
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
