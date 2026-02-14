# zmk-studio-api

`zmk-studio-api` is a Rust client for the ZMK Studio RPC API on ZMK keyboards.
It can read device and keymap state, and apply keymap changes over serial (default) or BLE (feature-gated).

Main capabilities:
- Core/device queries (device info, lock state, settings reset)
- Behavior discovery and metadata lookup
- Keymap and physical layout queries
- Key edits and layer operations (set key/binding, add/move/remove/restore layer, set layer properties)
- Change management (check, save, and discard pending changes)

## Usage

Add dependency with Cargo:

```bash
cargo add zmk-studio-api --git https://github.com/srwi/zmk-studio-api.git
```

or add to `Cargo.toml`:

```toml
[dependencies]
zmk-studio-api = { git = "https://github.com/srwi/zmk-studio-api.git" }
```

For BLE transport support, enable the `ble` feature instead:

```toml
[dependencies]
zmk-studio-api = { git = "https://github.com/srwi/zmk-studio-api.git", default-features = false, features = ["ble"] }
```

Minimal example:

```rust
use zmk_studio_api::{StudioClient, transport::serial::SerialTransport};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut client = StudioClient::new(SerialTransport::open("COM3")?);
    let info = client.get_device_info()?;
    println!("Device: {}", info.name);
    println!("Lock: {:?}", client.get_lock_state()?);
    Ok(())
}
```

For a complete runnable example, see `examples/basic_example.rs`.

# License & Attribution

Parts of this project are based on code from the [ZMK Studio](https://github.com/zmkfirmware/zmk-studio) (Apache 2.0) and its [TypeScript client](https://github.com/zmkfirmware/zmk-studio-ts-client) implementation (MIT). This project is licensed under the [Apache 2.0](LICENSE) license.