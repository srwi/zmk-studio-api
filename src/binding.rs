//! Typed binding domain model on top of raw ZMK behavior bindings.

use crate::keycode::KeyCode;
use crate::proto::zmk;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BehaviorRole {
    KeyPress,
    KeyToggle,
    LayerTap,
    ModTap,
    MomentaryLayer,
    ToggleLayer,
    ToLayer,
    Transparent,
    None,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Behavior {
    KeyPress(KeyCode),
    KeyToggle(KeyCode),
    LayerTap { layer_id: u32, tap: KeyCode },
    ModTap { hold: KeyCode, tap: KeyCode },
    MomentaryLayer { layer_id: u32 },
    ToggleLayer { layer_id: u32 },
    ToLayer { layer_id: u32 },
    Transparent,
    None,
    Raw(zmk::keymap::BehaviorBinding),
}

pub fn role_from_display_name(name: &str) -> Option<BehaviorRole> {
    let n = name.trim().to_ascii_lowercase();
    match n.as_str() {
        "key press" => Some(BehaviorRole::KeyPress),
        "key toggle" => Some(BehaviorRole::KeyToggle),
        "layer-tap" => Some(BehaviorRole::LayerTap),
        "mod-tap" => Some(BehaviorRole::ModTap),
        "momentary layer" => Some(BehaviorRole::MomentaryLayer),
        "toggle layer" => Some(BehaviorRole::ToggleLayer),
        "to layer" => Some(BehaviorRole::ToLayer),
        "transparent" => Some(BehaviorRole::Transparent),
        "none" => Some(BehaviorRole::None),
        _ => None,
    }
}
