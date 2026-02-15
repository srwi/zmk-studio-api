use std::io::{Read, Write};
use std::sync::Mutex;

use prost::Message;
use pyo3::exceptions::{PyRuntimeError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyBytes, PyDict, PyModule};
use strum::IntoEnumIterator;

#[cfg(feature = "ble")]
use crate::transport::ble::BleTransport;
#[cfg(feature = "serial")]
use crate::transport::serial::SerialTransport;
use crate::{Behavior, ClientError, HidUsage, Keycode, StudioClient};

trait ReadWriteSend: Read + Write + Send {}
impl<T: Read + Write + Send> ReadWriteSend for T {}

type DynClient = StudioClient<Box<dyn ReadWriteSend>>;

#[pyclass(name = "Behavior")]
#[derive(Clone)]
pub struct PyBehavior {
    inner: Behavior,
}

impl PyBehavior {
    fn new(inner: Behavior) -> Self {
        Self { inner }
    }
}

#[pymethods]
impl PyBehavior {
    #[getter]
    pub fn kind(&self) -> &'static str {
        match self.inner {
            Behavior::KeyPress(_) => "KeyPress",
            Behavior::KeyToggle(_) => "KeyToggle",
            Behavior::LayerTap { .. } => "LayerTap",
            Behavior::ModTap { .. } => "ModTap",
            Behavior::StickyKey(_) => "StickyKey",
            Behavior::StickyLayer { .. } => "StickyLayer",
            Behavior::MomentaryLayer { .. } => "MomentaryLayer",
            Behavior::ToggleLayer { .. } => "ToggleLayer",
            Behavior::ToLayer { .. } => "ToLayer",
            Behavior::Bluetooth { .. } => "Bluetooth",
            Behavior::ExternalPower { .. } => "ExternalPower",
            Behavior::OutputSelection { .. } => "OutputSelection",
            Behavior::Backlight { .. } => "Backlight",
            Behavior::Underglow { .. } => "Underglow",
            Behavior::MouseKeyPress { .. } => "MouseKeyPress",
            Behavior::MouseMove { .. } => "MouseMove",
            Behavior::MouseScroll { .. } => "MouseScroll",
            Behavior::CapsWord => "CapsWord",
            Behavior::KeyRepeat => "KeyRepeat",
            Behavior::Reset => "Reset",
            Behavior::Bootloader => "Bootloader",
            Behavior::SoftOff => "SoftOff",
            Behavior::StudioUnlock => "StudioUnlock",
            Behavior::GraveEscape => "GraveEscape",
            Behavior::Transparent => "Transparent",
            Behavior::None => "None",
            Behavior::Unknown { .. } => "Unknown",
        }
    }

    fn __repr__(&self) -> String {
        format!("Behavior({:?})", self.inner)
    }
}

#[pyclass(name = "StudioClient")]
pub struct PyStudioClient {
    inner: Mutex<DynClient>,
}

#[pymethods]
impl PyStudioClient {
    #[staticmethod]
    #[cfg(feature = "serial")]
    pub fn open_serial(path: &str) -> PyResult<Self> {
        let transport = SerialTransport::open(path).map_err(|err| {
            PyRuntimeError::new_err(format!("failed to open serial transport: {err}"))
        })?;
        Ok(Self {
            inner: Mutex::new(StudioClient::new(Box::new(transport))),
        })
    }

    #[staticmethod]
    #[cfg(not(feature = "serial"))]
    pub fn open_serial(_path: &str) -> PyResult<Self> {
        Err(PyRuntimeError::new_err(
            "serial support is disabled for this build",
        ))
    }

    #[staticmethod]
    #[cfg(feature = "ble")]
    pub fn connect_ble() -> PyResult<Self> {
        let transport = BleTransport::connect_first().map_err(|err| {
            PyRuntimeError::new_err(format!("failed to connect BLE transport: {err}"))
        })?;
        Ok(Self {
            inner: Mutex::new(StudioClient::new(Box::new(transport))),
        })
    }

    #[staticmethod]
    #[cfg(not(feature = "ble"))]
    pub fn connect_ble() -> PyResult<Self> {
        Err(PyRuntimeError::new_err(
            "ble support is disabled for this build",
        ))
    }

    pub fn get_lock_state(&self) -> PyResult<String> {
        let state = self.with_client(|client| client.get_lock_state())?;
        Ok(state.as_str_name().to_string())
    }

    pub fn reset_settings(&self) -> PyResult<bool> {
        self.with_client(|client| client.reset_settings())
    }

    pub fn list_all_behaviors(&self) -> PyResult<Vec<u32>> {
        self.with_client(|client| client.list_all_behaviors())
    }

    pub fn get_device_info_bytes<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyBytes>> {
        let info = self.with_client(|client| client.get_device_info())?;
        Ok(PyBytes::new(py, &info.encode_to_vec()))
    }

    pub fn get_behavior_details_bytes<'py>(
        &self,
        py: Python<'py>,
        behavior_id: u32,
    ) -> PyResult<Bound<'py, PyBytes>> {
        let details = self.with_client(|client| client.get_behavior_details(behavior_id))?;
        Ok(PyBytes::new(py, &details.encode_to_vec()))
    }

    pub fn get_keymap_bytes<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyBytes>> {
        let keymap = self.with_client(|client| client.get_keymap())?;
        Ok(PyBytes::new(py, &keymap.encode_to_vec()))
    }

    pub fn get_physical_layouts_bytes<'py>(
        &self,
        py: Python<'py>,
    ) -> PyResult<Bound<'py, PyBytes>> {
        let layouts = self.with_client(|client| client.get_physical_layouts())?;
        Ok(PyBytes::new(py, &layouts.encode_to_vec()))
    }

    pub fn get_key_at(&self, layer_id: u32, key_position: i32) -> PyResult<PyBehavior> {
        let behavior = self.with_client(|client| client.get_key_at(layer_id, key_position))?;
        Ok(PyBehavior::new(behavior))
    }

    pub fn set_key_at(
        &self,
        layer_id: u32,
        key_position: i32,
        behavior: PyBehavior,
    ) -> PyResult<()> {
        self.with_client(|client| client.set_key_at(layer_id, key_position, behavior.inner))
    }

    pub fn check_unsaved_changes(&self) -> PyResult<bool> {
        self.with_client(|client| client.check_unsaved_changes())
    }

    pub fn save_changes(&self) -> PyResult<()> {
        self.with_client(|client| client.save_changes())
    }

    pub fn discard_changes(&self) -> PyResult<bool> {
        self.with_client(|client| client.discard_changes())
    }
}

impl PyStudioClient {
    fn with_client<R>(
        &self,
        f: impl FnOnce(&mut DynClient) -> Result<R, ClientError>,
    ) -> PyResult<R> {
        let mut client = self
            .inner
            .lock()
            .map_err(|_| PyRuntimeError::new_err("client mutex is poisoned"))?;
        f(&mut client).map_err(|err| PyRuntimeError::new_err(err.to_string()))
    }
}

fn parse_hid_usage(value: &Bound<'_, PyAny>) -> PyResult<HidUsage> {
    if let Ok(encoded) = value.extract::<u32>() {
        return Ok(HidUsage::from_encoded(encoded));
    }

    if let Ok(name) = value.extract::<String>() {
        let keycode = Keycode::from_name(&name)
            .ok_or_else(|| PyValueError::new_err(format!("invalid keycode name: {name}")))?;
        return Ok(HidUsage::from_encoded(keycode.to_hid_usage()));
    }

    Err(PyTypeError::new_err(
        "key must be a Keycode/int or keycode name string",
    ))
}

#[pyfunction(name = "KeyPress")]
fn key_press(key: &Bound<'_, PyAny>) -> PyResult<PyBehavior> {
    Ok(PyBehavior::new(Behavior::KeyPress(parse_hid_usage(key)?)))
}

#[pyfunction(name = "KeyToggle")]
fn key_toggle(key: &Bound<'_, PyAny>) -> PyResult<PyBehavior> {
    Ok(PyBehavior::new(Behavior::KeyToggle(parse_hid_usage(key)?)))
}

#[pyfunction(name = "LayerTap")]
fn layer_tap(layer_id: u32, tap: &Bound<'_, PyAny>) -> PyResult<PyBehavior> {
    Ok(PyBehavior::new(Behavior::LayerTap {
        layer_id,
        tap: parse_hid_usage(tap)?,
    }))
}

#[pyfunction(name = "ModTap")]
fn mod_tap(hold: &Bound<'_, PyAny>, tap: &Bound<'_, PyAny>) -> PyResult<PyBehavior> {
    Ok(PyBehavior::new(Behavior::ModTap {
        hold: parse_hid_usage(hold)?,
        tap: parse_hid_usage(tap)?,
    }))
}

#[pyfunction(name = "StickyKey")]
fn sticky_key(key: &Bound<'_, PyAny>) -> PyResult<PyBehavior> {
    Ok(PyBehavior::new(Behavior::StickyKey(parse_hid_usage(key)?)))
}

#[pyfunction(name = "StickyLayer")]
fn sticky_layer(layer_id: u32) -> PyBehavior {
    PyBehavior::new(Behavior::StickyLayer { layer_id })
}

#[pyfunction(name = "MomentaryLayer")]
fn momentary_layer(layer_id: u32) -> PyBehavior {
    PyBehavior::new(Behavior::MomentaryLayer { layer_id })
}

#[pyfunction(name = "ToggleLayer")]
fn toggle_layer(layer_id: u32) -> PyBehavior {
    PyBehavior::new(Behavior::ToggleLayer { layer_id })
}

#[pyfunction(name = "ToLayer")]
fn to_layer(layer_id: u32) -> PyBehavior {
    PyBehavior::new(Behavior::ToLayer { layer_id })
}

#[pyfunction(name = "Bluetooth")]
fn bluetooth(command: u32, value: u32) -> PyBehavior {
    PyBehavior::new(Behavior::Bluetooth { command, value })
}

#[pyfunction(name = "ExternalPower")]
fn external_power(value: u32) -> PyBehavior {
    PyBehavior::new(Behavior::ExternalPower { value })
}

#[pyfunction(name = "OutputSelection")]
fn output_selection(value: u32) -> PyBehavior {
    PyBehavior::new(Behavior::OutputSelection { value })
}

#[pyfunction(name = "Backlight")]
fn backlight(command: u32, value: u32) -> PyBehavior {
    PyBehavior::new(Behavior::Backlight { command, value })
}

#[pyfunction(name = "Underglow")]
fn underglow(command: u32, value: u32) -> PyBehavior {
    PyBehavior::new(Behavior::Underglow { command, value })
}

#[pyfunction(name = "MouseKeyPress")]
fn mouse_key_press(value: u32) -> PyBehavior {
    PyBehavior::new(Behavior::MouseKeyPress { value })
}

#[pyfunction(name = "MouseMove")]
fn mouse_move(value: u32) -> PyBehavior {
    PyBehavior::new(Behavior::MouseMove { value })
}

#[pyfunction(name = "MouseScroll")]
fn mouse_scroll(value: u32) -> PyBehavior {
    PyBehavior::new(Behavior::MouseScroll { value })
}

#[pyfunction(name = "CapsWord")]
fn caps_word() -> PyBehavior {
    PyBehavior::new(Behavior::CapsWord)
}

#[pyfunction(name = "KeyRepeat")]
fn key_repeat() -> PyBehavior {
    PyBehavior::new(Behavior::KeyRepeat)
}

#[pyfunction(name = "Reset")]
fn reset() -> PyBehavior {
    PyBehavior::new(Behavior::Reset)
}

#[pyfunction(name = "Bootloader")]
fn bootloader() -> PyBehavior {
    PyBehavior::new(Behavior::Bootloader)
}

#[pyfunction(name = "SoftOff")]
fn soft_off() -> PyBehavior {
    PyBehavior::new(Behavior::SoftOff)
}

#[pyfunction(name = "StudioUnlock")]
fn studio_unlock() -> PyBehavior {
    PyBehavior::new(Behavior::StudioUnlock)
}

#[pyfunction(name = "GraveEscape")]
fn grave_escape() -> PyBehavior {
    PyBehavior::new(Behavior::GraveEscape)
}

#[pyfunction(name = "Transparent")]
fn transparent() -> PyBehavior {
    PyBehavior::new(Behavior::Transparent)
}

#[pyfunction(name = "NoBehavior")]
fn no_behavior() -> PyBehavior {
    PyBehavior::new(Behavior::None)
}

#[pyfunction(name = "Raw")]
fn raw(behavior_id: i32, param1: u32, param2: u32) -> PyBehavior {
    PyBehavior::new(Behavior::Unknown {
        behavior_id,
        param1,
        param2,
    })
}

#[pymodule]
fn zmk_studio_api(py: Python<'_>, module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<PyStudioClient>()?;
    module.add_class::<PyBehavior>()?;

    let enum_module = py.import("enum")?;
    let int_enum = enum_module.getattr("IntEnum")?;
    let members = PyDict::new(py);
    for keycode in Keycode::iter() {
        members.set_item(keycode.to_name(), keycode.to_hid_usage())?;
    }
    let keycode_enum = int_enum.call1(("Keycode", members))?;
    module.add("Keycode", keycode_enum)?;

    module.add_function(wrap_pyfunction!(key_press, module)?)?;
    module.add_function(wrap_pyfunction!(key_toggle, module)?)?;
    module.add_function(wrap_pyfunction!(layer_tap, module)?)?;
    module.add_function(wrap_pyfunction!(mod_tap, module)?)?;
    module.add_function(wrap_pyfunction!(sticky_key, module)?)?;
    module.add_function(wrap_pyfunction!(sticky_layer, module)?)?;
    module.add_function(wrap_pyfunction!(momentary_layer, module)?)?;
    module.add_function(wrap_pyfunction!(toggle_layer, module)?)?;
    module.add_function(wrap_pyfunction!(to_layer, module)?)?;
    module.add_function(wrap_pyfunction!(bluetooth, module)?)?;
    module.add_function(wrap_pyfunction!(external_power, module)?)?;
    module.add_function(wrap_pyfunction!(output_selection, module)?)?;
    module.add_function(wrap_pyfunction!(backlight, module)?)?;
    module.add_function(wrap_pyfunction!(underglow, module)?)?;
    module.add_function(wrap_pyfunction!(mouse_key_press, module)?)?;
    module.add_function(wrap_pyfunction!(mouse_move, module)?)?;
    module.add_function(wrap_pyfunction!(mouse_scroll, module)?)?;
    module.add_function(wrap_pyfunction!(caps_word, module)?)?;
    module.add_function(wrap_pyfunction!(key_repeat, module)?)?;
    module.add_function(wrap_pyfunction!(reset, module)?)?;
    module.add_function(wrap_pyfunction!(bootloader, module)?)?;
    module.add_function(wrap_pyfunction!(soft_off, module)?)?;
    module.add_function(wrap_pyfunction!(studio_unlock, module)?)?;
    module.add_function(wrap_pyfunction!(grave_escape, module)?)?;
    module.add_function(wrap_pyfunction!(transparent, module)?)?;
    module.add_function(wrap_pyfunction!(no_behavior, module)?)?;
    module.add_function(wrap_pyfunction!(raw, module)?)?;

    Ok(())
}
