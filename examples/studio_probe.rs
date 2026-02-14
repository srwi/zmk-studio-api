use std::error::Error;
use std::io::{Read, Write};
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use zmk_studio_rust_client::binding::BindingKind;
use zmk_studio_rust_client::client::{ClientError, StudioClient};
use zmk_studio_rust_client::keycode::{self, KeyCode, KeyboardCode};
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
            let current_kind = client.get_binding_kind_at(first_layer.id, 0)?;
            println!(
                "current first-key binding: {}",
                binding_kind_summary(&current_kind)
            );

            let next_kind = match current_kind {
                BindingKind::KeyPress(_) => BindingKind::KeyPress(keycode),
                BindingKind::KeyToggle(_) => BindingKind::KeyToggle(keycode),
                BindingKind::LayerTap { layer_id, .. } => BindingKind::LayerTap {
                    layer_id,
                    tap: keycode,
                },
                BindingKind::ModTap { hold, .. } => BindingKind::ModTap { hold, tap: keycode },
                _ => {
                    println!(
                        "first key binding is not one of KeyPress/KeyToggle/LayerTap/ModTap, skipping edit"
                    );
                    return Ok(());
                }
            };

            let key_name = keycode.to_zmk_name().unwrap_or("UNKNOWN");
            println!("setting random tap key '{key_name}'");
            client.set_binding_kind_at(first_layer.id, 0, next_kind)?;

            let read_kind = client.get_binding_kind_at(first_layer.id, 0)?;
            println!(
                "read back first-key binding: {}",
                binding_kind_summary(&read_kind)
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

fn random_letter_key() -> KeyCode {
    const LETTERS: [u32; 26] = [
        keycode::zmk_keys::A.raw(),
        keycode::zmk_keys::B.raw(),
        keycode::zmk_keys::C.raw(),
        keycode::zmk_keys::D.raw(),
        keycode::zmk_keys::E.raw(),
        keycode::zmk_keys::F.raw(),
        keycode::zmk_keys::G.raw(),
        keycode::zmk_keys::H.raw(),
        keycode::zmk_keys::I.raw(),
        keycode::zmk_keys::J.raw(),
        keycode::zmk_keys::K.raw(),
        keycode::zmk_keys::L.raw(),
        keycode::zmk_keys::M.raw(),
        keycode::zmk_keys::N.raw(),
        keycode::zmk_keys::O.raw(),
        keycode::zmk_keys::P.raw(),
        keycode::zmk_keys::Q.raw(),
        keycode::zmk_keys::R.raw(),
        keycode::zmk_keys::S.raw(),
        keycode::zmk_keys::T.raw(),
        keycode::zmk_keys::U.raw(),
        keycode::zmk_keys::V.raw(),
        keycode::zmk_keys::W.raw(),
        keycode::zmk_keys::X.raw(),
        keycode::zmk_keys::Y.raw(),
        keycode::zmk_keys::Z.raw(),
    ];

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    let idx = (now as usize) % LETTERS.len();
    KeyCode::from_hid_usage(LETTERS[idx])
}

fn binding_kind_summary(kind: &BindingKind) -> String {
    match kind {
        BindingKind::KeyPress(k) => format!("KeyPress({})", keycode_summary(*k)),
        BindingKind::KeyToggle(k) => format!("KeyToggle({})", keycode_summary(*k)),
        BindingKind::LayerTap { layer_id, tap } => {
            format!("LayerTap(layer={layer_id}, tap={})", keycode_summary(*tap))
        }
        BindingKind::ModTap { hold, tap } => {
            format!(
                "ModTap(hold={}, tap={})",
                keycode_summary(*hold),
                keycode_summary(*tap)
            )
        }
        BindingKind::MomentaryLayer { layer_id } => format!("MomentaryLayer({layer_id})"),
        BindingKind::ToggleLayer { layer_id } => format!("ToggleLayer({layer_id})"),
        BindingKind::ToLayer { layer_id } => format!("ToLayer({layer_id})"),
        BindingKind::Transparent => "Transparent".to_string(),
        BindingKind::None => "None".to_string(),
        BindingKind::Raw(raw) => format!(
            "Raw(behavior_id={}, param1={}, param2={})",
            raw.behavior_id, raw.param1, raw.param2
        ),
    }
}

fn keycode_summary(key: KeyCode) -> String {
    match key {
        KeyCode::Keyboard(KeyboardCode::Modifier(m)) => format!("Keyboard::{m:?}"),
        KeyCode::Keyboard(KeyboardCode::UsageId(id)) => format!("Keyboard::UsageId({id})"),
        KeyCode::Consumer(id) => format!("Consumer({id})"),
        KeyCode::GenericDesktop(id) => format!("GenericDesktop({id})"),
        KeyCode::Other(raw) => format!("Other(page={}, id={})", raw.page, raw.id),
    }
}
