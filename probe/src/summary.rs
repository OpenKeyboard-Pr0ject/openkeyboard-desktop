use std::collections::BTreeMap;
use std::fmt::Write;
use std::path::Path;

use crate::device_report::{DeviceEntry, DeviceReport, InterfaceEntry};
use crate::hid_traffic::{TrafficCapture, TrafficKind};
use crate::read_only_queries::QueryRecord;

const HEX_DIGITS_PER_BYTE: usize = 2;

pub fn describe_report(report: &DeviceReport, saved_to: &Path) -> String {
    let mut text = String::from("OpenKeyboard device report\n\n");
    let _ = writeln!(text, "Looked for {}.", report.selection);
    let traffic = if report.read_only_queries_sent {
        "Only read commands from a fixed list were sent."
    } else {
        "No commands were sent to any device."
    };
    let _ = writeln!(text, "Found {}. {traffic}", counted(report.devices.len(), "device"));
    for device in &report.devices {
        text.push('\n');
        text.push_str(&describe_device(device));
    }
    for capture in &report.traffic_captures {
        text.push('\n');
        text.push_str(&describe_capture(capture));
    }
    if !report.vendor_software.is_empty() {
        text.push_str("\nVendor software\n");
        for software in &report.vendor_software {
            let version = software
                .version
                .as_deref()
                .filter(|version| !software.name.contains(version))
                .map(|version| format!(" {version}"))
                .unwrap_or_default();
            let publisher =
                software.publisher.as_deref().map(|publisher| format!(" ({publisher})")).unwrap_or_default();
            let _ = writeln!(text, "  {}{version}{publisher}", software.name);
        }
    }
    let _ = write!(
        text,
        "\nSaved to {}\nThe report contains no serial numbers, device paths or Bluetooth addresses.\n\
         Read it before you share it.",
        saved_to.display()
    );
    text
}

fn counted(count: usize, noun: &str) -> String {
    if count == 1 { format!("1 {noun}") } else { format!("{count} {noun}s") }
}

fn describe_device(device: &DeviceEntry) -> String {
    let name = device.product.as_deref().unwrap_or("Unnamed device");
    let manufacturer = device.manufacturer.as_deref().map(|maker| format!(" ({maker})")).unwrap_or_default();
    let mut text = format!("{name}{manufacturer}\n");
    let vendor_id = device.vendor_id.trim_start_matches("0x");
    let product_id = device.product_id.trim_start_matches("0x");
    let _ = writeln!(text, "  ids {vendor_id}:{product_id}, release {}, {}", device.release_number, device.bus);
    for interface in &device.interfaces {
        let _ = writeln!(text, "  {}", describe_interface(interface));
        for query in &interface.queries {
            let _ = writeln!(text, "      {}", describe_query(query));
        }
    }
    text
}

fn describe_capture(capture: &TrafficCapture) -> String {
    let mut text = format!("USB traffic of {}:{} ({})\n", capture.vendor_id, capture.product_id, capture.source);
    let mut counts: BTreeMap<TrafficKind, usize> = BTreeMap::new();
    for entry in &capture.entries {
        *counts.entry(entry.kind).or_default() += 1;
    }
    let breakdown: Vec<String> = counts.iter().map(|(kind, count)| format!("{count} {}", kind.name())).collect();
    let _ = writeln!(text, "  {}: {}", counted(capture.entries.len(), "report"), breakdown.join(", "));
    if capture.left_out_input_reports > 0 {
        let left_out = counted(capture.left_out_input_reports, "input report");
        let _ = writeln!(text, "  Left out {left_out} from keyboard, mouse or media interfaces.");
    }
    if capture.entry_limit_reached {
        let _ = writeln!(text, "  The capture was cut off at {}.", counted(capture.entries.len(), "report"));
    }
    if !capture.marks.is_empty() {
        text.push_str("  Changes\n");
        for (index, mark) in capture.marks.iter().enumerate() {
            let count = capture.entries.iter().filter(|entry| entry.mark == Some(index)).count();
            let _ = writeln!(text, "    {}. {}: {}", index + 1, mark.label, counted(count, "report"));
        }
    }
    if !capture.repeated_requests.is_empty() {
        text.push_str("  Repeated requests, probably polling\n");
        for request in &capture.repeated_requests {
            let interface = request.interface.map(|number| format!(" interface {number}")).unwrap_or_default();
            let report_id = request.report_id.map(|id| format!(" report {id}")).unwrap_or_default();
            let kind = request.kind.name();
            let _ = writeln!(text, "    {kind}{interface}{report_id} {} x{}", request.data, request.count);
        }
    }
    text
}

fn describe_query(query: &QueryRecord) -> String {
    match (&query.result, &query.error) {
        (Some(result), _) => format!("{}: {result}", query.name),
        (None, Some(error)) => format!("{}: {error}", query.name),
        (None, None) => format!("{}: no result", query.name),
    }
}

fn describe_interface(interface: &InterfaceEntry) -> String {
    let number = if interface.interface_number < 0 { "-".to_string() } else { interface.interface_number.to_string() };
    let descriptor = match (&interface.report_descriptor, &interface.report_descriptor_error) {
        (Some(hex), _) => format!("descriptor {} bytes", hex.len() / HEX_DIGITS_PER_BYTE),
        (None, Some(_)) => "descriptor unreadable".to_string(),
        (None, None) => "no descriptor".to_string(),
    };
    let mut parts =
        vec![format!("interface {number:<2}"), format!("usage {}:{}", interface.usage_page, interface.usage)];
    parts.push(descriptor);
    if let Some(service) = interface.windows_devices.iter().find_map(|device| device.service.as_deref()) {
        parts.push(format!("driver {service}"));
    }
    let filters = driver_filters(interface);
    if !filters.is_empty() {
        parts.push(format!("filters {}", filters.join(", ")));
    }
    parts.join("   ")
}

fn driver_filters(interface: &InterfaceEntry) -> Vec<String> {
    let mut filters: Vec<String> = Vec::new();
    for device in &interface.windows_devices {
        for filter in device.lower_filters.iter().chain(&device.upper_filters) {
            if !filters.contains(filter) {
                filters.push(filter.clone());
            }
        }
    }
    filters
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::device_report::Generator;
    use crate::environment::Environment;
    use crate::hid_traffic::{CaptureMark, RepeatedRequest, TrafficEntry};
    use crate::read_only_queries::ProtocolFamily;
    use crate::vendor_software::InstalledSoftware;
    use crate::windows_devices::WindowsDevice;

    fn razer_report() -> DeviceReport {
        DeviceReport {
            schema_version: 1,
            generator: Generator { name: "openkeyboard-probe".to_string(), version: "0.1.0".to_string() },
            created_at: "2026-10-16T00:00:00Z".to_string(),
            selection: "devices with a keyboard collection".to_string(),
            read_only_queries_sent: false,
            environment: Environment {
                operating_system: "windows".to_string(),
                architecture: "x86_64".to_string(),
                windows: None,
            },
            devices: vec![DeviceEntry {
                vendor_id: "0x1532".to_string(),
                product_id: "0x0258".to_string(),
                release_number: "0x0200".to_string(),
                manufacturer: Some("Razer".to_string()),
                product: Some("Razer BlackWidow V3 Mini".to_string()),
                bus: "usb".to_string(),
                interfaces: vec![InterfaceEntry {
                    interface_number: 3,
                    usage_page: "0x000c".to_string(),
                    usage: "0x0001".to_string(),
                    report_descriptor: Some("050c0901a101755a95b103c0".to_string()),
                    report_descriptor_error: None,
                    declared_reports: Vec::new(),
                    queries: Vec::new(),
                    windows_devices: vec![
                        WindowsDevice {
                            enumerator: "HID".to_string(),
                            service: Some("HidUsb".to_string()),
                            upper_filters: vec!["RzDev_0258".to_string()],
                            ..WindowsDevice::default()
                        },
                        WindowsDevice {
                            enumerator: "USB".to_string(),
                            lower_filters: vec!["RzDev_0258".to_string()],
                            ..WindowsDevice::default()
                        },
                    ],
                }],
            }],
            traffic_captures: Vec::new(),
            vendor_software: vec![InstalledSoftware {
                name: "Razer Synapse".to_string(),
                version: Some("4.0.1".to_string()),
                publisher: Some("Razer Inc.".to_string()),
            }],
        }
    }

    #[test]
    fn lists_each_interface_with_its_driver() {
        let text = describe_report(&razer_report(), Path::new("report.json"));

        assert!(text.contains("Razer BlackWidow V3 Mini (Razer)"));
        assert!(text.contains("ids 1532:0258, release 0x0200, usb"));
        assert!(text.contains("usage 0x000c:0x0001   descriptor 12 bytes   driver HidUsb   filters RzDev_0258"));
    }

    #[test]
    fn lists_vendor_software() {
        let text = describe_report(&razer_report(), Path::new("report.json"));

        assert!(text.contains("Razer Synapse 4.0.1 (Razer Inc.)"));
    }

    #[test]
    fn lists_query_results_under_their_interface() {
        let mut report = razer_report();
        report.read_only_queries_sent = true;
        report.devices[0].interfaces[0].queries = vec![
            QueryRecord::new(ProtocolFamily::Razer, "Razer firmware version, transaction id 0x1f", &[0x1f])
                .failed("The device did not answer this transaction id.".to_string()),
            QueryRecord::new(ProtocolFamily::Razer, "Razer firmware version, transaction id 0x3f", &[0x3f])
                .answered(&[0x02, 0x3f], Ok("2.3".to_string())),
        ];

        let text = describe_report(&report, Path::new("report.json"));

        assert!(text.contains("Found 1 device. Only read commands from a fixed list were sent."));
        assert!(text.contains(
            "      Razer firmware version, transaction id 0x1f: The device did not answer this transaction id.
"
        ));
        assert!(text.contains(
            "      Razer firmware version, transaction id 0x3f: 2.3
"
        ));
    }

    #[test]
    fn describes_a_traffic_capture() {
        let entry = |mark: Option<usize>, kind: TrafficKind| TrafficEntry {
            at_micros: 0,
            mark,
            kind,
            interface: Some(3),
            report_id: Some(0),
            endpoint: None,
            data: "001f".to_string(),
            status: None,
        };
        let mut report = razer_report();
        report.traffic_captures = vec![TrafficCapture {
            source: "usbpcap".to_string(),
            vendor_id: "0x1532".to_string(),
            product_id: "0x0258".to_string(),
            release_number: "0x0200".to_string(),
            marks: vec![CaptureMark { label: "brightness 60 to 40".to_string(), at_micros: 0 }],
            entries: vec![
                entry(None, TrafficKind::SetFeature),
                entry(Some(0), TrafficKind::SetFeature),
                entry(Some(0), TrafficKind::GetFeature),
            ],
            left_out_input_reports: 4,
            entry_limit_reached: false,
            repeated_requests: vec![RepeatedRequest {
                kind: TrafficKind::SetFeature,
                interface: Some(3),
                report_id: Some(0),
                data: "001f".to_string(),
                count: 9,
            }],
        }];

        let text = describe_report(&report, Path::new("report.json"));

        assert!(text.contains("USB traffic of 0x1532:0x0258 (usbpcap)\n  3 reports: 2 set-feature, 1 get-feature\n"));
        assert!(text.contains("  Left out 4 input reports from keyboard, mouse or media interfaces.\n"));
        assert!(text.contains("    1. brightness 60 to 40: 2 reports\n"));
        assert!(text.contains("    set-feature interface 3 report 0 001f x9\n"));
    }

    #[test]
    fn writes_a_version_only_once() {
        let mut report = razer_report();
        report.vendor_software[0].name = "Wootility 5.4.2".to_string();
        report.vendor_software[0].version = Some("5.4.2".to_string());

        let text = describe_report(&report, Path::new("report.json"));

        assert!(text.contains("  Wootility 5.4.2 (Razer Inc.)"));
    }

    #[test]
    fn says_where_the_report_was_saved() {
        let text = describe_report(&razer_report(), Path::new("report.json"));

        assert!(text.contains("Found 1 device. No commands were sent to any device."));
        assert!(text.contains("Saved to report.json"));
    }
}
