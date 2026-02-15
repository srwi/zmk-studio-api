//! High-level Rust client for the ZMK Studio RPC protocol.
//!
//! The recommended API surface is:
//! - [`StudioClient`] for RPC operations
//! - [`Behavior`] for typed key bindings
//! - [`HidUsage`] and [`Keycode`] for ZMK key values
//! - [`transport`] for BLE/serial I/O adapters
//!
//! [`proto`] exposes raw generated protobuf types for advanced use cases.

mod binding;
mod client;
mod framing;
mod hid_usage;
mod keycode;
/// Raw generated protobuf types used by the RPC protocol.
pub mod proto;
mod protocol;
#[cfg(feature = "python")]
mod python;
/// Transport adapters for connecting to a ZMK Studio-capable device.
pub mod transport;

/// Typed key binding value used by [`StudioClient::get_key_at`] and [`StudioClient::set_key_at`].
pub use binding::Behavior;
/// Errors returned by high-level client operations.
pub use client::{ClientError, StudioClient};
/// Decoded ZMK HID usage values used in typed behavior APIs.
pub use hid_usage::{
    HID_USAGE_KEYBOARD, HidUsage, MOD_LALT, MOD_LCTL, MOD_LGUI, MOD_LSFT, MOD_RALT, MOD_RCTL,
    MOD_RGUI, MOD_RSFT,
};
/// ZMK keycode enum used in typed behavior APIs.
pub use keycode::Keycode;
