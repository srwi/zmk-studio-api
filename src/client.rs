use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};

use crate::binding::{Behavior, BehaviorRole, role_from_display_name};
use crate::framing::FrameDecoder;
use crate::keycode::Keycode;
use crate::proto::zmk;
use crate::proto::zmk::studio;
use crate::protocol::{ProtocolError, decode_responses, encode_request};
#[cfg(feature = "ble")]
use crate::transport::ble::{BleConnectOptions, BleTransport, BleTransportError};
#[cfg(feature = "serial")]
use crate::transport::serial::{SerialTransport, SerialTransportError};

#[derive(Debug)]
pub enum ClientError {
    Io(std::io::Error),
    Protocol(ProtocolError),
    Meta(zmk::meta::ErrorConditions),
    NoResponse,
    MissingResponseType,
    MissingSubsystem,
    UnexpectedSubsystem(&'static str),
    UnexpectedRequestId { expected: u32, actual: u32 },
    UnknownEnumValue { field: &'static str, value: i32 },
    SetLayerBindingFailed(zmk::keymap::SetLayerBindingResponse),
    SaveChangesFailed(zmk::keymap::SaveChangesErrorCode),
    SetActivePhysicalLayoutFailed(zmk::keymap::SetActivePhysicalLayoutErrorCode),
    MoveLayerFailed(zmk::keymap::MoveLayerErrorCode),
    AddLayerFailed(zmk::keymap::AddLayerErrorCode),
    RemoveLayerFailed(zmk::keymap::RemoveLayerErrorCode),
    RestoreLayerFailed(zmk::keymap::RestoreLayerErrorCode),
    SetLayerPropsFailed(zmk::keymap::SetLayerPropsResponse),
    InvalidLayerOrPosition { layer_id: u32, key_position: i32 },
    MissingBehaviorRole(&'static str),
    BehaviorIdOutOfRange { behavior_id: u32 },
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(err) => write!(f, "I/O error: {err}"),
            Self::Protocol(err) => write!(f, "Protocol error: {err}"),
            Self::Meta(cond) => write!(f, "Device returned meta error: {}", cond.as_str_name()),
            Self::NoResponse => write!(f, "Device returned no response"),
            Self::MissingResponseType => write!(f, "Response was missing type"),
            Self::MissingSubsystem => write!(f, "Request response was missing subsystem"),
            Self::UnexpectedSubsystem(expected) => {
                write!(f, "Unexpected subsystem in response; expected {expected}")
            }
            Self::UnexpectedRequestId { expected, actual } => {
                write!(
                    f,
                    "Unexpected request ID in response: expected {expected}, got {actual}"
                )
            }
            Self::UnknownEnumValue { field, value } => {
                write!(f, "Unknown enum value for {field}: {value}")
            }
            Self::SetLayerBindingFailed(code) => {
                write!(f, "Set layer binding failed: {}", code.as_str_name())
            }
            Self::SaveChangesFailed(code) => {
                write!(f, "Save changes failed: {}", code.as_str_name())
            }
            Self::SetActivePhysicalLayoutFailed(code) => {
                write!(
                    f,
                    "Set active physical layout failed: {}",
                    code.as_str_name()
                )
            }
            Self::MoveLayerFailed(code) => write!(f, "Move layer failed: {}", code.as_str_name()),
            Self::AddLayerFailed(code) => write!(f, "Add layer failed: {}", code.as_str_name()),
            Self::RemoveLayerFailed(code) => {
                write!(f, "Remove layer failed: {}", code.as_str_name())
            }
            Self::RestoreLayerFailed(code) => {
                write!(f, "Restore layer failed: {}", code.as_str_name())
            }
            Self::SetLayerPropsFailed(code) => {
                write!(f, "Set layer properties failed: {}", code.as_str_name())
            }
            Self::InvalidLayerOrPosition {
                layer_id,
                key_position,
            } => write!(
                f,
                "Invalid layer/position: layer_id={layer_id}, key_position={key_position}"
            ),
            Self::MissingBehaviorRole(role) => {
                write!(f, "Missing required behavior role in firmware: {role}")
            }
            Self::BehaviorIdOutOfRange { behavior_id } => {
                write!(f, "Behavior ID is out of i32 range: {behavior_id}")
            }
        }
    }
}

impl std::error::Error for ClientError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            Self::Protocol(err) => Some(err),
            _ => None,
        }
    }
}

impl From<std::io::Error> for ClientError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<ProtocolError> for ClientError {
    fn from(value: ProtocolError) -> Self {
        Self::Protocol(value)
    }
}

pub struct StudioClient<T> {
    io: T,
    next_request_id: u32,
    decoder: FrameDecoder,
    read_buffer: Vec<u8>,
    responses: VecDeque<studio::Response>,
    notifications: VecDeque<studio::Notification>,
    behavior_role_by_id: HashMap<u32, BehaviorRole>,
    behavior_id_by_role: HashMap<BehaviorRole, u32>,
}

impl<T: Read + Write> StudioClient<T> {
    pub fn new(io: T) -> Self {
        Self::with_read_buffer(io, 256)
    }

    pub fn with_read_buffer(io: T, read_buffer_size: usize) -> Self {
        Self {
            io,
            next_request_id: 0,
            decoder: FrameDecoder::new(),
            read_buffer: vec![0; read_buffer_size.max(1)],
            responses: VecDeque::new(),
            notifications: VecDeque::new(),
            behavior_role_by_id: HashMap::new(),
            behavior_id_by_role: HashMap::new(),
        }
    }

    pub fn into_inner(self) -> T {
        self.io
    }

    pub fn next_notification(&mut self) -> Option<studio::Notification> {
        self.notifications.pop_front()
    }

    pub fn read_notification_blocking(&mut self) -> Result<studio::Notification, ClientError> {
        loop {
            if let Some(notification) = self.next_notification() {
                return Ok(notification);
            }

            let _ = self.read_next_response()?;
        }
    }

    pub fn get_device_info(&mut self) -> Result<zmk::core::GetDeviceInfoResponse, ClientError> {
        let response = self.call_core(zmk::core::request::RequestType::GetDeviceInfo(true))?;
        match response.response_type {
            Some(zmk::core::response::ResponseType::GetDeviceInfo(info)) => Ok(info),
            _ => Err(ClientError::MissingResponseType),
        }
    }

    pub fn get_lock_state(&mut self) -> Result<zmk::core::LockState, ClientError> {
        let response = self.call_core(zmk::core::request::RequestType::GetLockState(true))?;
        match response.response_type {
            Some(zmk::core::response::ResponseType::GetLockState(state)) => {
                zmk::core::LockState::try_from(state).map_err(|_| ClientError::UnknownEnumValue {
                    field: "core.get_lock_state",
                    value: state,
                })
            }
            _ => Err(ClientError::MissingResponseType),
        }
    }

    pub fn reset_settings(&mut self) -> Result<bool, ClientError> {
        let response = self.call_core(zmk::core::request::RequestType::ResetSettings(true))?;
        match response.response_type {
            Some(zmk::core::response::ResponseType::ResetSettings(ok)) => Ok(ok),
            _ => Err(ClientError::MissingResponseType),
        }
    }

    pub fn list_all_behaviors(&mut self) -> Result<Vec<u32>, ClientError> {
        let response =
            self.call_behaviors(zmk::behaviors::request::RequestType::ListAllBehaviors(true))?;
        match response.response_type {
            Some(zmk::behaviors::response::ResponseType::ListAllBehaviors(items)) => {
                Ok(items.behaviors)
            }
            _ => Err(ClientError::MissingResponseType),
        }
    }

    pub fn get_behavior_details(
        &mut self,
        behavior_id: u32,
    ) -> Result<zmk::behaviors::GetBehaviorDetailsResponse, ClientError> {
        let request = zmk::behaviors::GetBehaviorDetailsRequest { behavior_id };
        let response = self.call_behaviors(
            zmk::behaviors::request::RequestType::GetBehaviorDetails(request),
        )?;
        match response.response_type {
            Some(zmk::behaviors::response::ResponseType::GetBehaviorDetails(details)) => {
                Ok(details)
            }
            _ => Err(ClientError::MissingResponseType),
        }
    }

    pub fn get_keymap(&mut self) -> Result<zmk::keymap::Keymap, ClientError> {
        let response = self.call_keymap(zmk::keymap::request::RequestType::GetKeymap(true))?;
        match response.response_type {
            Some(zmk::keymap::response::ResponseType::GetKeymap(keymap)) => Ok(keymap),
            _ => Err(ClientError::MissingResponseType),
        }
    }

    pub fn get_physical_layouts(&mut self) -> Result<zmk::keymap::PhysicalLayouts, ClientError> {
        let response =
            self.call_keymap(zmk::keymap::request::RequestType::GetPhysicalLayouts(true))?;
        match response.response_type {
            Some(zmk::keymap::response::ResponseType::GetPhysicalLayouts(layouts)) => Ok(layouts),
            _ => Err(ClientError::MissingResponseType),
        }
    }

    pub fn set_layer_binding(
        &mut self,
        layer_id: u32,
        key_position: i32,
        binding: zmk::keymap::BehaviorBinding,
    ) -> Result<(), ClientError> {
        let request = zmk::keymap::SetLayerBindingRequest {
            layer_id,
            key_position,
            binding: Some(binding),
        };

        let response =
            self.call_keymap(zmk::keymap::request::RequestType::SetLayerBinding(request))?;

        match response.response_type {
            Some(zmk::keymap::response::ResponseType::SetLayerBinding(raw)) => {
                let code = zmk::keymap::SetLayerBindingResponse::try_from(raw).map_err(|_| {
                    ClientError::UnknownEnumValue {
                        field: "keymap.set_layer_binding",
                        value: raw,
                    }
                })?;

                if code == zmk::keymap::SetLayerBindingResponse::SetLayerBindingRespOk {
                    Ok(())
                } else {
                    Err(ClientError::SetLayerBindingFailed(code))
                }
            }
            _ => Err(ClientError::MissingResponseType),
        }
    }

    /// Typed keymap API: read a behavior from a specific layer/key position.
    pub fn get_key_at(
        &mut self,
        layer_id: u32,
        key_position: i32,
    ) -> Result<Behavior, ClientError> {
        self.ensure_behavior_catalog()?;

        let keymap = self.get_keymap()?;
        let binding = binding_at(&keymap, layer_id, key_position).ok_or(
            ClientError::InvalidLayerOrPosition {
                layer_id,
                key_position,
            },
        )?;

        let Ok(binding_behavior_id) = u32::try_from(binding.behavior_id) else {
            return Ok(Behavior::Raw(binding));
        };
        let Some(role) = self.behavior_role_by_id.get(&binding_behavior_id).copied() else {
            return Ok(Behavior::Raw(binding));
        };

        let behavior = match role {
            BehaviorRole::KeyPress => match Keycode::from_hid_usage(binding.param1) {
                Some(key) => Behavior::KeyPress(key),
                None => Behavior::Raw(binding),
            },
            BehaviorRole::KeyToggle => match Keycode::from_hid_usage(binding.param1) {
                Some(key) => Behavior::KeyToggle(key),
                None => Behavior::Raw(binding),
            },
            BehaviorRole::LayerTap => match Keycode::from_hid_usage(binding.param2) {
                Some(tap) => Behavior::LayerTap {
                    layer_id: binding.param1,
                    tap,
                },
                None => Behavior::Raw(binding),
            },
            BehaviorRole::ModTap => match (
                Keycode::from_hid_usage(binding.param1),
                Keycode::from_hid_usage(binding.param2),
            ) {
                (Some(hold), Some(tap)) => Behavior::ModTap { hold, tap },
                _ => Behavior::Raw(binding),
            },
            BehaviorRole::StickyKey => match Keycode::from_hid_usage(binding.param1) {
                Some(key) => Behavior::StickyKey(key),
                None => Behavior::Raw(binding),
            },
            BehaviorRole::StickyLayer => Behavior::StickyLayer {
                layer_id: binding.param1,
            },
            BehaviorRole::MomentaryLayer => Behavior::MomentaryLayer {
                layer_id: binding.param1,
            },
            BehaviorRole::ToggleLayer => Behavior::ToggleLayer {
                layer_id: binding.param1,
            },
            BehaviorRole::ToLayer => Behavior::ToLayer {
                layer_id: binding.param1,
            },
            BehaviorRole::Bluetooth => Behavior::Bluetooth {
                command: binding.param1,
                value: binding.param2,
            },
            BehaviorRole::ExternalPower => Behavior::ExternalPower {
                value: binding.param1,
            },
            BehaviorRole::OutputSelection => Behavior::OutputSelection {
                value: binding.param1,
            },
            BehaviorRole::Backlight => Behavior::Backlight {
                command: binding.param1,
                value: binding.param2,
            },
            BehaviorRole::Underglow => Behavior::Underglow {
                command: binding.param1,
                value: binding.param2,
            },
            BehaviorRole::MouseKeyPress => Behavior::MouseKeyPress {
                value: binding.param1,
            },
            BehaviorRole::MouseMove => Behavior::MouseMove {
                value: binding.param1,
            },
            BehaviorRole::MouseScroll => Behavior::MouseScroll {
                value: binding.param1,
            },
            BehaviorRole::CapsWord => Behavior::CapsWord,
            BehaviorRole::KeyRepeat => Behavior::KeyRepeat,
            BehaviorRole::Reset => Behavior::Reset,
            BehaviorRole::Bootloader => Behavior::Bootloader,
            BehaviorRole::SoftOff => Behavior::SoftOff,
            BehaviorRole::StudioUnlock => Behavior::StudioUnlock,
            BehaviorRole::GraveEscape => Behavior::GraveEscape,
            BehaviorRole::Transparent => Behavior::Transparent,
            BehaviorRole::None => Behavior::None,
        };

        Ok(behavior)
    }

    /// Typed keymap API: set a behavior at a specific layer/key position.
    ///
    /// This updates the device's working keymap state only.
    /// Persist with [`StudioClient::save_changes`] or revert with [`StudioClient::discard_changes`].
    pub fn set_key_at(
        &mut self,
        layer_id: u32,
        key_position: i32,
        behavior: Behavior,
    ) -> Result<(), ClientError> {
        self.ensure_behavior_catalog()?;
        let binding = match behavior {
            Behavior::KeyPress(key) => zmk::keymap::BehaviorBinding {
                behavior_id: self.behavior_id_for(BehaviorRole::KeyPress, "Key Press")?,
                param1: key.to_hid_usage(),
                param2: 0,
            },
            Behavior::KeyToggle(key) => zmk::keymap::BehaviorBinding {
                behavior_id: self.behavior_id_for(BehaviorRole::KeyToggle, "Key Toggle")?,
                param1: key.to_hid_usage(),
                param2: 0,
            },
            Behavior::LayerTap {
                layer_id: hold_layer_id,
                tap,
            } => zmk::keymap::BehaviorBinding {
                behavior_id: self.behavior_id_for(BehaviorRole::LayerTap, "Layer-Tap")?,
                param1: hold_layer_id,
                param2: tap.to_hid_usage(),
            },
            Behavior::ModTap { hold, tap } => zmk::keymap::BehaviorBinding {
                behavior_id: self.behavior_id_for(BehaviorRole::ModTap, "Mod-Tap")?,
                param1: hold.to_hid_usage(),
                param2: tap.to_hid_usage(),
            },
            Behavior::StickyKey(key) => zmk::keymap::BehaviorBinding {
                behavior_id: self.behavior_id_for(BehaviorRole::StickyKey, "Sticky Key")?,
                param1: key.to_hid_usage(),
                param2: 0,
            },
            Behavior::StickyLayer {
                layer_id: target_layer_id,
            } => zmk::keymap::BehaviorBinding {
                behavior_id: self.behavior_id_for(BehaviorRole::StickyLayer, "Sticky Layer")?,
                param1: target_layer_id,
                param2: 0,
            },
            Behavior::MomentaryLayer {
                layer_id: hold_layer_id,
            } => zmk::keymap::BehaviorBinding {
                behavior_id: self
                    .behavior_id_for(BehaviorRole::MomentaryLayer, "Momentary Layer")?,
                param1: hold_layer_id,
                param2: 0,
            },
            Behavior::ToggleLayer {
                layer_id: target_layer_id,
            } => zmk::keymap::BehaviorBinding {
                behavior_id: self.behavior_id_for(BehaviorRole::ToggleLayer, "Toggle Layer")?,
                param1: target_layer_id,
                param2: 0,
            },
            Behavior::ToLayer {
                layer_id: target_layer_id,
            } => zmk::keymap::BehaviorBinding {
                behavior_id: self.behavior_id_for(BehaviorRole::ToLayer, "To Layer")?,
                param1: target_layer_id,
                param2: 0,
            },
            Behavior::Bluetooth { command, value } => zmk::keymap::BehaviorBinding {
                behavior_id: self.behavior_id_for(BehaviorRole::Bluetooth, "Bluetooth")?,
                param1: command,
                param2: value,
            },
            Behavior::ExternalPower { value } => zmk::keymap::BehaviorBinding {
                behavior_id: self.behavior_id_for(BehaviorRole::ExternalPower, "External Power")?,
                param1: value,
                param2: 0,
            },
            Behavior::OutputSelection { value } => zmk::keymap::BehaviorBinding {
                behavior_id: self
                    .behavior_id_for(BehaviorRole::OutputSelection, "Output Selection")?,
                param1: value,
                param2: 0,
            },
            Behavior::Backlight { command, value } => zmk::keymap::BehaviorBinding {
                behavior_id: self.behavior_id_for(BehaviorRole::Backlight, "Backlight")?,
                param1: command,
                param2: value,
            },
            Behavior::Underglow { command, value } => zmk::keymap::BehaviorBinding {
                behavior_id: self.behavior_id_for(BehaviorRole::Underglow, "Underglow")?,
                param1: command,
                param2: value,
            },
            Behavior::MouseKeyPress { value } => zmk::keymap::BehaviorBinding {
                behavior_id: self
                    .behavior_id_for(BehaviorRole::MouseKeyPress, "Mouse Key Press")?,
                param1: value,
                param2: 0,
            },
            Behavior::MouseMove { value } => zmk::keymap::BehaviorBinding {
                behavior_id: self.behavior_id_for(BehaviorRole::MouseMove, "Mouse Move")?,
                param1: value,
                param2: 0,
            },
            Behavior::MouseScroll { value } => zmk::keymap::BehaviorBinding {
                behavior_id: self.behavior_id_for(BehaviorRole::MouseScroll, "Mouse Scroll")?,
                param1: value,
                param2: 0,
            },
            Behavior::CapsWord => zmk::keymap::BehaviorBinding {
                behavior_id: self.behavior_id_for(BehaviorRole::CapsWord, "Caps Word")?,
                param1: 0,
                param2: 0,
            },
            Behavior::KeyRepeat => zmk::keymap::BehaviorBinding {
                behavior_id: self.behavior_id_for(BehaviorRole::KeyRepeat, "Key Repeat")?,
                param1: 0,
                param2: 0,
            },
            Behavior::Reset => zmk::keymap::BehaviorBinding {
                behavior_id: self.behavior_id_for(BehaviorRole::Reset, "Reset")?,
                param1: 0,
                param2: 0,
            },
            Behavior::Bootloader => zmk::keymap::BehaviorBinding {
                behavior_id: self.behavior_id_for(BehaviorRole::Bootloader, "Bootloader")?,
                param1: 0,
                param2: 0,
            },
            Behavior::SoftOff => zmk::keymap::BehaviorBinding {
                behavior_id: self.behavior_id_for(BehaviorRole::SoftOff, "Soft Off")?,
                param1: 0,
                param2: 0,
            },
            Behavior::StudioUnlock => zmk::keymap::BehaviorBinding {
                behavior_id: self.behavior_id_for(BehaviorRole::StudioUnlock, "Studio Unlock")?,
                param1: 0,
                param2: 0,
            },
            Behavior::GraveEscape => zmk::keymap::BehaviorBinding {
                behavior_id: self.behavior_id_for(BehaviorRole::GraveEscape, "Grave/Escape")?,
                param1: 0,
                param2: 0,
            },
            Behavior::Transparent => zmk::keymap::BehaviorBinding {
                behavior_id: self.behavior_id_for(BehaviorRole::Transparent, "Transparent")?,
                param1: 0,
                param2: 0,
            },
            Behavior::None => zmk::keymap::BehaviorBinding {
                behavior_id: self.behavior_id_for(BehaviorRole::None, "None")?,
                param1: 0,
                param2: 0,
            },
            Behavior::Raw(raw) => raw,
        };

        self.set_layer_binding(layer_id, key_position, binding)
    }

    pub fn check_unsaved_changes(&mut self) -> Result<bool, ClientError> {
        let response =
            self.call_keymap(zmk::keymap::request::RequestType::CheckUnsavedChanges(true))?;
        match response.response_type {
            Some(zmk::keymap::response::ResponseType::CheckUnsavedChanges(has_changes)) => {
                Ok(has_changes)
            }
            _ => Err(ClientError::MissingResponseType),
        }
    }

    /// Saves pending keymap/layout mutations made by methods like [`StudioClient::set_key_at`].
    ///
    /// After this succeeds, changes are persisted on the device.
    pub fn save_changes(&mut self) -> Result<(), ClientError> {
        let response = self.call_keymap(zmk::keymap::request::RequestType::SaveChanges(true))?;
        match response.response_type {
            Some(zmk::keymap::response::ResponseType::SaveChanges(save)) => match save.result {
                Some(zmk::keymap::save_changes_response::Result::Ok(_)) => Ok(()),
                Some(zmk::keymap::save_changes_response::Result::Err(raw)) => {
                    let err = zmk::keymap::SaveChangesErrorCode::try_from(raw).map_err(|_| {
                        ClientError::UnknownEnumValue {
                            field: "keymap.save_changes",
                            value: raw,
                        }
                    })?;
                    Err(ClientError::SaveChangesFailed(err))
                }
                None => Err(ClientError::MissingResponseType),
            },
            _ => Err(ClientError::MissingResponseType),
        }
    }

    /// Discards pending keymap/layout mutations made since the last save.
    ///
    /// Returns `true` if there were pending changes and they were discarded.
    pub fn discard_changes(&mut self) -> Result<bool, ClientError> {
        let response = self.call_keymap(zmk::keymap::request::RequestType::DiscardChanges(true))?;
        match response.response_type {
            Some(zmk::keymap::response::ResponseType::DiscardChanges(discarded)) => Ok(discarded),
            _ => Err(ClientError::MissingResponseType),
        }
    }

    pub fn set_active_physical_layout(
        &mut self,
        index: u32,
    ) -> Result<zmk::keymap::Keymap, ClientError> {
        let response = self.call_keymap(
            zmk::keymap::request::RequestType::SetActivePhysicalLayout(index),
        )?;
        match response.response_type {
            Some(zmk::keymap::response::ResponseType::SetActivePhysicalLayout(resp)) => {
                match resp.result {
                    Some(zmk::keymap::set_active_physical_layout_response::Result::Ok(keymap)) => {
                        Ok(keymap)
                    }
                    Some(zmk::keymap::set_active_physical_layout_response::Result::Err(raw)) => {
                        let err = zmk::keymap::SetActivePhysicalLayoutErrorCode::try_from(raw)
                            .map_err(|_| ClientError::UnknownEnumValue {
                                field: "keymap.set_active_physical_layout",
                                value: raw,
                            })?;
                        Err(ClientError::SetActivePhysicalLayoutFailed(err))
                    }
                    None => Err(ClientError::MissingResponseType),
                }
            }
            _ => Err(ClientError::MissingResponseType),
        }
    }

    pub fn move_layer(
        &mut self,
        start_index: u32,
        dest_index: u32,
    ) -> Result<zmk::keymap::Keymap, ClientError> {
        let request = zmk::keymap::MoveLayerRequest {
            start_index,
            dest_index,
        };
        let response = self.call_keymap(zmk::keymap::request::RequestType::MoveLayer(request))?;
        match response.response_type {
            Some(zmk::keymap::response::ResponseType::MoveLayer(resp)) => match resp.result {
                Some(zmk::keymap::move_layer_response::Result::Ok(keymap)) => Ok(keymap),
                Some(zmk::keymap::move_layer_response::Result::Err(raw)) => {
                    let err = zmk::keymap::MoveLayerErrorCode::try_from(raw).map_err(|_| {
                        ClientError::UnknownEnumValue {
                            field: "keymap.move_layer",
                            value: raw,
                        }
                    })?;
                    Err(ClientError::MoveLayerFailed(err))
                }
                None => Err(ClientError::MissingResponseType),
            },
            _ => Err(ClientError::MissingResponseType),
        }
    }

    pub fn add_layer(&mut self) -> Result<zmk::keymap::AddLayerResponseDetails, ClientError> {
        let response = self.call_keymap(zmk::keymap::request::RequestType::AddLayer(
            zmk::keymap::AddLayerRequest {},
        ))?;
        match response.response_type {
            Some(zmk::keymap::response::ResponseType::AddLayer(resp)) => match resp.result {
                Some(zmk::keymap::add_layer_response::Result::Ok(details)) => Ok(details),
                Some(zmk::keymap::add_layer_response::Result::Err(raw)) => {
                    let err = zmk::keymap::AddLayerErrorCode::try_from(raw).map_err(|_| {
                        ClientError::UnknownEnumValue {
                            field: "keymap.add_layer",
                            value: raw,
                        }
                    })?;
                    Err(ClientError::AddLayerFailed(err))
                }
                None => Err(ClientError::MissingResponseType),
            },
            _ => Err(ClientError::MissingResponseType),
        }
    }

    pub fn remove_layer(&mut self, layer_index: u32) -> Result<(), ClientError> {
        let request = zmk::keymap::RemoveLayerRequest { layer_index };
        let response = self.call_keymap(zmk::keymap::request::RequestType::RemoveLayer(request))?;
        match response.response_type {
            Some(zmk::keymap::response::ResponseType::RemoveLayer(resp)) => match resp.result {
                Some(zmk::keymap::remove_layer_response::Result::Ok(_)) => Ok(()),
                Some(zmk::keymap::remove_layer_response::Result::Err(raw)) => {
                    let err = zmk::keymap::RemoveLayerErrorCode::try_from(raw).map_err(|_| {
                        ClientError::UnknownEnumValue {
                            field: "keymap.remove_layer",
                            value: raw,
                        }
                    })?;
                    Err(ClientError::RemoveLayerFailed(err))
                }
                None => Err(ClientError::MissingResponseType),
            },
            _ => Err(ClientError::MissingResponseType),
        }
    }

    pub fn restore_layer(
        &mut self,
        layer_id: u32,
        at_index: u32,
    ) -> Result<zmk::keymap::Layer, ClientError> {
        let request = zmk::keymap::RestoreLayerRequest { layer_id, at_index };
        let response =
            self.call_keymap(zmk::keymap::request::RequestType::RestoreLayer(request))?;
        match response.response_type {
            Some(zmk::keymap::response::ResponseType::RestoreLayer(resp)) => match resp.result {
                Some(zmk::keymap::restore_layer_response::Result::Ok(layer)) => Ok(layer),
                Some(zmk::keymap::restore_layer_response::Result::Err(raw)) => {
                    let err = zmk::keymap::RestoreLayerErrorCode::try_from(raw).map_err(|_| {
                        ClientError::UnknownEnumValue {
                            field: "keymap.restore_layer",
                            value: raw,
                        }
                    })?;
                    Err(ClientError::RestoreLayerFailed(err))
                }
                None => Err(ClientError::MissingResponseType),
            },
            _ => Err(ClientError::MissingResponseType),
        }
    }

    pub fn set_layer_props(
        &mut self,
        layer_id: u32,
        name: impl Into<String>,
    ) -> Result<(), ClientError> {
        let request = zmk::keymap::SetLayerPropsRequest {
            layer_id,
            name: name.into(),
        };
        let response =
            self.call_keymap(zmk::keymap::request::RequestType::SetLayerProps(request))?;
        match response.response_type {
            Some(zmk::keymap::response::ResponseType::SetLayerProps(raw)) => {
                let code = zmk::keymap::SetLayerPropsResponse::try_from(raw).map_err(|_| {
                    ClientError::UnknownEnumValue {
                        field: "keymap.set_layer_props",
                        value: raw,
                    }
                })?;

                if code == zmk::keymap::SetLayerPropsResponse::SetLayerPropsRespOk {
                    Ok(())
                } else {
                    Err(ClientError::SetLayerPropsFailed(code))
                }
            }
            _ => Err(ClientError::MissingResponseType),
        }
    }

    fn behavior_id_for(
        &self,
        role: BehaviorRole,
        display_name: &'static str,
    ) -> Result<i32, ClientError> {
        let behavior_id = self
            .behavior_id_by_role
            .get(&role)
            .copied()
            .ok_or(ClientError::MissingBehaviorRole(display_name))?;
        i32::try_from(behavior_id).map_err(|_| ClientError::BehaviorIdOutOfRange { behavior_id })
    }

    fn ensure_behavior_catalog(&mut self) -> Result<(), ClientError> {
        if !self.behavior_role_by_id.is_empty() {
            return Ok(());
        }

        let ids = self.list_all_behaviors()?;
        for id in ids {
            let details = self.get_behavior_details(id)?;
            let role = role_from_display_name(&details.display_name);
            if let Some(role) = role {
                self.behavior_role_by_id.insert(id, role);
                self.behavior_id_by_role.entry(role).or_insert(id);
            }
        }

        Ok(())
    }

    fn call_core(
        &mut self,
        request_type: zmk::core::request::RequestType,
    ) -> Result<zmk::core::Response, ClientError> {
        let request = zmk::core::Request {
            request_type: Some(request_type),
        };
        let rr = self.call(studio::request::Subsystem::Core(request))?;

        match rr.subsystem {
            Some(studio::request_response::Subsystem::Core(resp)) => Ok(resp),
            Some(_) => Err(ClientError::UnexpectedSubsystem("core")),
            None => Err(ClientError::MissingSubsystem),
        }
    }

    fn call_behaviors(
        &mut self,
        request_type: zmk::behaviors::request::RequestType,
    ) -> Result<zmk::behaviors::Response, ClientError> {
        let request = zmk::behaviors::Request {
            request_type: Some(request_type),
        };
        let rr = self.call(studio::request::Subsystem::Behaviors(request))?;

        match rr.subsystem {
            Some(studio::request_response::Subsystem::Behaviors(resp)) => Ok(resp),
            Some(_) => Err(ClientError::UnexpectedSubsystem("behaviors")),
            None => Err(ClientError::MissingSubsystem),
        }
    }

    fn call_keymap(
        &mut self,
        request_type: zmk::keymap::request::RequestType,
    ) -> Result<zmk::keymap::Response, ClientError> {
        let request = zmk::keymap::Request {
            request_type: Some(request_type),
        };
        let rr = self.call(studio::request::Subsystem::Keymap(request))?;

        match rr.subsystem {
            Some(studio::request_response::Subsystem::Keymap(resp)) => Ok(resp),
            Some(_) => Err(ClientError::UnexpectedSubsystem("keymap")),
            None => Err(ClientError::MissingSubsystem),
        }
    }

    fn call(
        &mut self,
        subsystem: studio::request::Subsystem,
    ) -> Result<studio::RequestResponse, ClientError> {
        let request_id = self.next_request_id;
        self.next_request_id = self.next_request_id.wrapping_add(1);

        let request = studio::Request {
            request_id,
            subsystem: Some(subsystem),
        };
        let bytes = encode_request(&request);
        self.io.write_all(&bytes)?;

        loop {
            let response = self.read_next_response()?;
            match response.r#type {
                Some(studio::response::Type::Notification(notification)) => {
                    self.notifications.push_back(notification);
                }
                Some(studio::response::Type::RequestResponse(rr)) => {
                    if rr.request_id != request_id {
                        return Err(ClientError::UnexpectedRequestId {
                            expected: request_id,
                            actual: rr.request_id,
                        });
                    }

                    if let Some(studio::request_response::Subsystem::Meta(meta)) = &rr.subsystem {
                        match meta.response_type {
                            Some(zmk::meta::response::ResponseType::NoResponse(true)) => {
                                return Err(ClientError::NoResponse);
                            }
                            Some(zmk::meta::response::ResponseType::SimpleError(raw)) => {
                                let cond =
                                    zmk::meta::ErrorConditions::try_from(raw).map_err(|_| {
                                        ClientError::UnknownEnumValue {
                                            field: "meta.simple_error",
                                            value: raw,
                                        }
                                    })?;
                                return Err(ClientError::Meta(cond));
                            }
                            _ => return Err(ClientError::MissingResponseType),
                        }
                    }

                    return Ok(rr);
                }
                None => return Err(ClientError::MissingResponseType),
            }
        }
    }

    fn read_next_response(&mut self) -> Result<studio::Response, ClientError> {
        if let Some(response) = self.responses.pop_front() {
            return Ok(response);
        }

        loop {
            let read = self.io.read(&mut self.read_buffer)?;
            if read == 0 {
                return Err(ClientError::Io(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    "Transport reached EOF",
                )));
            }

            let decoded = decode_responses(&mut self.decoder, &self.read_buffer[..read])?;
            self.responses.extend(decoded);

            if let Some(response) = self.responses.pop_front() {
                return Ok(response);
            }
        }
    }
}

fn binding_at(
    keymap: &zmk::keymap::Keymap,
    layer_id: u32,
    key_position: i32,
) -> Option<zmk::keymap::BehaviorBinding> {
    let pos = usize::try_from(key_position).ok()?;
    let layer = keymap.layers.iter().find(|l| l.id == layer_id)?;
    layer.bindings.get(pos).copied()
}

#[cfg(feature = "serial")]
impl StudioClient<SerialTransport> {
    pub fn open_serial(path: &str) -> Result<Self, SerialTransportError> {
        Ok(Self::new(SerialTransport::open(path)?))
    }
}

#[cfg(feature = "ble")]
impl StudioClient<BleTransport> {
    pub fn connect_ble() -> Result<Self, BleTransportError> {
        Ok(Self::new(BleTransport::connect_first()?))
    }

    pub fn connect_ble_with_options(options: BleConnectOptions) -> Result<Self, BleTransportError> {
        Ok(Self::new(BleTransport::connect_with_options(options)?))
    }
}
