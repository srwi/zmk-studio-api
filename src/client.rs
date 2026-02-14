//! High-level typed API for interacting with ZMK Studio devices.

use std::collections::VecDeque;
use std::io::{Read, Write};

use crate::framing::FrameDecoder;
use crate::proto::zmk;
use crate::proto::zmk::studio;
use crate::protocol::{ProtocolError, decode_responses, encode_request};

/// BLE service UUID used by the firmware and TypeScript client.
pub const BLE_SERVICE_UUID: &str = "00000000-0196-6107-c967-c5cfb1c2482a";
/// BLE RPC characteristic UUID used by the firmware and TypeScript client.
pub const BLE_RPC_CHARACTERISTIC_UUID: &str = "00000001-0196-6107-c967-c5cfb1c2482a";

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
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(err) => write!(f, "I/O error: {err}"),
            Self::Protocol(err) => write!(f, "protocol error: {err}"),
            Self::Meta(cond) => write!(f, "device returned meta error: {}", cond.as_str_name()),
            Self::NoResponse => write!(f, "device returned no response"),
            Self::MissingResponseType => write!(f, "response was missing type"),
            Self::MissingSubsystem => write!(f, "request_response was missing subsystem"),
            Self::UnexpectedSubsystem(expected) => {
                write!(f, "unexpected subsystem in response; expected {expected}")
            }
            Self::UnexpectedRequestId { expected, actual } => {
                write!(
                    f,
                    "unexpected request id in response: expected {expected}, got {actual}"
                )
            }
            Self::UnknownEnumValue { field, value } => {
                write!(f, "unknown enum value for {field}: {value}")
            }
            Self::SetLayerBindingFailed(code) => {
                write!(f, "set layer binding failed: {}", code.as_str_name())
            }
            Self::SaveChangesFailed(code) => {
                write!(f, "save changes failed: {}", code.as_str_name())
            }
            Self::SetActivePhysicalLayoutFailed(code) => {
                write!(
                    f,
                    "set active physical layout failed: {}",
                    code.as_str_name()
                )
            }
            Self::MoveLayerFailed(code) => write!(f, "move layer failed: {}", code.as_str_name()),
            Self::AddLayerFailed(code) => write!(f, "add layer failed: {}", code.as_str_name()),
            Self::RemoveLayerFailed(code) => {
                write!(f, "remove layer failed: {}", code.as_str_name())
            }
            Self::RestoreLayerFailed(code) => {
                write!(f, "restore layer failed: {}", code.as_str_name())
            }
            Self::SetLayerPropsFailed(code) => {
                write!(f, "set layer properties failed: {}", code.as_str_name())
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
                    "transport reached EOF",
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
