use std::collections::{HashMap, HashSet, VecDeque};
use std::io::{Read, Write};
use std::time::{Duration, Instant};

use crate::binding::{Behavior, BehaviorRole, ResolvedLayer, role_from_display_name, typed_params};
use crate::framing::FrameDecoder;
use crate::hid_usage::HidUsage;
use crate::proto::zmk;
use crate::proto::zmk::studio;
use crate::protocol::{decode_responses, encode_request};
#[cfg(feature = "serial")]
use crate::transport::serial::{SerialTransport, SerialTransportError};
#[cfg(feature = "ble")]
use crate::transport::{
    BleDeviceInfo, BleDiscoveryMode, PlatformBleError, PlatformBleTransport,
    discover_platform_ble_devices,
};

/// High-level error type returned by [`StudioClient`] operations.
#[derive(Debug)]
pub enum ClientError {
    Io(std::io::Error),
    Timeout { elapsed: Duration },
    Meta(zmk::meta::ErrorConditions),
    NoResponse,
    MissingResponseType,
    MissingSubsystem,
    UnexpectedSubsystem(&'static str),
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
            Self::Timeout { elapsed } => {
                write!(f, "No response from device within {elapsed:?}")
            }
            Self::Meta(cond) => write!(f, "Device returned meta error: {}", cond.as_str_name()),
            Self::NoResponse => write!(f, "Device returned no response"),
            Self::MissingResponseType => write!(f, "Response was missing type"),
            Self::MissingSubsystem => write!(f, "Request response was missing subsystem"),
            Self::UnexpectedSubsystem(expected) => {
                write!(f, "Unexpected subsystem in response; expected {expected}")
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
            _ => None,
        }
    }
}

impl From<std::io::Error> for ClientError {
    fn from(value: std::io::Error) -> Self {
        Self::Io(value)
    }
}

/// Default per-call budget. Individual transport reads time out much faster;
/// the client keeps reading (and re-sends the request) until this elapses.
pub const DEFAULT_CALL_TIMEOUT: Duration = Duration::from_secs(3);

/// Number of times a request is (re-)sent within one call budget before the
/// call fails with [`ClientError::Timeout`]. Requests carry an echoed ID, so a
/// late response to an earlier send still matches.
const CALL_SEND_ATTEMPTS: u32 = 3;

/// Seed the request-ID counter from the clock so IDs from a previous session
/// (whose responses may still sit in the device's transmit buffer) will not
/// collide with ours.
fn seed_request_id() -> u32 {
    use std::time::{SystemTime, UNIX_EPOCH};
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    ((elapsed.as_secs() as u32) << 20) ^ elapsed.subsec_nanos()
}

/// High-level synchronous ZMK Studio RPC client.
///
/// The generic parameter `T` is any transport implementing [`Read`] + [`Write`]
/// (for example [`crate::transport::serial::SerialTransport`]).
pub struct StudioClient<T> {
    io: T,
    next_request_id: u32,
    call_timeout: Duration,
    decoder: FrameDecoder,
    read_buffer: Vec<u8>,
    responses: VecDeque<studio::Response>,
    notifications: VecDeque<studio::Notification>,
    behavior_role_by_id: HashMap<u32, BehaviorRole>,
    behavior_id_by_role: HashMap<BehaviorRole, u32>,
    behavior_details_fetched: HashSet<u32>,
    /// Details of behaviors with no built-in role, kept so their bindings can be
    /// resolved into [`Behavior::Custom`] without another round trip.
    custom_behavior_details: HashMap<u32, zmk::behaviors::GetBehaviorDetailsResponse>,
    behavior_catalog_complete: bool,
}

impl<T: Read + Write> StudioClient<T> {
    pub fn new(io: T) -> Self {
        Self::with_read_buffer(io, 256)
    }

    fn with_read_buffer(io: T, read_buffer_size: usize) -> Self {
        Self {
            io,
            next_request_id: seed_request_id(),
            call_timeout: DEFAULT_CALL_TIMEOUT,
            decoder: FrameDecoder::new(),
            read_buffer: vec![0; read_buffer_size.max(1)],
            responses: VecDeque::new(),
            notifications: VecDeque::new(),
            behavior_role_by_id: HashMap::new(),
            behavior_id_by_role: HashMap::new(),
            behavior_details_fetched: HashSet::new(),
            custom_behavior_details: HashMap::new(),
            behavior_catalog_complete: false,
        }
    }

    /// Sets the total time budget for a single RPC call, including re-sends.
    pub fn set_call_timeout(&mut self, timeout: Duration) {
        self.call_timeout = timeout.max(Duration::from_millis(1));
    }

    /// Reads and discards any bytes the device (or the OS) buffered before
    /// this session, until the line has been quiet for one transport read
    /// timeout or `max_total` elapsed.
    ///
    /// ZMK's UART transport keeps unsent responses and notifications in a ring
    /// buffer across host connections, so a fresh serial session frequently
    /// starts with stale — possibly truncated — frames from the previous one.
    pub fn drain_stale_input(&mut self, max_total: Duration) {
        let deadline = Instant::now() + max_total;
        while Instant::now() < deadline {
            match self.io.read(&mut self.read_buffer) {
                Ok(0) => break,
                Ok(_) => continue,
                Err(err) if is_read_timeout(&err) => break,
                Err(_) => break,
            }
        }
        self.decoder.reset();
        self.responses.clear();
    }

    /// Returns the next queued notification, if any.
    pub fn next_notification(&mut self) -> Option<studio::Notification> {
        self.notifications.pop_front()
    }

    /// Blocks until a notification arrives and returns it.
    ///
    /// Waits indefinitely; only a transport failure ends the wait early.
    pub fn read_notification_blocking(&mut self) -> Result<studio::Notification, ClientError> {
        loop {
            if let Some(notification) = self.next_notification() {
                return Ok(notification);
            }

            // Request responses read here have no in-flight request and are
            // therefore stale; only notifications are kept.
            if let Some(studio::Response {
                r#type: Some(studio::response::Type::Notification(notification)),
            }) = self.try_read_response()?
            {
                self.notifications.push_back(notification);
            }
        }
    }

    /// Returns static device information.
    pub fn get_device_info(&mut self) -> Result<zmk::core::GetDeviceInfoResponse, ClientError> {
        let response = self.call_core(zmk::core::request::RequestType::GetDeviceInfo(true))?;
        match response.response_type {
            Some(zmk::core::response::ResponseType::GetDeviceInfo(info)) => Ok(info),
            _ => Err(ClientError::MissingResponseType),
        }
    }

    /// Returns the current Studio lock state.
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

    /// Resets settings on the device.
    ///
    /// Returns the firmware-provided success boolean.
    pub fn reset_settings(&mut self) -> Result<bool, ClientError> {
        let response = self.call_core(zmk::core::request::RequestType::ResetSettings(true))?;
        match response.response_type {
            Some(zmk::core::response::ResponseType::ResetSettings(ok)) => Ok(ok),
            _ => Err(ClientError::MissingResponseType),
        }
    }

    /// Lists behavior IDs available on the connected device.
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

    /// Returns details for a behavior ID (name and parameter metadata).
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

    /// Returns the current keymap state from the device.
    pub fn get_keymap(&mut self) -> Result<zmk::keymap::Keymap, ClientError> {
        let response = self.call_keymap(zmk::keymap::request::RequestType::GetKeymap(true))?;
        match response.response_type {
            Some(zmk::keymap::response::ResponseType::GetKeymap(keymap)) => Ok(keymap),
            _ => Err(ClientError::MissingResponseType),
        }
    }

    /// Returns available physical layouts and the active layout index.
    pub fn get_physical_layouts(&mut self) -> Result<zmk::keymap::PhysicalLayouts, ClientError> {
        let response =
            self.call_keymap(zmk::keymap::request::RequestType::GetPhysicalLayouts(true))?;
        match response.response_type {
            Some(zmk::keymap::response::ResponseType::GetPhysicalLayouts(layouts)) => Ok(layouts),
            _ => Err(ClientError::MissingResponseType),
        }
    }

    /// Sets a raw behavior binding for a specific layer position.
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

    /// Reads a behavior from a specific layer/key position.
    pub fn get_key_at(
        &mut self,
        layer_id: u32,
        key_position: i32,
    ) -> Result<Behavior, ClientError> {
        let keymap = self.get_keymap()?;
        let binding = binding_at(&keymap, layer_id, key_position).ok_or(
            ClientError::InvalidLayerOrPosition {
                layer_id,
                key_position,
            },
        )?;
        self.ensure_roles_for_bindings(std::iter::once(&binding))?;

        Ok(self.resolve_binding(&binding))
    }

    /// Fetches the keymap and resolves every binding into a typed [`Behavior`].
    ///
    /// Returns the layers in keymap order as [`ResolvedLayer`]s, each carrying the
    /// layer `id`, `name`, and its resolved bindings. It fetches the keymap once and
    /// converts all bindings in a single pass. Behavior details are only fetched
    /// for behaviors the keymap actually uses, keeping the number of RPC round
    /// trips small on slow links (BLE in particular).
    pub fn resolve_keymap(&mut self) -> Result<Vec<ResolvedLayer>, ClientError> {
        let keymap = self.get_keymap()?;
        self.ensure_roles_for_bindings(
            keymap.layers.iter().flat_map(|layer| layer.bindings.iter()),
        )?;

        let layers = keymap
            .layers
            .iter()
            .map(|layer| ResolvedLayer {
                id: layer.id,
                name: layer.name.clone(),
                bindings: layer
                    .bindings
                    .iter()
                    .map(|binding| self.resolve_binding(binding))
                    .collect(),
            })
            .collect();

        Ok(layers)
    }

    /// Resolves a binding whose behavior is not one of ZMK's built-ins into
    /// [`Behavior::Custom`], using the name and parameter metadata the device
    /// reported for it.
    fn resolve_custom_binding(
        &self,
        behavior_id: u32,
        binding: &zmk::keymap::BehaviorBinding,
    ) -> Behavior {
        let Some(details) = self.custom_behavior_details.get(&behavior_id) else {
            return Behavior::Unknown {
                behavior_id: binding.behavior_id,
                param1: binding.param1,
                param2: binding.param2,
            };
        };

        let (param1, param2) = typed_params(&details.metadata, binding.param1, binding.param2);
        Behavior::Custom {
            behavior_id,
            display_name: details.display_name.clone(),
            param1,
            param2,
        }
    }

    fn resolve_binding(&self, binding: &zmk::keymap::BehaviorBinding) -> Behavior {
        let Ok(binding_behavior_id) = u32::try_from(binding.behavior_id) else {
            return Behavior::Unknown {
                behavior_id: binding.behavior_id,
                param1: binding.param1,
                param2: binding.param2,
            };
        };
        let Some(role) = self.behavior_role_by_id.get(&binding_behavior_id).copied() else {
            return self.resolve_custom_binding(binding_behavior_id, binding);
        };

        match role {
            BehaviorRole::KeyPress => Behavior::KeyPress(HidUsage::from_encoded(binding.param1)),
            BehaviorRole::KeyToggle => Behavior::KeyToggle(HidUsage::from_encoded(binding.param1)),
            BehaviorRole::LayerTap => Behavior::LayerTap {
                layer_id: binding.param1,
                tap: HidUsage::from_encoded(binding.param2),
            },
            BehaviorRole::ModTap => Behavior::ModTap {
                hold: HidUsage::from_encoded(binding.param1),
                tap: HidUsage::from_encoded(binding.param2),
            },
            BehaviorRole::StickyKey => Behavior::StickyKey(HidUsage::from_encoded(binding.param1)),
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
        }
    }

    /// Set a behavior at a specific layer/key position.
    ///
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
            Behavior::Custom {
                behavior_id,
                param1,
                param2,
                ..
            } => zmk::keymap::BehaviorBinding {
                behavior_id: i32::try_from(behavior_id)
                    .map_err(|_| ClientError::BehaviorIdOutOfRange { behavior_id })?,
                param1: param1.to_raw(),
                param2: param2.to_raw(),
            },
            Behavior::Unknown {
                behavior_id,
                param1,
                param2,
            } => zmk::keymap::BehaviorBinding {
                behavior_id,
                param1,
                param2,
            },
        };

        self.set_layer_binding(layer_id, key_position, binding)
    }

    /// Returns whether there are pending unsaved keymap/layout changes.
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

    /// Sets the active physical layout by index and returns the resulting keymap.
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

    /// Moves a layer from `start_index` to `dest_index` and returns the updated keymap.
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

    /// Adds a layer and returns firmware-provided details about the created layer.
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

    /// Removes a layer by index.
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

    /// Restores a previously removed layer at a specific index.
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

    /// Sets user-facing properties for a layer (currently just `name`).
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

    /// Fetches the full behavior catalog (one `ListAllBehaviors` call plus one
    /// `GetBehaviorDetails` call per behavior) and caches the role mapping, so
    /// the first [`StudioClient::set_key_at`] is a single round trip instead
    /// of a catalog fetch. Safe to call more than once; later calls are
    /// no-ops.
    pub fn ensure_behavior_catalog(&mut self) -> Result<(), ClientError> {
        if self.behavior_catalog_complete {
            return Ok(());
        }

        let ids = self.list_all_behaviors()?;
        for id in ids {
            self.fetch_behavior_role(id)?;
        }
        self.behavior_catalog_complete = true;

        Ok(())
    }

    /// Returns the set of behavior roles supported by the connected device.
    ///
    /// Fetches and caches the full behavior catalog if not already loaded.
    pub fn supported_roles(&mut self) -> Result<HashSet<BehaviorRole>, ClientError> {
        self.ensure_behavior_catalog()?;
        Ok(self.behavior_id_by_role.keys().copied().collect())
    }

    /// Returns whether the connected device supports the given behavior role.
    pub fn supports_role(&mut self, role: BehaviorRole) -> Result<bool, ClientError> {
        self.ensure_behavior_catalog()?;
        Ok(self.behavior_id_by_role.contains_key(&role))
    }

    /// Returns whether the connected device supports the given behavior.
    ///
    /// Custom and unknown behaviors return `true`. Standard behaviors check
    /// against the device's behavior catalog.
    pub fn supports_behavior(&mut self, behavior: &Behavior) -> Result<bool, ClientError> {
        match behavior.role() {
            Some(role) => self.supports_role(role),
            None => Ok(true),
        }
    }

    /// Fetches behavior details only for behaviors referenced by `bindings`
    /// that are not cached yet.
    fn ensure_roles_for_bindings<'a>(
        &mut self,
        bindings: impl Iterator<Item = &'a zmk::keymap::BehaviorBinding>,
    ) -> Result<(), ClientError> {
        let missing: HashSet<u32> = bindings
            .filter_map(|binding| u32::try_from(binding.behavior_id).ok())
            .filter(|id| *id != 0 && !self.behavior_details_fetched.contains(id))
            .collect();

        for id in missing {
            self.fetch_behavior_role(id)?;
        }

        Ok(())
    }

    fn fetch_behavior_role(&mut self, id: u32) -> Result<(), ClientError> {
        if !self.behavior_details_fetched.insert(id) {
            return Ok(());
        }

        let details = match self.get_behavior_details(id) {
            Ok(details) => details,
            Err(ClientError::Meta(zmk::meta::ErrorConditions::Generic)) => return Ok(()),
            Err(err) => return Err(err),
        };
        match role_from_display_name(&details.display_name) {
            Some(role) => {
                self.behavior_role_by_id.insert(id, role);
                self.behavior_id_by_role.entry(role).or_insert(id);
            }
            // A behavior from the user's keymap: keep its name and parameter
            // metadata so its bindings resolve into `Behavior::Custom`.
            None => {
                self.custom_behavior_details.insert(id, details);
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

        let started = Instant::now();
        let deadline = started + self.call_timeout;
        let resend_interval = self.call_timeout / CALL_SEND_ATTEMPTS;
        let mut sends_left = CALL_SEND_ATTEMPTS;
        let mut next_send = started;

        loop {
            if Instant::now() >= deadline {
                return Err(ClientError::Timeout {
                    elapsed: started.elapsed(),
                });
            }

            // (Re-)send on a fraction of the budget: the device answers every
            // request it receives, so silence means the request (or response)
            // was lost — e.g. dropped in a device-side buffer overflow.
            if sends_left > 0 && Instant::now() >= next_send {
                self.io.write_all(&bytes)?;
                self.io.flush()?;
                sends_left -= 1;
                next_send += resend_interval;
            }

            let Some(response) = self.try_read_response()? else {
                continue;
            };

            match response.r#type {
                Some(studio::response::Type::Notification(notification)) => {
                    self.notifications.push_back(notification);
                }
                Some(studio::response::Type::RequestResponse(rr)) => {
                    if rr.request_id != request_id {
                        // Response to a request from an earlier session, still
                        // queued in the device's transmit buffer. Skip it.
                        continue;
                    }

                    let Some(response_subsystem) = &rr.subsystem else {
                        return Err(ClientError::MissingSubsystem);
                    };
                    let request_subsystem =
                        request.subsystem.as_ref().expect("subsystem set above");
                    if !subsystem_matches(request_subsystem, response_subsystem) {
                        // Same ID but a different subsystem: a stale response
                        // whose ID happens to collide with ours. Skip it.
                        continue;
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
                // A frame that decoded to an empty response; nothing to do.
                None => continue,
            }
        }
    }

    /// Attempts to produce one decoded response, reading from the transport at
    /// most once. Returns `Ok(None)` when no data arrived within the
    /// transport's own read timeout.
    fn try_read_response(&mut self) -> Result<Option<studio::Response>, ClientError> {
        if let Some(response) = self.responses.pop_front() {
            return Ok(Some(response));
        }

        match self.io.read(&mut self.read_buffer) {
            // Transports may legitimately deliver zero bytes (e.g. an empty
            // BLE packet); it does not signal end-of-stream.
            Ok(0) => Ok(None),
            Ok(read) => {
                let decoded = decode_responses(&mut self.decoder, &self.read_buffer[..read]);
                self.responses.extend(decoded);
                Ok(self.responses.pop_front())
            }
            Err(err) if is_read_timeout(&err) => Ok(None),
            Err(err) => Err(ClientError::Io(err)),
        }
    }
}

fn is_read_timeout(err: &std::io::Error) -> bool {
    matches!(
        err.kind(),
        std::io::ErrorKind::TimedOut
            | std::io::ErrorKind::WouldBlock
            | std::io::ErrorKind::Interrupted
    )
}

fn subsystem_matches(
    request: &studio::request::Subsystem,
    response: &studio::request_response::Subsystem,
) -> bool {
    use studio::request::Subsystem as Req;
    use studio::request_response::Subsystem as Resp;
    matches!(
        (request, response),
        (Req::Core(_), Resp::Core(_))
            | (Req::Keymap(_), Resp::Keymap(_))
            | (Req::Behaviors(_), Resp::Behaviors(_))
            | (_, Resp::Meta(_))
    )
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
    /// Convenience constructor for opening a serial transport and wrapping it in a client.
    ///
    /// Discards any stale bytes buffered by the device or the OS before the
    /// first request is sent; see [`StudioClient::drain_stale_input`].
    pub fn open_serial(path: &str) -> Result<Self, SerialTransportError> {
        let mut client = Self::new(SerialTransport::open(path)?);
        client.drain_stale_input(Duration::from_secs(1));
        Ok(client)
    }
}

#[cfg(feature = "ble")]
impl StudioClient<PlatformBleTransport> {
    /// Lists BLE devices using the backend strategy chosen for the current OS.
    pub fn list_ble_devices() -> Result<Vec<BleDeviceInfo>, PlatformBleError> {
        Self::list_ble_devices_with_mode(BleDiscoveryMode::Any)
    }

    /// Lists BLE devices using an explicit discovery mode.
    pub fn list_ble_devices_with_mode(
        mode: BleDiscoveryMode,
    ) -> Result<Vec<BleDeviceInfo>, PlatformBleError> {
        discover_platform_ble_devices(mode)
    }

    /// Open a BLE connection to a device previously returned by discovery.
    pub fn open_ble(device_id: &str) -> Result<Self, PlatformBleError> {
        let mut client = Self::new(PlatformBleTransport::connect_device(device_id)?);
        // BLE round trips are paced by the connection interval; allow a
        // larger budget per call than over USB serial.
        client.set_call_timeout(Duration::from_secs(8));
        Ok(client)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framing::encode_frame;
    use prost::Message;
    use std::io;

    /// Scripted transport: each `read` consumes one event; an empty script
    /// behaves like a quiet line (read timeout). All writes are recorded.
    struct MockTransport {
        reads: VecDeque<Vec<u8>>,
        writes: Vec<Vec<u8>>,
        /// When set, the mock stays quiet until this many writes arrived,
        /// then delivers one unlock response echoing `respond_id`.
        respond_after_writes: Option<(usize, u32)>,
    }

    impl MockTransport {
        fn new() -> Self {
            Self {
                reads: VecDeque::new(),
                writes: Vec::new(),
                respond_after_writes: None,
            }
        }

        fn queue(&mut self, data: Vec<u8>) {
            self.reads.push_back(data);
        }
    }

    impl Read for MockTransport {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if let Some((min_writes, request_id)) = self.respond_after_writes
                && self.writes.len() >= min_writes
            {
                self.respond_after_writes = None;
                self.reads.push_back(unlock_response(request_id));
            }

            match self.reads.pop_front() {
                Some(data) => {
                    let len = data.len().min(buf.len());
                    buf[..len].copy_from_slice(&data[..len]);
                    assert_eq!(len, data.len(), "test chunk larger than read buffer");
                    Ok(len)
                }
                None => Err(io::Error::new(io::ErrorKind::TimedOut, "quiet line")),
            }
        }
    }

    impl Write for MockTransport {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.writes.push(buf.to_vec());
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn encode_response(response: &studio::Response) -> Vec<u8> {
        encode_frame(&response.encode_to_vec())
    }

    /// A core get-lock-state response reporting "unlocked".
    fn unlock_response(request_id: u32) -> Vec<u8> {
        encode_response(&studio::Response {
            r#type: Some(studio::response::Type::RequestResponse(
                studio::RequestResponse {
                    request_id,
                    subsystem: Some(studio::request_response::Subsystem::Core(
                        zmk::core::Response {
                            response_type: Some(zmk::core::response::ResponseType::GetLockState(
                                zmk::core::LockState::ZmkStudioCoreLockStateUnlocked as i32,
                            )),
                        },
                    )),
                },
            )),
        })
    }

    /// A keymap response carrying the same request ID (subsystem mismatch for
    /// a core request).
    fn keymap_response(request_id: u32) -> Vec<u8> {
        encode_response(&studio::Response {
            r#type: Some(studio::response::Type::RequestResponse(
                studio::RequestResponse {
                    request_id,
                    subsystem: Some(studio::request_response::Subsystem::Keymap(
                        zmk::keymap::Response {
                            response_type: Some(
                                zmk::keymap::response::ResponseType::CheckUnsavedChanges(false),
                            ),
                        },
                    )),
                },
            )),
        })
    }

    fn notification_response() -> Vec<u8> {
        encode_response(&studio::Response {
            r#type: Some(studio::response::Type::Notification(
                studio::Notification::default(),
            )),
        })
    }

    fn test_client() -> StudioClient<MockTransport> {
        let mut client = StudioClient::new(MockTransport::new());
        client.set_call_timeout(Duration::from_millis(200));
        client
    }

    /// A keymap response holding a single layer with a single binding.
    fn one_binding_keymap_response(
        request_id: u32,
        binding: zmk::keymap::BehaviorBinding,
    ) -> Vec<u8> {
        encode_response(&studio::Response {
            r#type: Some(studio::response::Type::RequestResponse(
                studio::RequestResponse {
                    request_id,
                    subsystem: Some(studio::request_response::Subsystem::Keymap(
                        zmk::keymap::Response {
                            response_type: Some(zmk::keymap::response::ResponseType::GetKeymap(
                                zmk::keymap::Keymap {
                                    layers: vec![zmk::keymap::Layer {
                                        id: 0,
                                        name: "BASE".to_string(),
                                        bindings: vec![binding],
                                    }],
                                    available_layers: 0,
                                    max_layer_name_length: 16,
                                },
                            )),
                        },
                    )),
                },
            )),
        })
    }

    fn behavior_details_response(
        request_id: u32,
        details: zmk::behaviors::GetBehaviorDetailsResponse,
    ) -> Vec<u8> {
        encode_response(&studio::Response {
            r#type: Some(studio::response::Type::RequestResponse(
                studio::RequestResponse {
                    request_id,
                    subsystem: Some(studio::request_response::Subsystem::Behaviors(
                        zmk::behaviors::Response {
                            response_type: Some(
                                zmk::behaviors::response::ResponseType::GetBehaviorDetails(details),
                            ),
                        },
                    )),
                },
            )),
        })
    }

    /// A keymap using a behavior from the user's own keymap — a home row mod —
    /// resolves to `Custom` with both key parameters typed, instead of the raw
    /// numbers a name lookup alone can offer.
    #[test]
    fn keymap_using_a_custom_behavior_resolves_its_parameters() {
        use crate::binding::BehaviorParam;
        use crate::keycode::Keycode;
        use zmk::behaviors::{
            BehaviorBindingParametersSet, BehaviorParameterHidUsage,
            BehaviorParameterValueDescription, behavior_parameter_value_description::ValueType,
        };

        let hold = Keycode::LEFT_SHIFT.to_hid_usage();
        let tap = Keycode::A.to_hid_usage();
        let hid_usage = || BehaviorParameterValueDescription {
            name: String::new(),
            value_type: Some(ValueType::HidUsage(BehaviorParameterHidUsage {
                keyboard_max: 0xFF,
                consumer_max: 0xFF,
            })),
        };

        let mut client = test_client();
        let id = client.next_request_id;
        client.io.queue(one_binding_keymap_response(
            id,
            zmk::keymap::BehaviorBinding {
                behavior_id: 27,
                param1: hold,
                param2: tap,
            },
        ));
        client.io.queue(behavior_details_response(
            id + 1,
            zmk::behaviors::GetBehaviorDetailsResponse {
                id: 27,
                display_name: "home_row_mod_left".to_string(),
                metadata: vec![BehaviorBindingParametersSet {
                    param1: vec![hid_usage()],
                    param2: vec![hid_usage()],
                }],
            },
        ));

        let layers = client.resolve_keymap().expect("call should succeed");

        assert_eq!(
            layers[0].bindings,
            vec![Behavior::Custom {
                behavior_id: 27,
                display_name: "home_row_mod_left".to_string(),
                param1: BehaviorParam::Keycode(HidUsage::from_encoded(hold)),
                param2: BehaviorParam::Keycode(HidUsage::from_encoded(tap)),
            }]
        );
    }

    #[test]
    fn stale_response_with_other_request_id_is_skipped() {
        let mut client = test_client();
        let id = client.next_request_id;
        client.io.queue(unlock_response(id.wrapping_sub(7)));
        client.io.queue(unlock_response(id));

        let state = client.get_lock_state().expect("call should succeed");
        assert_eq!(state, zmk::core::LockState::ZmkStudioCoreLockStateUnlocked);
    }

    #[test]
    fn garbage_and_truncated_frames_are_skipped() {
        let mut client = test_client();
        let id = client.next_request_id;
        // Garbage bytes, then a frame that never completes before the next
        // SOF: exactly what a stale device transmit buffer produces.
        client.io.queue(vec![0x01, 0x02, 0xAB, 0x33, 0x34]);
        client.io.queue(unlock_response(id));

        let state = client.get_lock_state().expect("call should succeed");
        assert_eq!(state, zmk::core::LockState::ZmkStudioCoreLockStateUnlocked);
    }

    #[test]
    fn same_id_with_wrong_subsystem_is_skipped() {
        let mut client = test_client();
        let id = client.next_request_id;
        client.io.queue(keymap_response(id));
        client.io.queue(unlock_response(id));

        let state = client.get_lock_state().expect("call should succeed");
        assert_eq!(state, zmk::core::LockState::ZmkStudioCoreLockStateUnlocked);
    }

    #[test]
    fn notifications_are_queued_during_call() {
        let mut client = test_client();
        let id = client.next_request_id;
        client.io.queue(notification_response());
        client.io.queue(unlock_response(id));

        client.get_lock_state().expect("call should succeed");
        assert!(client.next_notification().is_some());
    }

    #[test]
    fn quiet_line_times_out() {
        let mut client = test_client();
        client.set_call_timeout(Duration::from_millis(50));

        match client.get_lock_state() {
            Err(ClientError::Timeout { .. }) => {}
            other => panic!("Expected timeout, got {other:?}"),
        }
    }

    #[test]
    fn request_is_resent_within_budget() {
        let mut client = test_client();
        let id = client.next_request_id;
        client.io.respond_after_writes = Some((2, id));

        let state = client.get_lock_state().expect("call should succeed");
        assert_eq!(state, zmk::core::LockState::ZmkStudioCoreLockStateUnlocked);
        assert!(
            client.io.writes.len() >= 2,
            "expected at least two sends, got {}",
            client.io.writes.len()
        );
    }

    #[test]
    fn drain_discards_stale_input() {
        let mut client = test_client();
        let id = client.next_request_id;
        client.io.queue(unlock_response(id.wrapping_sub(1)));
        client.io.queue(vec![0xAB, 0x55]); // truncated frame

        client.drain_stale_input(Duration::from_secs(1));

        client.io.queue(unlock_response(id));
        let state = client.get_lock_state().expect("call should succeed");
        assert_eq!(state, zmk::core::LockState::ZmkStudioCoreLockStateUnlocked);
    }
}
