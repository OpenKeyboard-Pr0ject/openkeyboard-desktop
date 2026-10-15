# openkeyboard-probe

A small command-line tool that writes a report about the keyboards on a computer. People who own a keyboard that OpenKeyboard does not support yet can run it and send the report, so support can start without the keyboard on the developer's desk.

By default it only lists devices and reads what the operating system already knows about them. **It sends no commands to any device** unless `--read-versions` is given.

## Running

Double-click `openkeyboard-probe.exe`, or run it from a terminal:

```
openkeyboard-probe                          # devices with a keyboard collection
openkeyboard-probe --read-versions          # also ask known protocol families for their versions
openkeyboard-probe --vendor-id 1532         # only one vendor, even without a keyboard collection
openkeyboard-probe --vendor-id 1532 --product-id 0258
openkeyboard-probe --all                    # every HID device
openkeyboard-probe --output razer.json
openkeyboard-probe --vendor-id 1532 --capture                        # record Synapse talking to the keyboard
openkeyboard-probe --vendor-id 1532 --import-capture synapse.pcapng  # read a capture made in Wireshark
```

## Read-only queries

`--read-versions` asks interfaces of a known protocol family for their versions. The commands come from a fixed list: each family has an enum of allowed queries, and there is no way to send other bytes.

| Family | Interface it is sent to | Queries |
|---|---|---|
| VIA | Usage `0xFF60:0x61` with a 32-byte output report, any vendor | Protocol version `01`, firmware version `02 04` |
| Vial | Same interface, after VIA answered | Keyboard id `FE 00` |
| Wooting | Vendor `0x31E3`, usage page `0xFF55` with feature report 1 | `GET_VERSION` `D1 DA 01` |
| Razer | Vendor `0x1532`, a 90-byte feature report without report id | Firmware `00:81`, with transaction ids `1F`, `3F`, `9F`, `FF` in turn until one is answered |

- **Already used elsewhere:** every query is one that OpenKeyboard, Wootility or OpenRazer already sends.
- **Interfaces are recognized by their reports:** a family is chosen from the parsed descriptor, so a lookalike collection without the expected report is left alone.
- **One answer per device:** once one interface of a family answers, the other interfaces of the same device are skipped.
- **Replies for other apps are ignored:** Wooting and VIA replies are matched by command, so traffic from a vendor tool that is open at the same time is skipped.
- **No serial numbers:** the commands that read serial numbers (Wooting `0x03`, Razer `00:82`) are not on the list.

It prints a summary and writes `openkeyboard-device-report.json` to the current folder. When it is started by double-clicking, it waits for Enter before the window closes.

## Traffic captures

Write formats are learned by watching the vendor tool make one change at a time. The probe turns a USBPcap capture into a list of HID reports for one keyboard.

**Recording with `--capture`:**

1. **Install USBPcap:** install it from [desowin.org/usbpcap](https://desowin.org/usbpcap/), or tick it in the Wireshark installer, then restart Windows.
2. **Start the probe as administrator:** open a terminal with **Run as administrator**, start the vendor tool, then run `openkeyboard-probe --vendor-id <vendor> --capture`.
3. **Describe each change before making it:** type what you are about to change (for example `brightness 60 to 40`), press Enter, make the change in the vendor tool, and wait a few seconds. Press Enter on an empty line to stop.
4. **Stay off the keyboard:** use the mouse in the vendor tool and do not type text on the keyboard while recording.

The probe runs `USBPcapCMD` on every root hub with `-A --inject-descriptors`, so devices that were plugged in before the capture are described in it. Each change becomes a mark, and every report records the mark that came before it.

**Importing with `--import-capture`:** a `.pcap` or `.pcapng` file recorded with USBPcap, for example from Wireshark's USBPcap interface. Keep **Inject already connected devices descriptors** turned on, or plug the keyboard in after starting the capture. Without the device descriptor, the probe cannot tell which USB address is the keyboard.

**What is kept:**

| Traffic | Kept |
|---|---|
| `SET_REPORT` and `GET_REPORT` for feature and output reports | Always |
| Interrupt OUT (host to device) | Always |
| Interrupt IN and `GET_REPORT` for input reports | Only on interfaces where every HID collection is vendor-defined (usage page `0xFF00` or higher), known from the keyboard that is plugged in now |
| Traffic of other devices | Never. `--vendor-id` is required, and only devices whose descriptors match it are read |

Input reports on keyboard, mouse and media interfaces carry keystrokes, so they are counted in `leftOutInputReports` and dropped. If the keyboard is not plugged in during `--import-capture`, every input report is dropped.

`repeatedRequests` lists host requests that appear at least 5 times. These are usually the vendor tool polling, and they are not part of a change.

## What the report contains

| Field | Source | Why it helps |
|---|---|---|
| `devices[]` ids, release number, bus, product strings | hidapi enumeration | Identifies the model and firmware revision (`releaseNumber` is the USB `bcdDevice`) |
| `interfaces[].usagePage`, `usage`, `interfaceNumber` | hidapi | Shows which protocol family the keyboard speaks: `0xFF60:0x61` is VIA, `0xFF55` is Wooting, a 90-byte feature report on `0x0C:0x01` is Razer |
| `interfaces[].reportDescriptor` | hidapi, rebuilt from the Windows preparsed data | Every report ID and size, including the keyboard and consumer collections that WebHID hides |
| `interfaces[].declaredReports` | Parsed from the descriptor | Kind, report ID and byte length of each report, with constant fields counted as WebHID's parser in the desktop app does |
| `interfaces[].queries` | `--read-versions` only | Name, request, reply and decoded result or error of each read-only query. Trailing zero bytes are left out of `request` and `reply` |
| `readOnlyQueriesSent` | Command line | Whether any query was allowed to run |
| `trafficCaptures[]` | `--capture` or `--import-capture` | Per USB device of the named vendor: `marks`, `entries` (time in microseconds from the start, kind, interface, report ID or endpoint, data without trailing zero bytes, USBD status when not 0), `leftOutInputReports` and `repeatedRequests` |
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

With `--read-versions`, on the same day:

- **Wooting 80HE TenZ:** answered `d1da01880300020e01`, firmware 2.14.1.
- **Razer Viper V4 Pro (`1532:00e6`):** a mouse, included because it has a keyboard collection. The control report is on MI_03 (`0x0C:0x01`). It answered transaction id `1F` with firmware 1.2 on the first try. The `0x0C:0x01` collection on MI_01 has no 90-byte report and was left alone.

Traffic import, on the same day:

- **Format check:** `cargo test writes_a_sample_capture -- --ignored` writes the test session to `target/usbpcap-sample.pcapng`. Wireshark 4.6.6 (`tshark`) decodes its device and configuration descriptors and reads frame 9 as a HID `SET_REPORT` for feature report 1. That confirms the test packets follow the USBPcap format.
- **End to end:** importing that file with `--vendor-id 31e3` while the 80HE was plugged in kept the feature report and the interface 2 reports, and dropped the input report on interface 1.
- **Live recording:** not tried yet, because USBPcap was not installed. Without it, `--capture` stops with installation steps.

## Next steps

- **Live recording on hardware:** install USBPcap and record one known change, such as a Wootility brightness change, to confirm the live path.
- **Linux captures:** read `usbmon` captures (link type 220) as well.
