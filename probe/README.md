# openkeyboard-probe

A small command-line tool that writes a report about the keyboards on a computer. People who own a keyboard that OpenKeyboard does not support yet can run it and send the report, so support can start without the keyboard on the developer's desk.

It only lists devices and reads what the operating system already knows about them. **It sends no commands to any device.**

## Running

Double-click `openkeyboard-probe.exe`, or run it from a terminal:

```
openkeyboard-probe                          # devices with a keyboard collection
openkeyboard-probe --vendor-id 1532         # only one vendor, even without a keyboard collection
openkeyboard-probe --vendor-id 1532 --product-id 0258
openkeyboard-probe --all                    # every HID device
openkeyboard-probe --output razer.json
```

It prints a summary and writes `openkeyboard-device-report.json` to the current folder. When it is started by double-clicking, it waits for Enter before the window closes.

## What the report contains

| Field | Source | Why it helps |
|---|---|---|
| `devices[]` ids, release number, bus, product strings | hidapi enumeration | Identifies the model and firmware revision (`releaseNumber` is the USB `bcdDevice`) |
| `interfaces[].usagePage`, `usage`, `interfaceNumber` | hidapi | Shows which protocol family the keyboard speaks: `0xFF60:0x61` is VIA, `0xFF55` is Wooting, a 90-byte feature report on `0x0C:0x01` is Razer |
| `interfaces[].reportDescriptor` | hidapi, rebuilt from the Windows preparsed data | Every report ID and size, including the keyboard and consumer collections that WebHID hides |
| `interfaces[].windowsDevices[]` | CfgMgr32, from the HID collection up to the USB device | Driver, filter drivers (for example a vendor's own filter that keeps Chrome away), hardware IDs and the name each USB interface reports |
| `vendorSoftware[]` | Uninstall entries in the registry, matched against known peripheral brands | Which vendor tool to capture traffic from, and its version |
| `environment` | `std::env::consts`, registry | OS, architecture and Windows build |

Interfaces of one keyboard are grouped by their Windows container ID, so two identical keyboards stay separate.

## Privacy

- **No serial numbers:** hidapi's serial number is never read into the report.
- **No device paths:** paths are used to look devices up and are never written.
- **No instance IDs:** the instance part of every Windows instance ID becomes `<redacted>`, because USB devices with a serial number use it there.
- **No Bluetooth addresses:** 12-digit hexadecimal runs in device IDs become `<redacted>`.
- **Only peripheral software:** installed programs are listed only when their name or publisher matches a peripheral brand.
- **Nothing leaves the computer:** the tool never uploads anything. Users read the file and decide whether to share it.

## Reading a report

- **Report descriptors:** paste a descriptor into `parseReportDescriptor` from `src/native-hid/report-descriptor.ts` to get the same `HIDCollectionInfo` that WebHID gives. A test fake built from it accepts the same report sizes as the real keyboard.
- **Filter drivers:** a vendor filter driver in `lowerFilters` or `upperFilters` usually means the vendor software must be closed while OpenKeyboard talks to the keyboard.
- **Wireless dongles:** a receiver shows up as its own device with its own product ID. Ask for one report on the cable and one on the receiver.

## Development

```
cargo test
cargo clippy --all-targets -- -D warnings
cargo build --release      # target/release/openkeyboard-probe.exe
```

Device lookups through CfgMgr32 and the registry run on Windows only. On Linux and macOS the report holds the hidapi part.

## Verified

On 2026-10-16, on Windows 11 25H2 (build 26200), it reported the Wooting 80HE TenZ (`31e3:1402`, release `0x0250`) with all six collections and their descriptors. These include `0xFF55` with report IDs 1 to 6 and a 7-byte feature report, as described in OpenKeyboard's `docs/wooting.md`. It also listed Wootility 5.4.2 among the installed software.

## Next levels

1. **Safe probes:** send only read commands from an allowlist, and only to interfaces that match a known protocol family.
2. **Traffic capture:** record vendor-tool traffic (USBPcap on Windows) into the same report format.
