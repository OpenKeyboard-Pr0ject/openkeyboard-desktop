use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::Serialize;

use crate::device_selection::DeviceSelection;
use crate::environment::Environment;
use crate::hex::{hex_bytes, hex_u16};
use crate::hid_traffic::TrafficCapture;
use crate::physical_devices::{DiscoveredInterface, PhysicalDevice};
use crate::read_only_queries::QueryRecord;
use crate::report_layout::{DeclaredReport, declared_reports};
use crate::timestamp::utc_timestamp;
use crate::vendor_software::InstalledSoftware;
use crate::windows_devices::WindowsDevice;

pub const SCHEMA_VERSION: u32 = 1;
pub const GENERATOR_NAME: &str = "openkeyboard-probe";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceReport {
    pub schema_version: u32,
    pub generator: Generator,
    pub created_at: String,
    pub selection: String,
    pub read_only_queries_sent: bool,
    pub environment: Environment,
    pub devices: Vec<DeviceEntry>,
    pub vendor_software: Vec<InstalledSoftware>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub traffic_captures: Vec<TrafficCapture>,
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
    pub declared_reports: Vec<DeclaredReport>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub queries: Vec<QueryRecord>,
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
        let descriptor = collection.report_descriptor.as_ref();
        let layout = descriptor.map_err(Clone::clone).and_then(|bytes| declared_reports(bytes));
        Self {
            interface_number: collection.interface_number,
            usage_page: hex_u16(collection.usage_page),
            usage: hex_u16(collection.usage),
            report_descriptor: descriptor.ok().map(|bytes| hex_bytes(bytes)),
            report_descriptor_error: layout.as_ref().err().cloned(),
            declared_reports: layout.unwrap_or_default(),
            queries: interface.queries.clone(),
            windows_devices: interface.windows_devices.clone(),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum CaptureSource {
    #[default]
    None,
    LiveUsbPcap,
    File(PathBuf),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReportRequest {
    pub selection: DeviceSelection,
    pub read_versions: bool,
    pub capture: CaptureSource,
}

pub struct ReportContext {
    pub request: ReportRequest,
    pub environment: Environment,
    pub vendor_software: Vec<InstalledSoftware>,
    pub traffic_captures: Vec<TrafficCapture>,
    pub created_at: SystemTime,
}

pub fn build_device_report(selected_devices: &[PhysicalDevice], context: ReportContext) -> DeviceReport {
    DeviceReport {
        schema_version: SCHEMA_VERSION,
        generator: Generator { name: GENERATOR_NAME.to_string(), version: env!("CARGO_PKG_VERSION").to_string() },
        created_at: utc_timestamp(context.created_at),
        selection: context.request.selection.describe(),
        read_only_queries_sent: context.request.read_versions,
        environment: context.environment,
        devices: selected_devices.iter().map(DeviceEntry::from_physical_device).collect(),
        vendor_software: context.vendor_software,
        traffic_captures: context.traffic_captures,
    }
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
    use crate::physical_devices::group_into_physical_devices;
    use crate::physical_devices::test_support::{collection, interface};
    use crate::read_only_queries::ProtocolFamily;
    use crate::report_layout::ReportKind;

    fn context(read_versions: bool) -> ReportContext {
        ReportContext {
            request: ReportRequest { read_versions, ..ReportRequest::default() },
            environment: Environment {
                operating_system: "windows".to_string(),
                architecture: "x86_64".to_string(),
                windows: None,
            },
            vendor_software: Vec::new(),
            traffic_captures: Vec::new(),
            created_at: UNIX_EPOCH,
        }
    }

    fn wooting() -> Vec<PhysicalDevice> {
        group_into_physical_devices(vec![
            interface(collection(0x31e3, 0x1402, 0x01, 0x06), Some("keyboard")),
            interface(collection(0x31e3, 0x1402, 0xff55, 0x01), Some("keyboard")),
        ])
    }

    #[test]
    fn writes_the_report_descriptor_as_hex() {
        let report = build_device_report(&wooting(), context(false));

        assert_eq!(report.devices[0].interfaces[1].report_descriptor.as_deref(), Some("0655ff0901a101c0"));
        assert_eq!(report.devices[0].interfaces[1].usage_page, "0xff55");
    }

    #[test]
    fn lists_the_declared_reports() {
        let mut config = collection(0x31e3, 0x1402, 0xff55, 0x01);
        config.report_descriptor = Ok(vec![0x85, 0x01, 0x75, 0x08, 0x95, 0x07, 0xb1, 0x02]);
        let devices = group_into_physical_devices(vec![interface(config, None)]);

        let report = build_device_report(&devices, context(false));

        assert_eq!(
            report.devices[0].interfaces[0].declared_reports,
            vec![DeclaredReport { kind: ReportKind::Feature, report_id: 1, byte_length: 7 }]
        );
    }

    #[test]
    fn records_why_a_descriptor_is_missing() {
        let mut unreadable = collection(0x31e3, 0x1402, 0x01, 0x06);
        unreadable.report_descriptor = Err("Could not open the collection: access denied".to_string());
        let devices = group_into_physical_devices(vec![interface(unreadable, None)]);

        let report = build_device_report(&devices, context(false));

        assert_eq!(report.devices[0].interfaces[0].report_descriptor, None);
        assert!(report.devices[0].interfaces[0].report_descriptor_error.as_deref().unwrap().contains("access denied"));
    }

    #[test]
    fn carries_the_query_results() {
        let mut devices = wooting();
        devices[0].interfaces[1].queries =
            vec![QueryRecord::new(ProtocolFamily::Wooting, "Wooting firmware version", &[0xd1, 0xda, 0x01])];

        let report = build_device_report(&devices, context(true));
        let json = serde_json::to_string(&report).unwrap();

        assert!(report.read_only_queries_sent);
        assert!(json.contains("\"queries\":[{\"family\":\"wooting\",\"name\":\"Wooting firmware version\""));
    }

    #[test]
    fn never_writes_device_paths() {
        let report = build_device_report(&wooting(), context(false));
        let json = serde_json::to_string(&report).unwrap();

        assert!(!json.contains("HID#VID"));
        assert!(json.contains("\"schemaVersion\":1"));
        assert!(json.contains("\"readOnlyQueriesSent\":false"));
        assert!(json.contains("\"createdAt\":\"1970-01-01T00:00:00Z\""));
    }
}
