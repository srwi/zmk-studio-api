use crate::hid_usage::{HID_USAGE_CONSUMER, HID_USAGE_KEYBOARD, HidUsage};
use crate::proto::zmk::behaviors::{
    BehaviorBindingParametersSet, BehaviorParameterHidUsage, BehaviorParameterValueDescription,
    behavior_parameter_value_description::ValueType,
};

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

impl BehaviorRole {
    pub const ALL: [BehaviorRole; 26] = [
        BehaviorRole::KeyPress,
        BehaviorRole::KeyToggle,
        BehaviorRole::LayerTap,
        BehaviorRole::ModTap,
        BehaviorRole::StickyKey,
        BehaviorRole::StickyLayer,
        BehaviorRole::MomentaryLayer,
        BehaviorRole::ToggleLayer,
        BehaviorRole::ToLayer,
        BehaviorRole::Bluetooth,
        BehaviorRole::ExternalPower,
        BehaviorRole::OutputSelection,
        BehaviorRole::Backlight,
        BehaviorRole::Underglow,
        BehaviorRole::MouseKeyPress,
        BehaviorRole::MouseMove,
        BehaviorRole::MouseScroll,
        BehaviorRole::CapsWord,
        BehaviorRole::KeyRepeat,
        BehaviorRole::Reset,
        BehaviorRole::Bootloader,
        BehaviorRole::SoftOff,
        BehaviorRole::StudioUnlock,
        BehaviorRole::GraveEscape,
        BehaviorRole::Transparent,
        BehaviorRole::None,
    ];

    /// Human-readable label for the role.
    pub fn label(self) -> &'static str {
        match self {
            Self::KeyPress => "Key Press",
            Self::KeyToggle => "Key Toggle",
            Self::LayerTap => "Layer-Tap",
            Self::ModTap => "Mod-Tap",
            Self::StickyKey => "Sticky Key",
            Self::StickyLayer => "Sticky Layer",
            Self::MomentaryLayer => "Momentary Layer",
            Self::ToggleLayer => "Toggle Layer",
            Self::ToLayer => "To Layer",
            Self::Bluetooth => "Bluetooth",
            Self::ExternalPower => "External Power",
            Self::OutputSelection => "Output Selection",
            Self::Backlight => "Backlight",
            Self::Underglow => "Underglow",
            Self::MouseKeyPress => "Mouse Key",
            Self::MouseMove => "Mouse Move",
            Self::MouseScroll => "Mouse Scroll",
            Self::CapsWord => "Caps Word",
            Self::KeyRepeat => "Key Repeat",
            Self::Reset => "Reset",
            Self::Bootloader => "Bootloader",
            Self::SoftOff => "Soft Off",
            Self::StudioUnlock => "Studio Unlock",
            Self::GraveEscape => "Grave Escape",
            Self::Transparent => "Transparent",
            Self::None => "None",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedLayer {
    /// Stable layer identifier assigned by ZMK Studio.
    pub id: u32,
    /// Human-readable layer name, or empty when the device reports none.
    pub name: String,
    /// Resolved bindings in key order.
    pub bindings: Vec<Behavior>,
}

/// One parameter of a [`Behavior::Custom`] binding, typed from the parameter
/// metadata the device publishes for that behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BehaviorParam {
    /// The behavior takes no value in this position.
    Unused,
    /// A key: the same encoding as [`Behavior::KeyPress`]'s parameter.
    Keycode(HidUsage),
    /// A layer index.
    LayerId(u32),
    /// A plain number — a constant or a value from a range, with no richer
    /// meaning the device tells us about.
    Number(u32),
}

impl BehaviorParam {
    /// The value as the RPC encodes it, so a binding read from one position can
    /// be written to another unchanged.
    pub fn to_raw(self) -> u32 {
        match self {
            BehaviorParam::Unused => 0,
            BehaviorParam::Keycode(usage) => usage.to_hid_usage(),
            BehaviorParam::LayerId(value) | BehaviorParam::Number(value) => value,
        }
    }
}

/// Lossless typed behavior value for a single key binding.
///
/// Used by [`crate::StudioClient::get_key_at`] and [`crate::StudioClient::set_key_at`].
/// Behaviors defined in the user's own keymap surface as [`Behavior::Custom`];
/// bindings whose behavior could not be looked up at all become
/// [`Behavior::Unknown`].
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
    /// A behavior that is not one of ZMK's built-ins: a hold-tap, tap-dance,
    /// mod-morph, macro, … defined in the user's keymap.
    ///
    /// The device reports no structure for these beyond a name and the types of
    /// their two parameters, so that is what this carries. The behaviors bound
    /// *inside* a custom behavior (a hold-tap's hold and tap sides, say) are not
    /// part of the RPC surface and cannot be recovered.
    Custom {
        behavior_id: u32,
        /// The behavior's `display-name`, or its devicetree node name when the
        /// keymap sets none.
        display_name: String,
        param1: BehaviorParam,
        param2: BehaviorParam,
    },
    /// A binding whose behavior could not be looked up — the device reported an
    /// ID that is not in its own behavior list.
    Unknown {
        behavior_id: i32,
        param1: u32,
        param2: u32,
    },
}

impl Behavior {
    /// Returns the standard [`BehaviorRole`] this behavior corresponds to, or `None`
    /// for custom/unknown behaviors.
    pub fn role(&self) -> Option<BehaviorRole> {
        match self {
            Self::KeyPress(_) => Some(BehaviorRole::KeyPress),
            Self::KeyToggle(_) => Some(BehaviorRole::KeyToggle),
            Self::LayerTap { .. } => Some(BehaviorRole::LayerTap),
            Self::ModTap { .. } => Some(BehaviorRole::ModTap),
            Self::StickyKey(_) => Some(BehaviorRole::StickyKey),
            Self::StickyLayer { .. } => Some(BehaviorRole::StickyLayer),
            Self::MomentaryLayer { .. } => Some(BehaviorRole::MomentaryLayer),
            Self::ToggleLayer { .. } => Some(BehaviorRole::ToggleLayer),
            Self::ToLayer { .. } => Some(BehaviorRole::ToLayer),
            Self::Bluetooth { .. } => Some(BehaviorRole::Bluetooth),
            Self::ExternalPower { .. } => Some(BehaviorRole::ExternalPower),
            Self::OutputSelection { .. } => Some(BehaviorRole::OutputSelection),
            Self::Backlight { .. } => Some(BehaviorRole::Backlight),
            Self::Underglow { .. } => Some(BehaviorRole::Underglow),
            Self::MouseKeyPress { .. } => Some(BehaviorRole::MouseKeyPress),
            Self::MouseMove { .. } => Some(BehaviorRole::MouseMove),
            Self::MouseScroll { .. } => Some(BehaviorRole::MouseScroll),
            Self::CapsWord => Some(BehaviorRole::CapsWord),
            Self::KeyRepeat => Some(BehaviorRole::KeyRepeat),
            Self::Reset => Some(BehaviorRole::Reset),
            Self::Bootloader => Some(BehaviorRole::Bootloader),
            Self::SoftOff => Some(BehaviorRole::SoftOff),
            Self::StudioUnlock => Some(BehaviorRole::StudioUnlock),
            Self::GraveEscape => Some(BehaviorRole::GraveEscape),
            Self::Transparent => Some(BehaviorRole::Transparent),
            Self::None => Some(BehaviorRole::None),
            Self::Custom { .. } | Self::Unknown { .. } => None,
        }
    }
}

/// Types both parameters of a binding against the behavior's metadata.
///
/// A behavior publishes zero or more *parameter sets*, each describing which
/// values `param1` and `param2` accept; behaviors whose second parameter depends
/// on the first (`&bt`, for instance) publish one set per group of `param1`
/// values. Publishing no set at all means the behavior takes no parameters.
///
/// Mirrors the firmware's own reading of that metadata — see
/// `zmk_behavior_check_params_match_metadata` in `app/src/behavior.c`.
pub(crate) fn typed_params(
    metadata: &[BehaviorBindingParametersSet],
    param1: u32,
    param2: u32,
) -> (BehaviorParam, BehaviorParam) {
    let Some(set) = select_set(metadata, param1, param2) else {
        return (BehaviorParam::Unused, BehaviorParam::Unused);
    };

    (
        typed_param(&set.param1, param1),
        typed_param(&set.param2, param2),
    )
}

/// The set the firmware would validate this binding against: the first one that
/// describes *both* values.
///
/// Keymaps compiled from devicetree are never validated against this metadata —
/// only bindings written back over the RPC are — so a keymap can hold a binding
/// that matches no set at all. The first set is then the best guess at how to
/// read it, which beats dropping both values.
fn select_set(
    metadata: &[BehaviorBindingParametersSet],
    param1: u32,
    param2: u32,
) -> Option<&BehaviorBindingParametersSet> {
    metadata
        .iter()
        .find(|set| accepts(&set.param1, param1) && accepts(&set.param2, param2))
        .or(metadata.first())
}

/// True when `values` describes `value` as valid for its position.
fn accepts(values: &[BehaviorParameterValueDescription], value: u32) -> bool {
    // No descriptions: the behavior takes nothing in this position, and the
    // firmware then only accepts the zero that stands for "no value".
    if values.is_empty() {
        return value == 0;
    }

    values
        .iter()
        .any(|description| describes(description, value))
}

/// True when this one description covers `value` — the client-side counterpart
/// of the firmware's `check_param_matches_value`.
fn describes(description: &BehaviorParameterValueDescription, value: u32) -> bool {
    match &description.value_type {
        Some(ValueType::Constant(constant)) => *constant == value,
        // Widened to `i64` so a keycode carrying modifiers in its top bits stays
        // positive; the firmware compares it unsigned, which comes out the same
        // for the small non-negative bounds ZMK actually publishes.
        Some(ValueType::Range(range)) => {
            (i64::from(range.min)..=i64::from(range.max)).contains(&i64::from(value))
        }
        Some(ValueType::HidUsage(limits)) => is_valid_hid_usage(value, limits),
        // The firmware checks the value against the keymap's layer count, which
        // is not part of this message. Any layer the keymap names is valid, and
        // one it does not still reads better as a layer than as a bare number.
        Some(ValueType::LayerId(_)) => true,
        Some(ValueType::Nil(_)) => value == 0,
        // A description this crate has no case for: not something we can vouch for.
        None => false,
    }
}

/// Mirrors the firmware's `validate_hid_usage`.
///
/// Note what is deliberately *not* checked: `keyboard_max` is the largest usage
/// the HID report can carry, and the modifier usages (`0xE0`–`0xE7`) sit above
/// it. The firmware lets those through — a home-row mod would be rejected
/// otherwise — so capping the keyboard page here would be wrong.
fn is_valid_hid_usage(value: u32, limits: &BehaviorParameterHidUsage) -> bool {
    let usage = HidUsage::from_encoded(value);
    match usage.page() {
        HID_USAGE_KEYBOARD => usage.id() != 0,
        HID_USAGE_CONSUMER => u32::from(usage.id()) <= limits.consumer_max,
        _ => false,
    }
}

/// Reads one value the way the description that covers it says to.
///
/// Descriptions are tried in the order the firmware sent them, matching the
/// order the firmware itself validates in: a value listed as a constant is a
/// constant even when a later alternative would also have taken it.
fn typed_param(values: &[BehaviorParameterValueDescription], value: u32) -> BehaviorParam {
    let Some(description) = values.iter().find(|d| describes(d, value)) else {
        // Either the behavior takes nothing here, or the keymap holds a value it
        // does not describe. Zero is the "no value" encoding in both cases;
        // anything else is worth showing, even untyped.
        return if value == 0 {
            BehaviorParam::Unused
        } else {
            BehaviorParam::Number(value)
        };
    };

    match description.value_type {
        Some(ValueType::HidUsage(_)) => BehaviorParam::Keycode(HidUsage::from_encoded(value)),
        Some(ValueType::LayerId(_)) => BehaviorParam::LayerId(value),
        Some(ValueType::Nil(_)) => BehaviorParam::Unused,
        _ => BehaviorParam::Number(value),
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keycode::Keycode;
    use crate::proto::zmk::behaviors::{
        BehaviorParameterHidUsage, BehaviorParameterLayerId, BehaviorParameterValueDescriptionRange,
    };

    fn described(value_type: ValueType) -> BehaviorParameterValueDescription {
        BehaviorParameterValueDescription {
            name: String::new(),
            value_type: Some(value_type),
        }
    }

    fn hid_usage() -> BehaviorParameterValueDescription {
        described(ValueType::HidUsage(BehaviorParameterHidUsage {
            keyboard_max: 0xFF,
            consumer_max: 0xFF,
        }))
    }

    fn layer_id() -> BehaviorParameterValueDescription {
        described(ValueType::LayerId(BehaviorParameterLayerId {}))
    }

    fn set(
        param1: Vec<BehaviorParameterValueDescription>,
        param2: Vec<BehaviorParameterValueDescription>,
    ) -> BehaviorBindingParametersSet {
        BehaviorBindingParametersSet { param1, param2 }
    }

    fn usage(keycode: Keycode) -> u32 {
        keycode.to_hid_usage()
    }

    /// A home-row mod (`hold-tap` over `&kp`/`&kp`) reports two key parameters.
    #[test]
    fn two_key_parameters_are_typed_as_keycodes() {
        let metadata = vec![set(vec![hid_usage()], vec![hid_usage()])];

        let params = typed_params(&metadata, usage(Keycode::LEFT_SHIFT), usage(Keycode::A));

        assert_eq!(
            params,
            (
                BehaviorParam::Keycode(HidUsage::from_encoded(usage(Keycode::LEFT_SHIFT))),
                BehaviorParam::Keycode(HidUsage::from_encoded(usage(Keycode::A))),
            )
        );
    }

    /// A `hold-tap` over `&mo`/`&kp` holds a layer and taps a key.
    #[test]
    fn layer_and_key_parameters_are_typed_separately() {
        let metadata = vec![set(vec![layer_id()], vec![hid_usage()])];

        let params = typed_params(&metadata, 2, usage(Keycode::D));

        assert_eq!(
            params,
            (
                BehaviorParam::LayerId(2),
                BehaviorParam::Keycode(HidUsage::from_encoded(usage(Keycode::D))),
            )
        );
    }

    /// A `hold-tap` whose tap side is a macro: the macro takes no parameter, so
    /// the hold-tap describes none for `param2` even though a zero is sent.
    #[test]
    fn parameter_without_description_is_unused() {
        let metadata = vec![set(vec![layer_id()], Vec::new())];

        let params = typed_params(&metadata, 5, 0);

        assert_eq!(params, (BehaviorParam::LayerId(5), BehaviorParam::Unused));
    }

    /// Tap-dances and other zero-parameter behaviors report no sets at all.
    #[test]
    fn behavior_without_metadata_has_no_parameters() {
        let params = typed_params(&[], 0, 0);

        assert_eq!(params, (BehaviorParam::Unused, BehaviorParam::Unused));
    }

    /// With one set per `param1` constant, the set matching `param1` types `param2`.
    #[test]
    fn parameter_set_is_chosen_by_first_parameter() {
        let metadata = vec![
            set(vec![described(ValueType::Constant(0))], Vec::new()),
            set(
                vec![described(ValueType::Constant(1))],
                vec![described(ValueType::Range(
                    BehaviorParameterValueDescriptionRange { min: 0, max: 4 },
                ))],
            ),
        ];

        assert_eq!(
            typed_params(&metadata, 1, 3),
            (BehaviorParam::Number(1), BehaviorParam::Number(3))
        );
        assert_eq!(
            typed_params(&metadata, 0, 0),
            (BehaviorParam::Number(0), BehaviorParam::Unused)
        );
    }

    /// The firmware validates a set on *both* parameters, so `param1` alone
    /// does not decide it: here the first set takes `param1` but not `param2`.
    #[test]
    fn parameter_set_is_chosen_by_both_parameters() {
        let metadata = vec![
            set(
                vec![described(ValueType::Range(
                    BehaviorParameterValueDescriptionRange { min: 0, max: 9 },
                ))],
                Vec::new(),
            ),
            set(
                vec![described(ValueType::Range(
                    BehaviorParameterValueDescriptionRange { min: 0, max: 9 },
                ))],
                vec![hid_usage()],
            ),
        ];

        assert_eq!(
            typed_params(&metadata, 3, usage(Keycode::A)),
            (
                BehaviorParam::Number(3),
                BehaviorParam::Keycode(HidUsage::from_encoded(usage(Keycode::A))),
            )
        );
    }

    /// A macro that passes only its second parameter along describes nothing for
    /// `param1`; the firmware still accepts the set, because the unused
    /// parameter is sent as zero.
    #[test]
    fn set_without_first_parameter_accepts_a_zero() {
        let metadata = vec![
            set(Vec::new(), vec![hid_usage()]),
            set(vec![layer_id()], Vec::new()),
        ];

        assert_eq!(
            typed_params(&metadata, 0, usage(Keycode::B)),
            (
                BehaviorParam::Unused,
                BehaviorParam::Keycode(HidUsage::from_encoded(usage(Keycode::B))),
            )
        );
    }

    /// A usage on no page the firmware accepts is not a keycode, so a set
    /// describing one does not cover it.
    #[test]
    fn usage_outside_the_supported_pages_is_not_a_keycode() {
        let metadata = vec![
            set(vec![hid_usage()], Vec::new()),
            set(
                vec![described(ValueType::Constant(0x00FF_0001))],
                Vec::new(),
            ),
        ];

        // Page 0xFF, id 1 — neither the keyboard nor the consumer page.
        assert_eq!(
            typed_params(&metadata, 0x00FF_0001, 0),
            (BehaviorParam::Number(0x00FF_0001), BehaviorParam::Unused)
        );
    }

    /// Consumer usages count as keycodes up to the maximum the device reports.
    #[test]
    fn consumer_usages_are_capped_at_the_reported_maximum() {
        let metadata = vec![set(vec![hid_usage()], Vec::new())];
        let in_range = (u32::from(HID_USAGE_CONSUMER) << 16) | 0xF0;
        let out_of_range = (u32::from(HID_USAGE_CONSUMER) << 16) | 0x100;

        assert_eq!(
            typed_params(&metadata, in_range, 0).0,
            BehaviorParam::Keycode(HidUsage::from_encoded(in_range))
        );
        assert_eq!(
            typed_params(&metadata, out_of_range, 0).0,
            BehaviorParam::Number(out_of_range)
        );
    }

    /// Modifier usages sit above the `keyboard_max` an NKRO build reports, and
    /// must still read as keycodes — or every home-row mod would come out as a
    /// bare number.
    #[test]
    fn modifier_usages_above_the_keyboard_maximum_are_keycodes() {
        // What an NKRO build reports: the last usage its report can carry.
        let nkro_max = described(ValueType::HidUsage(BehaviorParameterHidUsage {
            keyboard_max: 0x99,
            consumer_max: 0xFF,
        }));
        let metadata = vec![set(vec![nkro_max], Vec::new())];
        let right_gui = usage(Keycode::RIGHT_COMMAND);

        assert!(HidUsage::from_encoded(right_gui).id() > 0x99);
        assert_eq!(
            typed_params(&metadata, right_gui, 0).0,
            BehaviorParam::Keycode(HidUsage::from_encoded(right_gui))
        );
    }

    /// Descriptions are tried in the order the firmware sent them, so a value
    /// listed as a constant stays a constant.
    #[test]
    fn the_first_matching_description_types_the_value() {
        let metadata = vec![set(
            vec![described(ValueType::Constant(0x0007_0004)), hid_usage()],
            Vec::new(),
        )];

        assert_eq!(
            typed_params(&metadata, 0x0007_0004, 0).0,
            BehaviorParam::Number(0x0007_0004)
        );
    }

    /// An unmatched `param1` still resolves against the first set rather than
    /// losing both values.
    #[test]
    fn unmatched_first_parameter_falls_back_to_first_set() {
        let metadata = vec![set(vec![described(ValueType::Constant(7))], Vec::new())];

        assert_eq!(
            typed_params(&metadata, 9, 0),
            (BehaviorParam::Number(9), BehaviorParam::Unused)
        );
    }

    #[test]
    fn typed_parameters_round_trip_to_raw_values() {
        let raw = usage(Keycode::LEFT_SHIFT);

        assert_eq!(
            BehaviorParam::Keycode(HidUsage::from_encoded(raw)).to_raw(),
            raw
        );
        assert_eq!(BehaviorParam::LayerId(3).to_raw(), 3);
        assert_eq!(BehaviorParam::Number(9).to_raw(), 9);
        assert_eq!(BehaviorParam::Unused.to_raw(), 0);
    }
}
