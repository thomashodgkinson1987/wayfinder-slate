//! CRC32 key IDs for save fields that Wayfinder Slate reads directly.
//!
//! Names come from the community flag-name table. The game misspells "heart"
//! as "hart" in two fields; constants keep the internal spelling so the CRC32
//! values stay correct.

/// Current rupees (`CurrentRupee`, s32).
pub const CURRENT_RUPEE: u32 = 0x2314_9bf8;
/// Maximum heart value (`MaxHartValue`, s32; four quarter-hearts per heart).
pub const MAX_HART: u32 = 0x2906_f327;
/// Current heart value (`CurrentHart`, s32).
pub const CURRENT_HART: u32 = 0xbe9b_c993;
/// Maximum stamina capacity (`StaminaMax`, f32).
pub const STAMINA_MAX: u32 = 0x3adf_f047;
/// Current stamina (`StaminaCurrentMax`, f32). Despite the internal name, the
/// game writes the current amount here, not the vessel-expanded maximum.
pub const STAMINA_CURRENT_MAX: u32 = 0x58af_5eac;
/// Collected Korok seed counter (`HiddenKorok_Number`, s32).
pub const HIDDEN_KOROK_NUMBER: u32 = 0x8a94_e07a;
/// Total play time in seconds (`PlayReport_PlayTime`, s32).
pub const PLAY_TIME: u32 = 0x73c2_9681;
/// Player position (`PlayerSavePos`, vector3f: x, y, z).
pub const PLAYER_SAVE_POS: u32 = 0xa40b_a103;
/// Player facing angle in degrees (`PlayerSavePosAngleYDegree`, f32).
pub const PLAYER_SAVE_POS_ANGLE_Y: u32 = 0x6293_b955;
/// Current map cell name (`PlayerSavePosMapName`, string), e.g. `G-6`.
pub const PLAYER_SAVE_POS_MAP_NAME: u32 = 0x0bee_9e46;
/// Current map type (`PlayerSavePosMapType`, string), e.g. `MainField`.
pub const PLAYER_SAVE_POS_MAP_TYPE: u32 = 0xd913_b769;
/// Inventory item names (`PorchItem`, string array).
pub const PORCH_ITEM: u32 = 0x5f28_3289;
/// Inventory item quantities (`PorchItem_Value1`, s32 array).
pub const PORCH_ITEM_VALUE1: u32 = 0x6a09_fc59;
/// Map pin icon numbers (`MapApp_MapIconNo`, s32 array).
pub const MAP_APP_ICON_NO: u32 = 0x9383_490e;
/// Map pin positions (`MapApp_MapIconPos`, vector3f array).
pub const MAP_APP_ICON_POS: u32 = 0xea9d_ef3f;

/// Save timestamp stored in `caption.sav`, as a Unix timestamp in seconds.
pub const CAPTION_SAVED_AT: u32 = 0x0ba7_cf40;
/// Location identifier stored in `caption.sav`, e.g. `FaronHatago000`.
pub const CAPTION_LOCATION: u32 = 0x46d2_66b6;
/// Map area stored in `caption.sav`, e.g. `MapArea_WestHateru`.
pub const CAPTION_MAP_AREA: u32 = 0xf26f_1a02;
