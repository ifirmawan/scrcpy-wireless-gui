# scrcpy wireless

A tiny Rust/egui front-end for mirroring an Android device over WiFi with
[scrcpy](https://github.com/Genymobile/scrcpy). It wraps the manual
`adb pair → find port → adb connect → scrcpy` workflow behind a few buttons,
and remembers your devices for one-click reconnects.

> This is just a GUI wrapper — it shells out to the `adb` and `scrcpy`
> binaries from your existing scrcpy install. It does **not** bundle or
> redistribute scrcpy.

## Features

- **Pair** with a phone using the IP:port + code from Android's *Wireless
  debugging → Pair device with pairing code* dialog.
- **Find port & Connect** — parallel-scans the device for the (random)
  wireless-debugging connect port and connects, since mDNS is often blocked.
- **Saved devices** — store `name → ip:port` and reconnect from a dropdown.
- **Quick Connect & Mirror** — connect to a saved (pinned) port and launch
  the mirror in one click, with automatic fallback to a port scan if the
  pinned port is gone (e.g. after a reboot).

## Build

```bash
cargo build --release
./target/release/scrcpy-wireless-gui
```

The app looks for `adb`/`scrcpy` in `$SCRCPY_DIR`, then the folder your scrcpy
release lives in, then the current directory.

## Usage

1. On the phone: *Developer options → Wireless debugging → Pair device with
   pairing code*. Enter that **IP:port** and **code** in the app, click **Pair**.
2. Enter the phone's **Device IP**, click **Find port & Connect**.
3. Click **▶ Start Mirror**.

### One-click reconnects

Pin a fixed port so you can skip pairing/scanning next time:

```bash
adb -s <ip>:<connect-port> tcpip 5555   # or over USB
```

Then in the app **Save** `name = My Phone`, `ip:port = <ip>:5555`. From then on:
pick it in the dropdown → **⚡ Quick Connect & Mirror**.

> Note: `adb tcpip` holds until the phone reboots; after a reboot, Quick
> Connect automatically falls back to scanning for the new port.

## Requirements

- Phone and PC on the same WiFi subnet.
- Developer options + Wireless debugging enabled.
- A scrcpy release (providing `adb` and `scrcpy`).

## License

[GPL-3.0-or-later](LICENSE).
