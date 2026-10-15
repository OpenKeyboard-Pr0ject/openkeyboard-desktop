use std::fmt::Write;
use std::path::Path;

use crate::device_report::{DeviceEntry, DeviceReport, InterfaceEntry};

const HEX_DIGITS_PER_BYTE: usize = 2;

pub fn describe_report(report: &DeviceReport, saved_to: &Path) -> String {
    let mut text = String::from("OpenKeyboard device report\n\n");
    let _ = writeln!(text, "Looked for {}.", report.selection);
    let _ = writeln!(text, "Found {}. No commands were sent to any device.", device_count(report.devices.len()));
    for device in &report.devices {
        text.push('\n');
        text.push_str(&describe_device(device));
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

fn device_count(count: usize) -> String {
    if count == 1 { "1 device".to_string() } else { format!("{count} devices") }
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
    }
    text
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
    use crate::vendor_software::InstalledSoftware;
    use crate::windows_devices::WindowsDevice;

    fn razer_report() -> DeviceReport {
        DeviceReport {
            schema_version: 1,
            generator: Generator { name: "openkeyboard-probe".to_string(), version: "0.1.0".to_string() },
            created_at: "2026-10-16T00:00:00Z".to_string(),
            selection: "devices with a keyboard collection".to_string(),
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
