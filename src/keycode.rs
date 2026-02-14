//! Helpers for HID usage encoding and typed keycodes.

pub const HID_USAGE_PAGE_GENERIC_DESKTOP: u16 = 0x01;
pub const HID_USAGE_PAGE_KEYBOARD: u16 = 0x07;
pub const HID_USAGE_PAGE_CONSUMER: u16 = 0x0C;

#[path = "keycode_zmk_generated.rs"]
mod generated_keys;

pub mod keycodes {
    pub use super::generated_keys::*;
}

pub use keycodes as zmk_keys;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HidUsage {
    pub page: u16,
    pub id: u16,
}

impl HidUsage {
    pub fn encode(self) -> u32 {
        ((self.page as u32) << 16) | (self.id as u32)
    }

    pub fn decode(encoded: u32) -> Self {
        Self {
            page: ((encoded >> 16) & 0xFF) as u16,
            id: (encoded & 0xFFFF) as u16,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyCode {
    Keyboard(KeyboardCode),
    Consumer(u16),
    GenericDesktop(u16),
    Other(HidUsage),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ZmkKeycode(u32);

impl ZmkKeycode {
    pub const fn from_raw(raw: u32) -> Self {
        Self(raw)
    }

    pub const fn raw(self) -> u32 {
        self.0
    }

    pub fn from_name(name: &str) -> Option<Self> {
        zmk_keys::Keycode::from_name(name).map(|k| Self(k.raw()))
    }

    pub fn name(self) -> Option<&'static str> {
        zmk_keys::Keycode::try_from(self.0)
            .ok()
            .map(<&'static str>::from)
    }

    pub fn to_key_code(self) -> KeyCode {
        KeyCode::from_hid_usage(self.0)
    }
}

impl From<ZmkKeycode> for KeyCode {
    fn from(value: ZmkKeycode) -> Self {
        value.to_key_code()
    }
}

impl From<KeyCode> for ZmkKeycode {
    fn from(value: KeyCode) -> Self {
        Self(value.to_hid_usage())
    }
}

pub fn is_keyboard_usage(encoded: u32) -> bool {
    HidUsage::decode(encoded).page == HID_USAGE_PAGE_KEYBOARD
}

impl KeyCode {
    pub fn to_hid_usage(self) -> u32 {
        match self {
            Self::Keyboard(kbd) => HidUsage {
                page: HID_USAGE_PAGE_KEYBOARD,
                id: kbd.usage_id(),
            }
            .encode(),
            Self::Consumer(id) => HidUsage {
                page: HID_USAGE_PAGE_CONSUMER,
                id,
            }
            .encode(),
            Self::GenericDesktop(id) => HidUsage {
                page: HID_USAGE_PAGE_GENERIC_DESKTOP,
                id,
            }
            .encode(),
            Self::Other(raw) => raw.encode(),
        }
    }

    pub fn from_hid_usage(encoded: u32) -> Self {
        let raw = HidUsage::decode(encoded);
        match raw.page {
            HID_USAGE_PAGE_KEYBOARD => Self::Keyboard(KeyboardCode::from_usage_id(raw.id)),
            HID_USAGE_PAGE_CONSUMER => Self::Consumer(raw.id),
            HID_USAGE_PAGE_GENERIC_DESKTOP => Self::GenericDesktop(raw.id),
            _ => Self::Other(raw),
        }
    }

    pub fn from_zmk_name(name: &str) -> Option<Self> {
        ZmkKeycode::from_name(name).map(|k| Self::from_hid_usage(k.raw()))
    }

    pub fn to_zmk_name(self) -> Option<&'static str> {
        ZmkKeycode::from(self).name()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum KeyboardCode {
    Modifier(ModifierKey),
    UsageId(u16),
}

impl KeyboardCode {
    pub fn usage_id(self) -> u16 {
        match self {
            Self::Modifier(m) => m.usage_id(),
            Self::UsageId(id) => id,
        }
    }

    pub fn from_usage_id(id: u16) -> Self {
        if let Some(modifier) = ModifierKey::from_usage_id(id) {
            return Self::Modifier(modifier);
        }
        Self::UsageId(id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModifierKey {
    LeftControl,
    LeftShift,
    LeftAlt,
    LeftGui,
    RightControl,
    RightShift,
    RightAlt,
    RightGui,
}

impl ModifierKey {
    pub fn usage_id(self) -> u16 {
        match self {
            Self::LeftControl => 224,
            Self::LeftShift => 225,
            Self::LeftAlt => 226,
            Self::LeftGui => 227,
            Self::RightControl => 228,
            Self::RightShift => 229,
            Self::RightAlt => 230,
            Self::RightGui => 231,
        }
    }

    pub fn from_usage_id(id: u16) -> Option<Self> {
        match id {
            224 => Some(Self::LeftControl),
            225 => Some(Self::LeftShift),
            226 => Some(Self::LeftAlt),
            227 => Some(Self::LeftGui),
            228 => Some(Self::RightControl),
            229 => Some(Self::RightShift),
            230 => Some(Self::RightAlt),
            231 => Some(Self::RightGui),
            _ => None,
        }
    }
}
