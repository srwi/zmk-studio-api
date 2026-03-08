#!/usr/bin/env python3
from __future__ import annotations

import argparse
import zmk_studio_api as zmk


def run(client: zmk.StudioClient) -> None:
    print("Lock state:", client.get_lock_state())
    behavior_ids = client.list_all_behaviors()
    print("Behavior count:", len(behavior_ids))

    if behavior_ids:
        details = client.get_behavior_details_bytes(behavior_ids[0])
        print("First behavior details bytes:", len(details))

    keymap_bytes = client.get_keymap_bytes()
    print("Keymap bytes:", len(keymap_bytes))

    layouts_bytes = client.get_physical_layouts_bytes()
    print("Physical layouts bytes:", len(layouts_bytes))

    # Demonstrate typed behavior get/set at (layer 0, position 0).
    before = client.get_key_at(0, 0)
    print("Before:", before)

    client.set_key_at(0, 0, zmk.KeyPress(zmk.Keycode.A))
    after = client.get_key_at(0, 0)
    print("After:", after)

    # Avoid persisting the example mutation.
    if client.check_unsaved_changes():
        client.discard_changes()
        print("Discarded changes")


def main() -> int:
    parser = argparse.ArgumentParser(description="Basic zmk_studio_api example")
    sub = parser.add_subparsers(dest="transport", required=True)

    serial = sub.add_parser("serial", help="Connect over serial")
    serial.add_argument("port", help="Serial port path (for example COM3)")

    ble = sub.add_parser("ble", help="Connect over BLE")

    args = parser.parse_args()

    if args.transport == "serial":
        client = zmk.StudioClient.open_serial(args.port)
    else:
        devices = zmk.StudioClient.list_ble_devices()
        if not devices:
            raise SystemExit("No Bluetooth keyboards found. Pair/connect your keyboard first.")

        device_id, local_name = devices[0]
        print("Using BLE device:", local_name or device_id)
        client = zmk.StudioClient.open_ble(device_id)

    run(client)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
