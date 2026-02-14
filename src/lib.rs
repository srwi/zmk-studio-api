//! High-level Rust client for the ZMK Studio RPC protocol.
//!
//! The recommended API surface is:
//! - [`StudioClient`] for RPC operations
//! - [`Behavior`] for typed key bindings
//! - [`Keycode`] for ZMK key values
//! - [`transport`] for BLE/serial I/O adapters
//!
//! [`proto`] exposes raw generated protobuf types for advanced use cases.

mod binding;
mod client;
mod framing;
mod keycode;
/// Raw generated protobuf types used by the RPC protocol.
pub mod proto;
mod protocol;
/// Transport adapters for connecting to a ZMK Studio-capable device.
pub mod transport;

/// Typed key binding value used by [`StudioClient::get_key_at`] and [`StudioClient::set_key_at`].
pub use binding::Behavior;
/// Errors returned by high-level client operations.
pub use client::{ClientError, StudioClient};
/// ZMK keycode enum used in typed behavior APIs.
pub use keycode::Keycode;
