use std::io::{Read, Write};
use std::sync::Mutex;

use prost::Message;
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyBytes, PyDict, PyModule};
use strum::IntoEnumIterator;

#[cfg(feature = "ble")]
use crate::transport::ble::BleTransport;
#[cfg(feature = "serial")]
use crate::transport::serial::SerialTransport;
use crate::{Behavior, ClientError, Keycode, StudioClient};

trait ReadWriteSend: Read + Write + Send {}
impl<T: Read + Write + Send> ReadWriteSend for T {}

type DynClient = StudioClient<Box<dyn ReadWriteSend>>;

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

    pub fn get_key_at<'py>(
        &self,
        py: Python<'py>,
        layer_id: u32,
        key_position: i32,
    ) -> PyResult<Bound<'py, PyAny>> {
        let behavior = self.with_client(|client| client.get_key_at(layer_id, key_position))?;
        behavior.to_python(py)
    }

    pub fn set_key_at(
        &self,
        layer_id: u32,
        key_position: i32,
        behavior: &Bound<'_, PyAny>,
    ) -> PyResult<()> {
        let parsed = Behavior::from_python(behavior)?;
        self.with_client(|client| client.set_key_at(layer_id, key_position, parsed))
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

#[pymodule]
fn zmk_studio_api(py: Python<'_>, module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<PyStudioClient>()?;

    let enum_module = py.import("enum")?;
    let int_enum = enum_module.getattr("IntEnum")?;
    let members = PyDict::new(py);
    for keycode in Keycode::iter() {
        members.set_item(keycode.to_name(), keycode.to_hid_usage())?;
    }
    let keycode_enum = int_enum.call1(("Keycode", members))?;
    module.add("Keycode", keycode_enum)?;

    Ok(())
}
