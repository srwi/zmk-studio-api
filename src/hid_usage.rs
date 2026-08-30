use std::fmt;

use crate::keycode::Keycode;

pub const HID_USAGE_KEYBOARD: u16 = 0x07;
pub const HID_USAGE_CONSUMER: u16 = 0x0C;

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

    /// Encodes an 8-bit modifier bitmask into a `HidUsage`.
    ///
    /// Single-modifier masks encode as a keyboard usage (`0xE0..=0xE7`).
    /// Multi-modifier masks encode with usage ID 0 and the modifier byte set.
    pub fn from_modifier_mask(mask: u8) -> Self {
        if mask.count_ones() == 1 {
            Self::from_parts(HID_USAGE_KEYBOARD, 0xE0 + mask.trailing_zeros() as u16, 0)
        } else {
            Self::from_parts(HID_USAGE_KEYBOARD, 0, mask)
        }
    }

    /// Extracts the 8-bit modifier bitmask whether stored as a modifier usage (`0xE0..=0xE7`)
    /// or in the modifier byte.
    pub fn modifier_mask(self) -> u8 {
        if self.modifiers != 0 {
            return self.modifiers;
        }
        if self.page == HID_USAGE_KEYBOARD && (0xE0..=0xE7).contains(&self.id) {
            1 << (self.id - 0xE0)
        } else {
            0
        }
    }

    /// Returns `true` if this usage represents a modifier key or carries modifier flags.
    pub fn is_modifier(self) -> bool {
        self.modifier_mask() != 0
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

impl From<Keycode> for HidUsage {
    fn from(code: Keycode) -> Self {
        Self::from_encoded(code as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_modifier_round_trip_mask() {
        let single_mods = [
            (MOD_LCTL, 0xE0),
            (MOD_LSFT, 0xE1),
            (MOD_LALT, 0xE2),
            (MOD_LGUI, 0xE3),
            (MOD_RCTL, 0xE4),
            (MOD_RSFT, 0xE5),
            (MOD_RALT, 0xE6),
            (MOD_RGUI, 0xE7),
        ];

        for (mask, expected_id) in single_mods {
            let usage = HidUsage::from_modifier_mask(mask);
            assert_eq!(usage.page(), HID_USAGE_KEYBOARD);
            assert_eq!(usage.id(), expected_id);
            assert_eq!(usage.modifiers(), 0);
            assert_eq!(usage.modifier_mask(), mask);
            assert!(usage.is_modifier());
        }
    }

    #[test]
    fn multi_modifier_round_trip_mask() {
        let mask = MOD_LSFT | MOD_LCTL | MOD_LALT;
        let usage = HidUsage::from_modifier_mask(mask);
        assert_eq!(usage.page(), HID_USAGE_KEYBOARD);
        assert_eq!(usage.id(), 0);
        assert_eq!(usage.modifiers(), mask);
        assert_eq!(usage.modifier_mask(), mask);
        assert!(usage.is_modifier());
    }

    #[test]
    fn zero_modifier_mask() {
        let usage = HidUsage::from_modifier_mask(0);
        assert_eq!(usage.modifier_mask(), 0);
        assert!(!usage.is_modifier());
    }

    #[test]
    fn regular_keycode_is_not_modifier() {
        let usage = HidUsage::from(Keycode::A);
        assert_eq!(usage.modifier_mask(), 0);
        assert!(!usage.is_modifier());

        let usage_with_mod = HidUsage::from_parts(HID_USAGE_KEYBOARD, 0x04, MOD_LSFT);
        assert_eq!(usage_with_mod.modifier_mask(), MOD_LSFT);
        assert!(usage_with_mod.is_modifier());
    }
}
