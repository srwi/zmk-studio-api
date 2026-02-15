use crate::keycode::Keycode;
use crate::proto::zmk;
#[cfg(feature = "python")]
use pyo3::exceptions::{PyTypeError, PyValueError};
#[cfg(feature = "python")]
use pyo3::prelude::*;
#[cfg(feature = "python")]
use pyo3::types::{PyAny, PyDict};

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

/// Typed behavior value for a single key binding.
///
/// Used by [`crate::StudioClient::get_key_at`] and [`crate::StudioClient::set_key_at`].
/// Unknown or unmapped bindings are represented as [`Behavior::Raw`].
#[derive(Debug, Clone, PartialEq)]
pub enum Behavior {
    KeyPress(Keycode),
    KeyToggle(Keycode),
    LayerTap { layer_id: u32, tap: Keycode },
    ModTap { hold: Keycode, tap: Keycode },
    StickyKey(Keycode),
    StickyLayer { layer_id: u32 },
    MomentaryLayer { layer_id: u32 },
    ToggleLayer { layer_id: u32 },
    ToLayer { layer_id: u32 },
    Bluetooth { command: u32, value: u32 },
    ExternalPower { value: u32 },
    OutputSelection { value: u32 },
    Backlight { command: u32, value: u32 },
    Underglow { command: u32, value: u32 },
    MouseKeyPress { value: u32 },
    MouseMove { value: u32 },
    MouseScroll { value: u32 },
    CapsWord,
    KeyRepeat,
    Reset,
    Bootloader,
    SoftOff,
    StudioUnlock,
    GraveEscape,
    Transparent,
    None,
    Raw(zmk::keymap::BehaviorBinding),
}

#[cfg(feature = "python")]
impl Behavior {
    pub fn to_python<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let dict = PyDict::new(py);
        match self {
            Self::KeyPress(key) => {
                dict.set_item("kind", "key_press")?;
                dict.set_item("key", key.to_hid_usage())?;
            }
            Self::KeyToggle(key) => {
                dict.set_item("kind", "key_toggle")?;
                dict.set_item("key", key.to_hid_usage())?;
            }
            Self::LayerTap { layer_id, tap } => {
                dict.set_item("kind", "layer_tap")?;
                dict.set_item("layer_id", layer_id)?;
                dict.set_item("tap", tap.to_hid_usage())?;
            }
            Self::ModTap { hold, tap } => {
                dict.set_item("kind", "mod_tap")?;
                dict.set_item("hold", hold.to_hid_usage())?;
                dict.set_item("tap", tap.to_hid_usage())?;
            }
            Self::StickyKey(key) => {
                dict.set_item("kind", "sticky_key")?;
                dict.set_item("key", key.to_hid_usage())?;
            }
            Self::StickyLayer { layer_id } => {
                dict.set_item("kind", "sticky_layer")?;
                dict.set_item("layer_id", layer_id)?;
            }
            Self::MomentaryLayer { layer_id } => {
                dict.set_item("kind", "momentary_layer")?;
                dict.set_item("layer_id", layer_id)?;
            }
            Self::ToggleLayer { layer_id } => {
                dict.set_item("kind", "toggle_layer")?;
                dict.set_item("layer_id", layer_id)?;
            }
            Self::ToLayer { layer_id } => {
                dict.set_item("kind", "to_layer")?;
                dict.set_item("layer_id", layer_id)?;
            }
            Self::Bluetooth { command, value } => {
                dict.set_item("kind", "bluetooth")?;
                dict.set_item("command", command)?;
                dict.set_item("value", value)?;
            }
            Self::ExternalPower { value } => {
                dict.set_item("kind", "external_power")?;
                dict.set_item("value", value)?;
            }
            Self::OutputSelection { value } => {
                dict.set_item("kind", "output_selection")?;
                dict.set_item("value", value)?;
            }
            Self::Backlight { command, value } => {
                dict.set_item("kind", "backlight")?;
                dict.set_item("command", command)?;
                dict.set_item("value", value)?;
            }
            Self::Underglow { command, value } => {
                dict.set_item("kind", "underglow")?;
                dict.set_item("command", command)?;
                dict.set_item("value", value)?;
            }
            Self::MouseKeyPress { value } => {
                dict.set_item("kind", "mouse_key_press")?;
                dict.set_item("value", value)?;
            }
            Self::MouseMove { value } => {
                dict.set_item("kind", "mouse_move")?;
                dict.set_item("value", value)?;
            }
            Self::MouseScroll { value } => {
                dict.set_item("kind", "mouse_scroll")?;
                dict.set_item("value", value)?;
            }
            Self::CapsWord => dict.set_item("kind", "caps_word")?,
            Self::KeyRepeat => dict.set_item("kind", "key_repeat")?,
            Self::Reset => dict.set_item("kind", "reset")?,
            Self::Bootloader => dict.set_item("kind", "bootloader")?,
            Self::SoftOff => dict.set_item("kind", "soft_off")?,
            Self::StudioUnlock => dict.set_item("kind", "studio_unlock")?,
            Self::GraveEscape => dict.set_item("kind", "grave_escape")?,
            Self::Transparent => dict.set_item("kind", "transparent")?,
            Self::None => dict.set_item("kind", "none")?,
            Self::Raw(raw) => {
                dict.set_item("kind", "raw")?;
                dict.set_item("behavior_id", raw.behavior_id)?;
                dict.set_item("param1", raw.param1)?;
                dict.set_item("param2", raw.param2)?;
            }
        }
        Ok(dict.into_any())
    }

    pub fn from_python(value: &Bound<'_, PyAny>) -> PyResult<Self> {
        let dict = value
            .downcast::<PyDict>()
            .map_err(|_| PyTypeError::new_err("behavior must be a dict with a 'kind' field"))?;
        let kind: String = required_item(dict, "kind")?.extract()?;

        match kind.as_str() {
            "key_press" => Ok(Self::KeyPress(required_keycode(dict, "key")?)),
            "key_toggle" => Ok(Self::KeyToggle(required_keycode(dict, "key")?)),
            "layer_tap" => Ok(Self::LayerTap {
                layer_id: required_item(dict, "layer_id")?.extract()?,
                tap: required_keycode(dict, "tap")?,
            }),
            "mod_tap" => Ok(Self::ModTap {
                hold: required_keycode(dict, "hold")?,
                tap: required_keycode(dict, "tap")?,
            }),
            "sticky_key" => Ok(Self::StickyKey(required_keycode(dict, "key")?)),
            "sticky_layer" => Ok(Self::StickyLayer {
                layer_id: required_item(dict, "layer_id")?.extract()?,
            }),
            "momentary_layer" => Ok(Self::MomentaryLayer {
                layer_id: required_item(dict, "layer_id")?.extract()?,
            }),
            "toggle_layer" => Ok(Self::ToggleLayer {
                layer_id: required_item(dict, "layer_id")?.extract()?,
            }),
            "to_layer" => Ok(Self::ToLayer {
                layer_id: required_item(dict, "layer_id")?.extract()?,
            }),
            "bluetooth" => Ok(Self::Bluetooth {
                command: required_item(dict, "command")?.extract()?,
                value: required_item(dict, "value")?.extract()?,
            }),
            "external_power" => Ok(Self::ExternalPower {
                value: required_item(dict, "value")?.extract()?,
            }),
            "output_selection" => Ok(Self::OutputSelection {
                value: required_item(dict, "value")?.extract()?,
            }),
            "backlight" => Ok(Self::Backlight {
                command: required_item(dict, "command")?.extract()?,
                value: required_item(dict, "value")?.extract()?,
            }),
            "underglow" => Ok(Self::Underglow {
                command: required_item(dict, "command")?.extract()?,
                value: required_item(dict, "value")?.extract()?,
            }),
            "mouse_key_press" => Ok(Self::MouseKeyPress {
                value: required_item(dict, "value")?.extract()?,
            }),
            "mouse_move" => Ok(Self::MouseMove {
                value: required_item(dict, "value")?.extract()?,
            }),
            "mouse_scroll" => Ok(Self::MouseScroll {
                value: required_item(dict, "value")?.extract()?,
            }),
            "caps_word" => Ok(Self::CapsWord),
            "key_repeat" => Ok(Self::KeyRepeat),
            "reset" => Ok(Self::Reset),
            "bootloader" => Ok(Self::Bootloader),
            "soft_off" => Ok(Self::SoftOff),
            "studio_unlock" => Ok(Self::StudioUnlock),
            "grave_escape" => Ok(Self::GraveEscape),
            "transparent" => Ok(Self::Transparent),
            "none" => Ok(Self::None),
            "raw" => Ok(Self::Raw(zmk::keymap::BehaviorBinding {
                behavior_id: required_item(dict, "behavior_id")?.extract()?,
                param1: required_item(dict, "param1")?.extract()?,
                param2: required_item(dict, "param2")?.extract()?,
            })),
            _ => Err(PyValueError::new_err(format!(
                "unsupported behavior kind '{kind}'"
            ))),
        }
    }
}

#[cfg(feature = "python")]
fn required_item<'py>(dict: &Bound<'py, PyDict>, field: &str) -> PyResult<Bound<'py, PyAny>> {
    dict.get_item(field)?
        .ok_or_else(|| PyValueError::new_err(format!("missing '{field}'")))
}

#[cfg(feature = "python")]
fn required_keycode(dict: &Bound<'_, PyDict>, field: &str) -> PyResult<Keycode> {
    let value = dict
        .get_item(field)?
        .ok_or_else(|| PyValueError::new_err(format!("missing '{field}'")))?;

    if let Ok(encoded) = value.extract::<u32>() {
        return Keycode::from_hid_usage(encoded).ok_or_else(|| {
            PyValueError::new_err(format!(
                "invalid keycode HID usage value for '{field}': {encoded}"
            ))
        });
    }

    if let Ok(name) = value.extract::<String>() {
        return Keycode::from_name(&name).ok_or_else(|| {
            PyValueError::new_err(format!("invalid keycode name for '{field}': {name}"))
        });
    }

    Err(PyTypeError::new_err(format!(
        "field '{field}' must be a keycode name (str) or HID usage value (int)"
    )))
}

pub fn role_from_display_name(name: &str) -> Option<BehaviorRole> {
    let n = name.trim().to_ascii_lowercase();
    match n.as_str() {
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
        "mouse move" => Some(BehaviorRole::MouseMove),
        "mouse scroll" => Some(BehaviorRole::MouseScroll),
        "caps word" => Some(BehaviorRole::CapsWord),
        "key repeat" => Some(BehaviorRole::KeyRepeat),
        "reset" => Some(BehaviorRole::Reset),
        "bootloader" => Some(BehaviorRole::Bootloader),
        "soft off" => Some(BehaviorRole::SoftOff),
        "studio unlock" => Some(BehaviorRole::StudioUnlock),
        "grave escape" => Some(BehaviorRole::GraveEscape),
        "transparent" => Some(BehaviorRole::Transparent),
        "none" => Some(BehaviorRole::None),
        _ => None,
    }
}
