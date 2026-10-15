use std::collections::{BTreeMap, BTreeSet};

use serde::Serialize;

use crate::hex::{hex_u16, trimmed_hex};
use crate::physical_devices::PhysicalDevice;
use crate::usb_descriptors::{UsbDeviceDescription, describe_devices};
use crate::usb_transfers::UsbTransfer;

const CLASS_INTERFACE_HOST_TO_DEVICE: u8 = 0x21;
const CLASS_INTERFACE_DEVICE_TO_HOST: u8 = 0xa1;
const GET_REPORT: u8 = 0x01;
const SET_REPORT: u8 = 0x09;
const INPUT_REPORT_TYPE: u8 = 1;
const OUTPUT_REPORT_TYPE: u8 = 2;
const FEATURE_REPORT_TYPE: u8 = 3;
const DIRECTION_IN_FLAG: u8 = 0x80;
const FIRST_VENDOR_DEFINED_USAGE_PAGE: u16 = 0xff00;
const USBD_STATUS_SUCCESS: u32 = 0;

pub const MAXIMUM_TRAFFIC_ENTRIES: usize = 50_000;
const REPEATED_REQUEST_MINIMUM: usize = 5;
const MAXIMUM_REPEATED_REQUESTS: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum TrafficKind {
    SetFeature,
    GetFeature,
    SetOutput,
    GetOutput,
    SetInput,
    GetInput,
    InterruptOut,
    InterruptIn,
}

impl TrafficKind {
    pub fn name(self) -> &'static str {
        match self {
            Self::SetFeature => "set-feature",
            Self::GetFeature => "get-feature",
            Self::SetOutput => "set-output",
            Self::GetOutput => "get-output",
            Self::SetInput => "set-input",
            Self::GetInput => "get-input",
            Self::InterruptOut => "interrupt-out",
            Self::InterruptIn => "interrupt-in",
        }
    }

    fn carries_device_input(self) -> bool {
        matches!(self, Self::GetInput | Self::InterruptIn)
    }

    fn is_sent_by_the_host(self) -> bool {
        matches!(self, Self::SetFeature | Self::SetOutput | Self::SetInput | Self::InterruptOut)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrafficEntry {
    pub at_micros: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mark: Option<usize>,
    pub kind: TrafficKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interface: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub report_id: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    pub data: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureMark {
    pub label: String,
    pub at_micros: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepeatedRequest {
    pub kind: TrafficKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interface: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub report_id: Option<u8>,
    pub data: String,
    pub count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrafficCapture {
    pub source: String,
    pub vendor_id: String,
    pub product_id: String,
    pub release_number: String,
    pub marks: Vec<CaptureMark>,
    pub entries: Vec<TrafficEntry>,
    pub left_out_input_reports: usize,
    pub entry_limit_reached: bool,
    pub repeated_requests: Vec<RepeatedRequest>,
}

pub type ProductKey = (u16, u16);

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TrafficScope {
    pub vendor_id: u16,
    pub product_id: Option<u16>,
    pub vendor_defined_interfaces: BTreeMap<ProductKey, BTreeSet<u8>>,
}

impl TrafficScope {
    fn includes(&self, description: &UsbDeviceDescription) -> bool {
        description.vendor_id == self.vendor_id
            && self.product_id.is_none_or(|product_id| product_id == description.product_id)
    }

    fn keeps_input_from(&self, description: &UsbDeviceDescription, interface: Option<u8>) -> bool {
        let product = (description.vendor_id, description.product_id);
        interface.is_some_and(|number| {
            self.vendor_defined_interfaces.get(&product).is_some_and(|interfaces| interfaces.contains(&number))
        })
    }
}

pub struct CaptureTimeline<'marks> {
    pub source: &'marks str,
    pub started_micros: u64,
    pub marks: &'marks [CaptureMark],
}

pub fn vendor_defined_interfaces(devices: &[PhysicalDevice]) -> BTreeMap<ProductKey, BTreeSet<u8>> {
    let mut usage_pages: BTreeMap<(ProductKey, u8), Vec<u16>> = BTreeMap::new();
    for device in devices {
        for interface in &device.interfaces {
            let collection = &interface.collection;
            let Ok(number) = u8::try_from(collection.interface_number) else {
                continue;
            };
            let product = (collection.vendor_id, collection.product_id);
            usage_pages.entry((product, number)).or_default().push(collection.usage_page);
        }
    }
    let mut interfaces: BTreeMap<ProductKey, BTreeSet<u8>> = BTreeMap::new();
    for ((product, number), pages) in usage_pages {
        if pages.iter().all(|page| *page >= FIRST_VENDOR_DEFINED_USAGE_PAGE) {
            interfaces.entry(product).or_default().insert(number);
        }
    }
    interfaces
}

pub fn listed_devices(transfers: &[UsbTransfer]) -> Vec<ProductKey> {
    let products: BTreeSet<ProductKey> = describe_devices(transfers)
        .values()
        .map(|description| (description.vendor_id, description.product_id))
        .collect();
    products.into_iter().collect()
}

pub fn extract_traffic_captures(
    transfers: &[UsbTransfer],
    scope: &TrafficScope,
    timeline: &CaptureTimeline,
) -> Vec<TrafficCapture> {
    describe_devices(transfers)
        .into_iter()
        .filter(|(_, description)| scope.includes(description))
        .map(|(address, description)| {
            let device_transfers = transfers.iter().filter(|transfer| transfer.device_address() == address);
            capture_of_device(device_transfers, &description, scope, timeline)
        })
        .collect()
}

fn capture_of_device<'transfer>(
    transfers: impl Iterator<Item = &'transfer UsbTransfer>,
    description: &UsbDeviceDescription,
    scope: &TrafficScope,
    timeline: &CaptureTimeline,
) -> TrafficCapture {
    let mut entries = Vec::new();
    let mut left_out_input_reports = 0;
    let mut entry_limit_reached = false;
    for transfer in transfers {
        let Some(mut entry) = entry_of(transfer, description, timeline.started_micros) else {
            continue;
        };
        if entry.kind.carries_device_input() && !scope.keeps_input_from(description, entry.interface) {
            left_out_input_reports += 1;
            continue;
        }
        if entries.len() == MAXIMUM_TRAFFIC_ENTRIES {
            entry_limit_reached = true;
            break;
        }
        entry.mark = timeline.marks.iter().rposition(|mark| mark.at_micros <= entry.at_micros);
        entries.push(entry);
    }
    TrafficCapture {
        source: timeline.source.to_string(),
        vendor_id: hex_u16(description.vendor_id),
        product_id: hex_u16(description.product_id),
        release_number: hex_u16(description.release_number),
        marks: timeline.marks.to_vec(),
        repeated_requests: repeated_requests(&entries),
        entries,
        left_out_input_reports,
        entry_limit_reached,
    }
}

fn entry_of(transfer: &UsbTransfer, description: &UsbDeviceDescription, started_micros: u64) -> Option<TrafficEntry> {
    let at_micros = transfer.timestamp_micros().saturating_sub(started_micros);
    match transfer {
        UsbTransfer::Control { setup, out_data, in_data, status, .. } => {
            let report_type = (setup.value >> 8) as u8;
            let (kind, data) = match (setup.request_type, setup.request) {
                (CLASS_INTERFACE_HOST_TO_DEVICE, SET_REPORT) => (set_report_kind(report_type)?, out_data),
                (CLASS_INTERFACE_DEVICE_TO_HOST, GET_REPORT) => (get_report_kind(report_type)?, in_data),
                _ => return None,
            };
            Some(TrafficEntry {
                at_micros,
                mark: None,
                kind,
                interface: Some(setup.index as u8),
                report_id: Some(setup.value as u8),
                endpoint: None,
                data: trimmed_hex(data),
                status: match status {
                    Some(USBD_STATUS_SUCCESS) => None,
                    Some(code) => Some(format!("0x{code:08x}")),
                    None => Some("not completed".to_string()),
                },
            })
        }
        UsbTransfer::Interrupt { endpoint, data, status, .. } => Some(TrafficEntry {
            at_micros,
            mark: None,
            kind: if endpoint & DIRECTION_IN_FLAG != 0 { TrafficKind::InterruptIn } else { TrafficKind::InterruptOut },
            interface: description.interface_of_endpoint(*endpoint),
            report_id: None,
            endpoint: Some(format!("0x{endpoint:02x}")),
            data: trimmed_hex(data),
            status: (*status != USBD_STATUS_SUCCESS).then(|| format!("0x{status:08x}")),
        }),
    }
}

fn set_report_kind(report_type: u8) -> Option<TrafficKind> {
    match report_type {
        INPUT_REPORT_TYPE => Some(TrafficKind::SetInput),
        OUTPUT_REPORT_TYPE => Some(TrafficKind::SetOutput),
        FEATURE_REPORT_TYPE => Some(TrafficKind::SetFeature),
        _ => None,
    }
}

fn get_report_kind(report_type: u8) -> Option<TrafficKind> {
    match report_type {
        INPUT_REPORT_TYPE => Some(TrafficKind::GetInput),
        OUTPUT_REPORT_TYPE => Some(TrafficKind::GetOutput),
        FEATURE_REPORT_TYPE => Some(TrafficKind::GetFeature),
        _ => None,
    }
}

type RequestKey = (TrafficKind, Option<u8>, Option<u8>, String);

fn repeated_requests(entries: &[TrafficEntry]) -> Vec<RepeatedRequest> {
    let mut counts: BTreeMap<RequestKey, usize> = BTreeMap::new();
    for entry in entries.iter().filter(|entry| entry.kind.is_sent_by_the_host()) {
        *counts.entry((entry.kind, entry.interface, entry.report_id, entry.data.clone())).or_default() += 1;
    }
    let mut repeated: Vec<RepeatedRequest> = counts
        .into_iter()
        .filter(|(_, count)| *count >= REPEATED_REQUEST_MINIMUM)
        .map(|((kind, interface, report_id, data), count)| RepeatedRequest { kind, interface, report_id, data, count })
        .collect();
    repeated.sort_by_key(|request| std::cmp::Reverse(request.count));
    repeated.truncate(MAXIMUM_REPEATED_REQUESTS);
    repeated
}

#[cfg(test)]
pub mod test_support {
    use super::*;
    use crate::usb_descriptors::test_support::{configuration_descriptor, device_descriptor};
    use crate::usb_transfers::test_support::{UsbPcapRecord, control_records, interrupt_record, setup_bytes};

    pub const STARTED_MICROS: u64 = 1_792_108_800_000_000;
    pub const WOOTING_ADDRESS: u16 = 7;
    pub const MOUSE_ADDRESS: u16 = 9;

    pub fn descriptor_records(address: u16, vendor_id: u16, product_id: u16, irp_base: u64) -> Vec<UsbPcapRecord> {
        let configuration = configuration_descriptor(&[(1, &[0x82]), (2, &[0x83, 0x03]), (4, &[0x85])]);
        let mut records = control_records(
            STARTED_MICROS + 10,
            irp_base,
            address,
            setup_bytes(0x80, 0x06, 0x0100, 0, 18),
            &[],
            &device_descriptor(vendor_id, product_id, 0x0250),
        );
        records.extend(control_records(
            STARTED_MICROS + 20,
            irp_base + 1,
            address,
            setup_bytes(0x80, 0x06, 0x0200, 0, configuration.len() as u16),
            &[],
            &configuration,
        ));
        records
    }

    pub fn wooting_session() -> Vec<UsbPcapRecord> {
        let mut records = descriptor_records(WOOTING_ADDRESS, 0x31e3, 0x1402, 100);
        records.extend(descriptor_records(MOUSE_ADDRESS, 0x046d, 0xc52b, 200));
        records.extend(control_records(
            STARTED_MICROS + 1_000_000,
            300,
            WOOTING_ADDRESS,
            setup_bytes(0x21, 0x09, 0x0301, 2, 7),
            &[0xd1, 0xda, 0x27, 0x01, 0x00, 0x00, 0x00],
            &[],
        ));
        records.push(interrupt_record(
            STARTED_MICROS + 1_001_000,
            301,
            WOOTING_ADDRESS,
            0x83,
            &[0x01, 0xd1, 0xda, 0x27],
        ));
        records.push(interrupt_record(STARTED_MICROS + 1_002_000, 302, WOOTING_ADDRESS, 0x82, &[0x00, 0x00, 0x04]));
        records.push(interrupt_record(
            STARTED_MICROS + 3_000_000,
            303,
            WOOTING_ADDRESS,
            0x03,
            &[0x03, 0xd1, 0xda, 0x11],
        ));
        records.push(interrupt_record(STARTED_MICROS + 3_001_000, 304, MOUSE_ADDRESS, 0x83, &[0x01, 0x02]));
        records
    }

    pub fn wooting_scope() -> TrafficScope {
        TrafficScope {
            vendor_id: 0x31e3,
            product_id: None,
            vendor_defined_interfaces: BTreeMap::from([((0x31e3, 0x1402), BTreeSet::from([2, 4]))]),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::*;
    use super::*;
    use crate::physical_devices::group_into_physical_devices;
    use crate::physical_devices::test_support::{collection, interface};
    use crate::usb_transfers::assemble_transfers;
    use crate::usb_transfers::test_support::parsed;

    fn captures(marks: &[CaptureMark]) -> Vec<TrafficCapture> {
        let transfers = assemble_transfers(&parsed(&wooting_session()));
        let timeline = CaptureTimeline { source: "capture-file", started_micros: STARTED_MICROS, marks };
        extract_traffic_captures(&transfers, &wooting_scope(), &timeline)
    }

    #[test]
    fn keeps_only_the_selected_vendor() {
        let captures = captures(&[]);

        assert_eq!(captures.len(), 1);
        assert_eq!((captures[0].vendor_id.as_str(), captures[0].product_id.as_str()), ("0x31e3", "0x1402"));
    }

    #[test]
    fn decodes_a_set_feature_request() {
        let entry = &captures(&[])[0].entries[0];

        assert_eq!(entry.kind, TrafficKind::SetFeature);
        assert_eq!((entry.interface, entry.report_id), (Some(2), Some(1)));
        assert_eq!(entry.data, "d1da2701");
        assert_eq!(entry.at_micros, 1_000_000);
        assert_eq!(entry.status, None);
    }

    #[test]
    fn keeps_input_reports_of_vendor_defined_interfaces_only() {
        let capture = &captures(&[])[0];
        let kinds: Vec<_> = capture.entries.iter().map(|entry| (entry.kind, entry.interface)).collect();

        assert_eq!(
            kinds,
            vec![
                (TrafficKind::SetFeature, Some(2)),
                (TrafficKind::InterruptIn, Some(2)),
                (TrafficKind::InterruptOut, Some(2)),
            ]
        );
        assert_eq!(capture.left_out_input_reports, 1);
    }

    #[test]
    fn leaves_out_every_input_report_when_no_interface_is_known() {
        let transfers = assemble_transfers(&parsed(&wooting_session()));
        let scope = TrafficScope { vendor_defined_interfaces: BTreeMap::new(), ..wooting_scope() };
        let timeline = CaptureTimeline { source: "capture-file", started_micros: STARTED_MICROS, marks: &[] };

        let capture = &extract_traffic_captures(&transfers, &scope, &timeline)[0];

        assert_eq!(capture.left_out_input_reports, 2);
        assert!(capture.entries.iter().all(|entry| entry.kind != TrafficKind::InterruptIn));
    }

    #[test]
    fn links_each_entry_to_the_mark_before_it() {
        let marks = [
            CaptureMark { label: "open Wootility".to_string(), at_micros: 500_000 },
            CaptureMark { label: "actuation 1.5 mm".to_string(), at_micros: 2_000_000 },
        ];

        let capture = &captures(&marks)[0];

        let marks_of_entries: Vec<_> = capture.entries.iter().map(|entry| entry.mark).collect();
        assert_eq!(marks_of_entries, vec![Some(0), Some(0), Some(1)]);
    }

    #[test]
    fn finds_requests_that_repeat() {
        let entry = |data: &str| TrafficEntry {
            at_micros: 0,
            mark: None,
            kind: TrafficKind::SetFeature,
            interface: Some(2),
            report_id: Some(1),
            endpoint: None,
            data: data.to_string(),
            status: None,
        };
        let mut entries: Vec<_> = (0..6).map(|_| entry("d1da4a")).collect();
        entries.push(entry("d1da27"));

        let repeated = repeated_requests(&entries);

        assert_eq!(repeated.len(), 1);
        assert_eq!((repeated[0].data.as_str(), repeated[0].count), ("d1da4a", 6));
    }

    #[test]
    fn treats_an_interface_as_vendor_defined_only_when_every_collection_is() {
        let mut consumer = collection(0x31e3, 0x1402, 0x0c, 0x01);
        consumer.interface_number = 3;
        let mut system = collection(0x31e3, 0x1402, 0x01, 0x80);
        system.interface_number = 3;
        let mut config = collection(0x31e3, 0x1402, 0xff55, 0x01);
        config.interface_number = 2;
        let devices = group_into_physical_devices(vec![
            interface(consumer, None),
            interface(system, None),
            interface(config, None),
        ]);

        let interfaces = vendor_defined_interfaces(&devices);

        assert_eq!(interfaces, BTreeMap::from([((0x31e3, 0x1402), BTreeSet::from([2]))]));
    }
}
