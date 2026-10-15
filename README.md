# OpenKeyboard desktop

OpenKeyboard as a desktop app. It runs the [OpenKeyboard](https://github.com/OpenKeyboard-Pr0ject/openkeyboard) web app inside a [Tauri](https://tauri.app) window and gives it direct access to keyboards through [hidapi](https://github.com/libusb/hidapi) instead of WebHID.

The browser version cannot reach every keyboard interface. Chrome on Windows never offers the control interface of the Razer BlackWidow V3 Mini to web pages, because that interface describes its 90-byte feature report only with constant fields. The desktop app reads the report descriptor itself and reaches the interface normally.

## Requirements

- Node 20 or newer, and Rust with the MSVC toolchain on Windows.
- Windows 10 or 11 ships the WebView2 runtime that Tauri uses.

## Running

```
npm install
npm run tauri dev       # development window with hot reload
npm run tauri build     # installer in src-tauri/target/release/bundle
```

`npm run tauri build -- --no-bundle` builds only `src-tauri/target/release/openkeyboard-desktop.exe`.

## How it works

```
Tauri window
  OpenKeyboard web app (KeyboardConnection, drivers, panels)    ← ./package/openkeyboard/src
    └ NativeHid, NativeHidDevice                                 ← src/native-hid
         │ invoke / events
  Rust: HidService (hidapi)                                      ← src-tauri/src
         │
  keyboards
```

- **Same drivers:** `NativeHid` and `NativeHidDevice` implement the parts of `HID` and `HIDDevice` that the web app uses. The drivers, connection and panels are the web app's own modules.
- **Device list:** the web app's chooser filters (`keyboardRequestFilters()`) are passed to Rust, which lists only matching collections. There is no chooser; matching keyboards are available at once.
- **Collections:** Rust returns each collection's report descriptor, and `report-descriptor.ts` turns it into `HIDCollectionInfo`, the same shape WebHID provides. Constant fields count towards a report's length, which is what makes the Razer control report usable.
- **Reports:** output and feature reports go through Tauri commands. Each open device has a reader thread that forwards input reports as `hid-input-report` events.
- **Plugging and unplugging:** Rust checks the device list every 2 seconds and sends `hid-devices-changed`; `NativeHid` turns that into `connect` and `disconnect` events.

## Device reports

[`probe/`](probe) holds `openkeyboard-probe`, a separate command-line tool for people whose keyboard is not supported yet. It writes a JSON report with every HID collection, its report descriptor, the Windows driver chain and the installed vendor software. It sends no commands to any device. See [probe/README.md](probe/README.md).

## Development

```
npm run check                                   # typecheck, tests, production build of the frontend
cd src-tauri
cargo test                                      # Rust unit tests
cargo test --test razer_hardware -- --ignored   # read-only check on a connected BlackWidow V3 Mini
cargo clippy --all-targets -- -D warnings
```

Code style follows the OpenKeyboard repository: no comments in code, descriptive names, lines of at most 120 characters, en-US text.

## Verified

On 2026-10-14, on Windows 11 with a Razer BlackWidow V3 Mini (`1532:0258`, firmware 2.3) on the cable:

- `cargo test --test razer_hardware -- --ignored` listed the control interface with its descriptor and read the firmware version through `HidService`.
- The release build showed the keyboard with its Lighting and Battery and performance panels, saved a static effect, and restored spectrum cycling, both confirmed by reading back.

Wooting and VIA keyboards use the same code paths but have not been tried in the desktop app yet.

## License

AGPL-3.0, the same as OpenKeyboard, because the app includes its code. See [LICENSE](LICENSE).
