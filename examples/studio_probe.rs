use std::error::Error;
use std::io::{Read, Write};
use std::process::ExitCode;

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
                    print_usage();
                    return Ok(());
                };
                let client = StudioClient::new(SerialTransport::open(&port)?);
                run_probe(client)
            }
            #[cfg(not(feature = "serial"))]
            {
                Err("built without `serial` feature".into())
            }
        }
        "ble" => {
            #[cfg(feature = "ble")]
            {
                let options = BleConnectOptions {
                    name_contains: args.next(),
                    ..Default::default()
                };
                let client = StudioClient::new(BleTransport::connect_with_options(options)?);
                run_probe(client)
            }
            #[cfg(not(feature = "ble"))]
            {
                Err("built without `ble` feature".into())
            }
        }
        _ => {
            print_usage();
            Ok(())
        }
    }
}

fn run_probe<T: Read + Write>(mut client: StudioClient<T>) -> Result<(), Box<dyn Error>> {
    let info = client.get_device_info()?;
    println!("Device: {}", info.name);
    println!("Lock: {:?}", client.get_lock_state()?);

    let behavior_ids = client.list_all_behaviors()?;
    println!("Behavior count: {}", behavior_ids.len());
    if let Some(first_behavior_id) = behavior_ids.first().copied() {
        let details = client.get_behavior_details(first_behavior_id)?;
        println!("First behavior: {} ({})", details.id, details.display_name);
    }

    let keymap = match client.get_keymap() {
        Ok(keymap) => keymap,
        Err(ClientError::Meta(ErrorConditions::UnlockRequired)) => {
            println!("Unlock required for keymap APIs (`&studio_unlock` then rerun).");
            return Ok(());
        }
        Err(err) => return Err(Box::new(err)),
    };
    println!("Layers: {}", keymap.layers.len());

    let layouts = client.get_physical_layouts()?;
    println!(
        "Physical layouts: {} (active index: {})",
        layouts.layouts.len(),
        layouts.active_layout_index
    );

    let Some(first_layer) = keymap.layers.first() else {
        return Ok(());
    };
    if first_layer.bindings.is_empty() {
        return Ok(());
    }

    let layer_id = first_layer.id;
    let key_position = 0;

    let before = client.get_key_at(layer_id, key_position)?;
    println!("Before: {before:?}");

    client.set_key_at(layer_id, key_position, Behavior::KeyPress(Keycode::A))?;
    let after = client.get_key_at(layer_id, key_position)?;
    println!("After:  {after:?}");

    // Change management APIs.
    let has_changes = client.check_unsaved_changes()?;
    println!("Unsaved changes: {has_changes}");
    if has_changes {
        client.discard_changes()?;
    }

    Ok(())
}

fn print_usage() {
    println!("Usage:");
    println!("  cargo run --example studio_probe -- serial <PORT>");
    println!("  cargo run --example studio_probe --features ble -- ble [NAME_SUBSTRING]");
}
