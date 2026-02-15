use crate::hid_usage::HidUsage;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BehaviorRole {
    KeyPress,
    KeyToggle,
    LayerTap,
    ModTap,
    StickyKey,
    StickyLayer,
    MomentaryLayer,
    ToggleLayer,
    ToLayer,
    Bluetooth,
    ExternalPower,
    OutputSelection,
    Backlight,
    Underglow,
    MouseKeyPress,
    MouseMove,
    MouseScroll,
    CapsWord,
    KeyRepeat,
    Reset,
    Bootloader,
    SoftOff,
    StudioUnlock,
    GraveEscape,
    Transparent,
    None,
}

/// Lossless typed behavior value for a single key binding.
///
/// Used by [`crate::StudioClient::get_key_at`] and [`crate::StudioClient::set_key_at`].
/// Unknown behavior IDs are represented by [`Behavior::Unknown`].
#[derive(Debug, Clone, PartialEq)]
pub enum Behavior {
    KeyPress(HidUsage),
    KeyToggle(HidUsage),
    LayerTap {
        layer_id: u32,
        tap: HidUsage,
    },
    ModTap {
        hold: HidUsage,
        tap: HidUsage,
    },
    StickyKey(HidUsage),
    StickyLayer {
        layer_id: u32,
    },
    MomentaryLayer {
        layer_id: u32,
    },
    ToggleLayer {
        layer_id: u32,
    },
    ToLayer {
        layer_id: u32,
    },
    Bluetooth {
        command: u32,
        value: u32,
    },
    ExternalPower {
        value: u32,
    },
    OutputSelection {
        value: u32,
    },
    Backlight {
        command: u32,
        value: u32,
    },
    Underglow {
        command: u32,
        value: u32,
    },
    MouseKeyPress {
        value: u32,
    },
    MouseMove {
        value: u32,
    },
    MouseScroll {
        value: u32,
    },
    CapsWord,
    KeyRepeat,
    Reset,
    Bootloader,
    SoftOff,
    StudioUnlock,
    GraveEscape,
    Transparent,
    None,
    Unknown {
        behavior_id: i32,
        param1: u32,
        param2: u32,
    },
}

pub fn role_from_display_name(name: &str) -> Option<BehaviorRole> {
    let n = name.trim().to_ascii_lowercase();
    match n.as_str() {
        // Explicit display-name values from zmk-main/app/dts/behaviors/*.dtsi
        "key press" => Some(BehaviorRole::KeyPress),
        "key toggle" => Some(BehaviorRole::KeyToggle),
        "layer-tap" => Some(BehaviorRole::LayerTap),
        "mod-tap" => Some(BehaviorRole::ModTap),
        "sticky key" => Some(BehaviorRole::StickyKey),
        "sticky layer" => Some(BehaviorRole::StickyLayer),
        "momentary layer" => Some(BehaviorRole::MomentaryLayer),
        "toggle layer" => Some(BehaviorRole::ToggleLayer),
        "to layer" => Some(BehaviorRole::ToLayer),
        "bluetooth" => Some(BehaviorRole::Bluetooth),
        "external power" => Some(BehaviorRole::ExternalPower),
        "output selection" => Some(BehaviorRole::OutputSelection),
        "backlight" => Some(BehaviorRole::Backlight),
        "underglow" => Some(BehaviorRole::Underglow),
        "mouse key press" => Some(BehaviorRole::MouseKeyPress),
        "caps word" => Some(BehaviorRole::CapsWord),
        "key repeat" => Some(BehaviorRole::KeyRepeat),
        "reset" => Some(BehaviorRole::Reset),
        "bootloader" => Some(BehaviorRole::Bootloader),
        "studio unlock" => Some(BehaviorRole::StudioUnlock),
        "grave/escape" => Some(BehaviorRole::GraveEscape),
        "transparent" => Some(BehaviorRole::Transparent),
        "none" => Some(BehaviorRole::None),
        // Behaviors without display-name that use DEVICE_DT_NAME(node_id)
        "mouse_move" => Some(BehaviorRole::MouseMove),
        "mouse_scroll" => Some(BehaviorRole::MouseScroll),
        "z_so_off" => Some(BehaviorRole::SoftOff),
        _ => None,
    }
}
