use std::fmt;

use crate::keycode::Keycode;

pub const HID_USAGE_KEYBOARD: u16 = 0x07;

pub const MOD_LCTL: u8 = 0x01;
pub const MOD_LSFT: u8 = 0x02;
pub const MOD_LALT: u8 = 0x04;
pub const MOD_LGUI: u8 = 0x08;
pub const MOD_RCTL: u8 = 0x10;
pub const MOD_RSFT: u8 = 0x20;
pub const MOD_RALT: u8 = 0x40;
pub const MOD_RGUI: u8 = 0x80;

/// Lossless decoded ZMK HID usage value (base usage + modifiers).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HidUsage {
    page: u16,
    id: u16,
    modifiers: u8,
}

impl HidUsage {
    /// Decode from ZMK's encoded usage format.
    ///
    /// ZMK encodes as:
    /// - bits 31:24: modifiers
    /// - bits 23:16: usage page
    /// - bits 15:00: usage id
    ///
    /// If page is 0, ZMK treats it as keyboard page (`0x07`).
    pub fn from_encoded(encoded: u32) -> Self {
        let mut page = ((encoded >> 16) & 0xFF) as u16;
        if page == 0 {
            page = HID_USAGE_KEYBOARD;
        }

        Self {
            page,
            id: (encoded & 0xFFFF) as u16,
            modifiers: (encoded >> 24) as u8,
        }
    }

    pub fn from_parts(page: u16, id: u16, modifiers: u8) -> Self {
        Self {
            page,
            id,
            modifiers,
        }
    }

    pub fn to_hid_usage(self) -> u32 {
        ((self.modifiers as u32) << 24) | ((self.page as u32) << 16) | self.id as u32
    }

    pub fn page(self) -> u16 {
        self.page
    }

    pub fn id(self) -> u16 {
        self.id
    }

    pub fn modifiers(self) -> u8 {
        self.modifiers
    }

    pub fn base(self) -> Self {
        Self {
            page: self.page,
            id: self.id,
            modifiers: 0,
        }
    }

    pub fn known_keycode(self) -> Option<Keycode> {
        Keycode::from_hid_usage(self.to_hid_usage())
    }

    pub fn known_base_keycode(self) -> Option<Keycode> {
        Keycode::from_hid_usage(self.base().to_hid_usage())
    }

    pub fn modifier_labels(self) -> Vec<&'static str> {
        let mut labels = Vec::new();
        let mods = self.modifiers;
        if mods & MOD_LCTL != 0 {
            labels.push("LCTL");
        }
        if mods & MOD_LSFT != 0 {
            labels.push("LSFT");
        }
        if mods & MOD_LALT != 0 {
            labels.push("LALT");
        }
        if mods & MOD_LGUI != 0 {
            labels.push("LGUI");
        }
        if mods & MOD_RCTL != 0 {
            labels.push("RCTL");
        }
        if mods & MOD_RSFT != 0 {
            labels.push("RSFT");
        }
        if mods & MOD_RALT != 0 {
            labels.push("RALT");
        }
        if mods & MOD_RGUI != 0 {
            labels.push("RGUI");
        }
        labels
    }
}

impl fmt::Display for HidUsage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(keycode) = self.known_keycode() {
            return f.write_str(keycode.to_name());
        }

        write!(
            f,
            "0x{:02X}{:02X}{:02X}{:02X}",
            self.modifiers,
            self.page,
            (self.id >> 8) as u8,
            self.id as u8
        )
    }
}
