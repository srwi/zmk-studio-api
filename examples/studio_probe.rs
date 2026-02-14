use std::error::Error;
use std::io::{Read, Write};
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use zmk_studio_rust_client::binding::Behavior;
use zmk_studio_rust_client::client::{ClientError, StudioClient};
use zmk_studio_rust_client::keycode::Keycode;
use zmk_studio_rust_client::proto::zmk::meta::ErrorConditions;
#[cfg(feature = "ble")]
use zmk_studio_rust_client::transport::ble::{BleConnectOptions, BleTransport};
#[cfg(feature = "serial")]
use zmk_studio_rust_client::transport::serial::SerialTransport;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let Some(mode) = args.next() else {
        print_usage();
        return Ok(());
    };

    match mode.as_str() {
        "serial" => {
            #[cfg(feature = "serial")]
            {
                let Some(port) = args.next() else {
                    eprintln!("missing serial port name");
                    print_usage();
                    return Ok(());
                };
                let client = StudioClient::new(SerialTransport::open(&port)?);
                run_probe(client)
            }
            #[cfg(not(feature = "serial"))]
            {
                Err("this binary was built without the `serial` feature".into())
            }
        }
        "ble" => {
            #[cfg(feature = "ble")]
            {
                let name_contains = args.next();
                let options = BleConnectOptions {
                    name_contains,
                    ..Default::default()
                };
                let client = StudioClient::new(BleTransport::connect_with_options(options)?);
                run_probe(client)
            }
            #[cfg(not(feature = "ble"))]
            {
                Err("this binary was built without the `ble` feature".into())
            }
        }
        _ => {
            eprintln!("unknown mode: {mode}");
            print_usage();
            Ok(())
        }
    }
}

fn run_probe<T: Read + Write>(mut client: StudioClient<T>) -> Result<(), Box<dyn Error>> {
    let info = client.get_device_info()?;
    println!("device name: {}", info.name);
    println!("serial number bytes: {}", info.serial_number.len());

    let lock_state = client.get_lock_state()?;
    println!("lock state: {}", lock_state.as_str_name());

    match client.get_keymap() {
        Ok(keymap) => {
            println!("keymap loaded");
            println!("layers: {}", keymap.layers.len());
            println!("available layer slots: {}", keymap.available_layers);
            println!("max layer name length: {}", keymap.max_layer_name_length);

            if let Some(first_layer) = keymap.layers.first() {
                println!(
                    "first layer: id={} name='{}' bindings={}",
                    first_layer.id,
                    first_layer.name,
                    first_layer.bindings.len()
                );
            }

            let layouts = client.get_physical_layouts()?;
            println!(
                "physical layouts: {} (active index: {})",
                layouts.layouts.len(),
                layouts.active_layout_index
            );

            let Some(first_layer) = keymap.layers.first() else {
                println!("no layers found, skipping edit test");
                return Ok(());
            };
            if first_layer.bindings.is_empty() {
                println!("first layer has no bindings, skipping edit test");
                return Ok(());
            }

            let keycode = random_letter_key();
            let current_behavior = client.get_key_at(first_layer.id, 0)?;
            println!(
                "current first-key binding: {}",
                behavior_summary(&current_behavior)
            );

            let next_behavior = match current_behavior {
                Behavior::KeyPress(_) => Behavior::KeyPress(keycode),
                Behavior::KeyToggle(_) => Behavior::KeyToggle(keycode),
                Behavior::LayerTap { layer_id, .. } => Behavior::LayerTap {
                    layer_id,
                    tap: keycode,
                },
                Behavior::ModTap { hold, .. } => Behavior::ModTap { hold, tap: keycode },
                _ => {
                    println!(
                        "first key binding is not one of KeyPress/KeyToggle/LayerTap/ModTap, skipping edit"
                    );
                    return Ok(());
                }
            };

            let key_name = keycode.to_name().unwrap_or("UNKNOWN");
            println!("setting random tap key '{key_name}'");
            client.set_key_at(first_layer.id, 0, next_behavior)?;

            let read_behavior = client.get_key_at(first_layer.id, 0)?;
            println!(
                "read back first-key binding: {}",
                behavior_summary(&read_behavior)
            );

            println!("note: change is not saved; call save_changes() if you want it persisted.");
        }
        Err(ClientError::Meta(ErrorConditions::UnlockRequired)) => {
            println!("device is locked for secured RPCs.");
            println!("press your `&studio_unlock` key on the keyboard, then rerun this command.");
        }
        Err(err) => return Err(Box::new(err)),
    }

    Ok(())
}

fn print_usage() {
    println!("Usage:");
    println!("  cargo run --example studio_probe -- serial <PORT>");
    println!("  cargo run --example studio_probe --features ble -- ble [NAME_SUBSTRING]");
}

fn random_letter_key() -> Keycode {
    const LETTERS: [Keycode; 26] = [
        Keycode::A,
        Keycode::B,
        Keycode::C,
        Keycode::D,
        Keycode::E,
        Keycode::F,
        Keycode::G,
        Keycode::H,
        Keycode::I,
        Keycode::J,
        Keycode::K,
        Keycode::L,
        Keycode::M,
        Keycode::N,
        Keycode::O,
        Keycode::P,
        Keycode::Q,
        Keycode::R,
        Keycode::S,
        Keycode::T,
        Keycode::U,
        Keycode::V,
        Keycode::W,
        Keycode::X,
        Keycode::Y,
        Keycode::Z,
    ];

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    let idx = (now as usize) % LETTERS.len();
    LETTERS[idx]
}

fn behavior_summary(behavior: &Behavior) -> String {
    match behavior {
        Behavior::KeyPress(k) => format!("KeyPress({})", keycode_summary(*k)),
        Behavior::KeyToggle(k) => format!("KeyToggle({})", keycode_summary(*k)),
        Behavior::LayerTap { layer_id, tap } => {
            format!("LayerTap(layer={layer_id}, tap={})", keycode_summary(*tap))
        }
        Behavior::ModTap { hold, tap } => {
            format!(
                "ModTap(hold={}, tap={})",
                keycode_summary(*hold),
                keycode_summary(*tap)
            )
        }
        Behavior::MomentaryLayer { layer_id } => format!("MomentaryLayer({layer_id})"),
        Behavior::ToggleLayer { layer_id } => format!("ToggleLayer({layer_id})"),
        Behavior::ToLayer { layer_id } => format!("ToLayer({layer_id})"),
        Behavior::Transparent => "Transparent".to_string(),
        Behavior::None => "None".to_string(),
        Behavior::Raw(raw) => format!(
            "Raw(behavior_id={}, param1={}, param2={})",
            raw.behavior_id, raw.param1, raw.param2
        ),
    }
}

fn keycode_summary(key: Keycode) -> String {
    if let Some(name) = key.to_name() {
        return name.to_string();
    }
    format!("0x{:08X}", key.to_hid_usage())
}
