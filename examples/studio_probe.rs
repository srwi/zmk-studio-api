use std::error::Error;
use std::io::{Read, Write};
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use zmk_studio_rust_client::client::{ClientError, StudioClient};
use zmk_studio_rust_client::proto::zmk;
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

            let template = first_layer.bindings[0];
            let usage = random_letter_hid_usage();
            let expected_letter = hid_usage_to_letter(usage).unwrap_or('?');
            let binding = zmk::keymap::BehaviorBinding {
                behavior_id: template.behavior_id,
                param1: usage,
                param2: template.param2,
            };

            println!(
                "setting layer_id={} key_position=0 to random letter '{}' with behavior_id={}",
                first_layer.id, expected_letter, binding.behavior_id
            );

            client.set_layer_binding(first_layer.id, 0, binding)?;

            let updated = client.get_keymap()?;
            let read_binding = updated
                .layers
                .first()
                .and_then(|layer| layer.bindings.first())
                .copied();

            if let Some(binding) = read_binding {
                let read_letter = hid_usage_to_letter(binding.param1)
                    .or_else(|| hid_usage_to_letter(binding.param2))
                    .unwrap_or('?');
                println!(
                    "read back first key: behavior_id={} param1={} param2={} letter='{}'",
                    binding.behavior_id, binding.param1, binding.param2, read_letter
                );
            } else {
                println!("failed to read back first key after update");
            }

            println!("note: change is not saved; call save_changes() if you want it persisted.");
        }
        Err(ClientError::Meta(zmk::meta::ErrorConditions::UnlockRequired)) => {
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

fn random_letter_hid_usage() -> u32 {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    let letter_id = 4 + (now % 26);
    encode_keyboard_usage(letter_id)
}

fn hid_usage_to_letter(usage: u32) -> Option<char> {
    let usage_id = hid_usage_id(usage);
    if is_keyboard_usage_page(usage) && (4..=29).contains(&usage_id) {
        let offset = (usage_id - 4) as u8;
        Some((b'A' + offset) as char)
    } else {
        None
    }
}

const HID_USAGE_PAGE_KEYBOARD: u32 = 0x07;

fn encode_keyboard_usage(usage_id: u32) -> u32 {
    (HID_USAGE_PAGE_KEYBOARD << 16) | usage_id
}

fn hid_usage_page(usage: u32) -> u32 {
    (usage >> 16) & 0xFF
}

fn hid_usage_id(usage: u32) -> u32 {
    usage & 0xFFFF
}

fn is_keyboard_usage_page(usage: u32) -> bool {
    hid_usage_page(usage) == HID_USAGE_PAGE_KEYBOARD
}
