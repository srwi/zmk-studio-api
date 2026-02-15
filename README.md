# zmk-studio-api

[![Version](https://img.shields.io/crates/v/zmk-studio-api.svg)](https://crates.io/crates/zmk-studio-api)
[![image](https://img.shields.io/pypi/v/zmk-studio-api.svg)](https://pypi.python.org/pypi/zmk-studio-api)
[![image](https://img.shields.io/pypi/l/zmk-studio-api.svg)](https://pypi.python.org/pypi/zmk-studio-api)

`zmk-studio-api` is a Rust client for the ZMK Studio RPC API on ZMK keyboards.
It can read device and keymap state, and apply keymap changes over serial or BLE.
Additionally, this library includes Python bindings for API access from Python applications and scripts.

## Usage

### Rust

Add dependency with Cargo:

```bash
cargo add zmk-studio-api [--features ble]
```

Usage example:

```rust
use zmk_studio_api::{Behavior, Keycode, StudioClient, transport::serial::SerialTransport};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut client = StudioClient::new(SerialTransport::open("COM3")?);
    let info = client.get_device_info()?;
    println!("Device: {}", info.name);
    println!("Lock: {:?}", client.get_lock_state()?);

    let before = client.get_key_at(0, 12)?;
    println!("Before: {before:?}");

    client.set_key_at(0, 12, Behavior::KeyPress(Keycode::A))?;
    let after = client.get_key_at(0, 12)?;
    println!("After: {after:?}");

    if client.check_unsaved_changes()? {
        client.discard_changes()?;
    }
    Ok(())
}
```

For a complete runnable example, see [`examples/basic_example.rs`](examples/basic_example.rs).

### Python

Install from PyPI:

```bash
pip install zmk-studio-api
```

Usage example:

```python
import zmk_studio_api as zmk

client = zmk.StudioClient.open_serial("COM3")
print("Lock:", client.get_lock_state())

before = client.get_key_at(0, 12)
print("Before:", before)

client.set_key_at(0, 12, zmk.KeyPress(zmk.Keycode.A))
after = client.get_key_at(0, 12)
print("After:", after)
```

For a complete runnable example, see [`examples/basic_example.py`](examples/basic_example.py).

# License & Attribution

This project is licensed under the [Apache 2.0](LICENSE) license. Parts of this project are based on code from the [ZMK Studio](https://github.com/zmkfirmware/zmk-studio) (Apache 2.0) and its [TypeScript client](https://github.com/zmkfirmware/zmk-studio-ts-client) implementation (MIT).