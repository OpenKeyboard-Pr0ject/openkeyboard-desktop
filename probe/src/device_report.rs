use std::collections::HashMap;
use std::path::Path;
use std::time::SystemTime;

use serde::Serialize;

use crate::device_selection::DeviceSelection;
use crate::environment::{Environment, current_environment};
use crate::hex::{hex_bytes, hex_u16};
use crate::hid_inventory::enumerate_hid_collections;
use crate::physical_devices::{DiscoveredInterface, PhysicalDevice, group_into_physical_devices};
use crate::timestamp::utc_timestamp;
use crate::vendor_software::{InstalledSoftware, installed_peripheral_software};
use crate::windows_devices::{WindowsDevice, WindowsDeviceLookup, look_up_windows_devices};

pub const SCHEMA_VERSION: u32 = 1;
pub const GENERATOR_NAME: &str = "openkeyboard-probe";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceReport {
    pub schema_version: u32,
    pub generator: Generator,
    pub created_at: String,
    pub selection: String,
    pub environment: Environment,
    pub devices: Vec<DeviceEntry>,
    pub vendor_software: Vec<InstalledSoftware>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Generator {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceEntry {
    pub vendor_id: String,
    pub product_id: String,
    pub release_number: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manufacturer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub product: Option<String>,
    pub bus: String,
    pub interfaces: Vec<InterfaceEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InterfaceEntry {
    pub interface_number: i32,
    pub usage_page: String,
    pub usage: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub report_descriptor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub report_descriptor_error: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub windows_devices: Vec<WindowsDevice>,
}

impl DeviceEntry {
    fn from_physical_device(device: &PhysicalDevice) -> Self {
        let identity = device.identity();
        Self {
            vendor_id: hex_u16(identity.vendor_id),
            product_id: hex_u16(identity.product_id),
            release_number: hex_u16(identity.release_number),
            manufacturer: identity.manufacturer.clone(),
            product: identity.product.clone(),
            bus: identity.bus.clone(),
            interfaces: device.interfaces.iter().map(InterfaceEntry::from_discovered_interface).collect(),
        }
    }
}

impl InterfaceEntry {
    fn from_discovered_interface(interface: &DiscoveredInterface) -> Self {
        let collection = &interface.collection;
        Self {
            interface_number: collection.interface_number,
            usage_page: hex_u16(collection.usage_page),
            usage: hex_u16(collection.usage),
            report_descriptor: collection.report_descriptor.as_ref().ok().map(|descriptor| hex_bytes(descriptor)),
            report_descriptor_error: collection.report_descriptor.as_ref().err().cloned(),
            windows_devices: interface.windows_devices.clone(),
        }
    }
}

pub struct ReportContext {
    pub selection: DeviceSelection,
    pub environment: Environment,
    pub vendor_software: Vec<InstalledSoftware>,
    pub created_at: SystemTime,
}

pub fn build_device_report(devices: &[PhysicalDevice], context: ReportContext) -> DeviceReport {
    DeviceReport {
        schema_version: SCHEMA_VERSION,
        generator: Generator { name: GENERATOR_NAME.to_string(), version: env!("CARGO_PKG_VERSION").to_string() },
        created_at: utc_timestamp(context.created_at),
        selection: context.selection.describe(),
        environment: context.environment,
        devices: devices
            .iter()
            .filter(|device| context.selection.selects(device))
            .map(DeviceEntry::from_physical_device)
            .collect(),
        vendor_software: context.vendor_software,
    }
}

pub fn collect_device_report(selection: DeviceSelection) -> Result<DeviceReport, String> {
    let mut lookups_by_path: HashMap<String, WindowsDeviceLookup> = HashMap::new();
    let interfaces = enumerate_hid_collections()?
        .into_iter()
        .map(|collection| {
            let lookup = lookups_by_path
                .entry(collection.path.clone())
                .or_insert_with(|| look_up_windows_devices(&collection.path));
            DiscoveredInterface {
                windows_devices: lookup.chain.clone(),
                container_key: lookup.container_key.clone(),
                collection,
            }
        })
        .collect();
    let context = ReportContext {
        selection,
        environment: current_environment(),
        vendor_software: installed_peripheral_software(),
        created_at: SystemTime::now(),
    };
    Ok(build_device_report(&group_into_physical_devices(interfaces), context))
}

pub fn write_device_report(report: &DeviceReport, destination: &Path) -> Result<(), String> {
    let json = serde_json::to_string_pretty(report).map_err(|error| format!("Could not format the report: {error}"))?;
    std::fs::write(destination, format!("{json}\n"))
        .map_err(|error| format!("Could not write {}: {error}", destination.display()))
}

#[cfg(test)]
mod tests {
    use std::time::UNIX_EPOCH;

    use super::*;
    use crate::physical_devices::test_support::{collection, interface};

    fn context(selection: DeviceSelection) -> ReportContext {
        ReportContext {
            selection,
            environment: Environment {
                operating_system: "windows".to_string(),
                architecture: "x86_64".to_string(),
                windows: None,
            },
            vendor_software: Vec::new(),
            created_at: UNIX_EPOCH,
        }
    }

    fn wooting_with_mouse() -> Vec<PhysicalDevice> {
        group_into_physical_devices(vec![
            interface(collection(0x31e3, 0x1402, 0x01, 0x06), Some("keyboard")),
            interface(collection(0x31e3, 0x1402, 0xff55, 0x01), Some("keyboard")),
            interface(collection(0x046d, 0xc52b, 0x01, 0x02), Some("mouse")),
        ])
    }

    #[test]
    fn keeps_only_selected_devices() {
        let report = build_device_report(&wooting_with_mouse(), context(DeviceSelection::default()));

        assert_eq!(report.devices.len(), 1);
        assert_eq!(report.devices[0].vendor_id, "0x31e3");
        assert_eq!(report.devices[0].interfaces.len(), 2);
    }

    #[test]
    fn writes_the_report_descriptor_as_hex() {
        let report = build_device_report(&wooting_with_mouse(), context(DeviceSelection::default()));

        assert_eq!(report.devices[0].interfaces[1].report_descriptor.as_deref(), Some("0655ff0901a101c0"));
        assert_eq!(report.devices[0].interfaces[1].usage_page, "0xff55");
    }

    #[test]
    fn records_why_a_descriptor_is_missing() {
        let mut unreadable = collection(0x31e3, 0x1402, 0x01, 0x06);
        unreadable.report_descriptor = Err("Could not open the collection: access denied".to_string());
        let devices = group_into_physical_devices(vec![interface(unreadable, None)]);

        let report = build_device_report(&devices, context(DeviceSelection::default()));

        assert_eq!(report.devices[0].interfaces[0].report_descriptor, None);
        assert!(report.devices[0].interfaces[0].report_descriptor_error.as_deref().unwrap().contains("access denied"));
    }

    #[test]
    fn never_writes_device_paths() {
        let report = build_device_report(&wooting_with_mouse(), context(DeviceSelection::default()));
        let json = serde_json::to_string(&report).unwrap();

        assert!(!json.contains("HID#VID"));
        assert!(json.contains("\"schemaVersion\":1"));
        assert!(json.contains("\"createdAt\":\"1970-01-01T00:00:00Z\""));
    }
}
