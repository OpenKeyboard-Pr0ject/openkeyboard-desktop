use std::time::Duration;

use serde::Serialize;

use crate::hex::trimmed_hex;
use crate::hid_inventory::HidCollectionRecord;
use crate::hid_transport::HidTransport;
use crate::physical_devices::PhysicalDevice;
use crate::razer_queries::{self, RazerPolling};
use crate::report_layout::{DeclaredReport, ReportKind, declared_reports, uses_numbered_input_reports};
use crate::{via_queries, wooting_queries};

const VIA_USAGE_PAGE: u16 = 0xff60;
const VIA_USAGE: u16 = 0x61;
const VIA_REPORT_BYTES: usize = 32;
const WOOTING_VENDOR_ID: u16 = 0x31e3;
const WOOTING_CONFIG_USAGE_PAGE: u16 = 0xff55;
const WOOTING_COMMAND_REPORT_ID: u8 = 1;
const RAZER_VENDOR_ID: u16 = 0x1532;
const RAZER_REPORT_BYTES: usize = 90;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolFamily {
    Via,
    Wooting,
    Razer,
}

impl ProtocolFamily {
    pub fn name(self) -> &'static str {
        match self {
            Self::Via => "via",
            Self::Wooting => "wooting",
            Self::Razer => "razer",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueryRecord {
    pub family: String,
    pub name: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub request: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reply: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl QueryRecord {
    pub fn new(family: ProtocolFamily, name: &str, request: &[u8]) -> Self {
        Self {
            family: family.name().to_string(),
            name: name.to_string(),
            request: trimmed_hex(request),
            reply: None,
            result: None,
            error: None,
        }
    }

    pub fn answered(self, reply: &[u8], decoded: Result<String, String>) -> Self {
        let reply = Some(trimmed_hex(reply));
        match decoded {
            Ok(result) => Self { reply, result: Some(result), ..self },
            Err(error) => Self { reply, error: Some(error), ..self },
        }
    }

    pub fn failed(self, error: String) -> Self {
        Self { error: Some(error), ..self }
    }

    pub fn succeeded(&self) -> bool {
        self.result.is_some()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueryTiming {
    pub reply_timeout: Duration,
    pub razer_polling: RazerPolling,
}

pub const HARDWARE_TIMING: QueryTiming = QueryTiming {
    reply_timeout: Duration::from_secs(1),
    razer_polling: RazerPolling { interval: Duration::from_millis(8), maximum_polls: 60 },
};

pub fn protocol_family_of(collection: &HidCollectionRecord, reports: &[DeclaredReport]) -> Option<ProtocolFamily> {
    let declares = |kind: ReportKind, report_id: u8, minimum_bytes: usize| {
        reports
            .iter()
            .any(|report| report.kind == kind && report.report_id == report_id && report.byte_length >= minimum_bytes)
    };
    if collection.usage_page == VIA_USAGE_PAGE
        && collection.usage == VIA_USAGE
        && declares(ReportKind::Output, 0, VIA_REPORT_BYTES)
    {
        return Some(ProtocolFamily::Via);
    }
    if collection.vendor_id == WOOTING_VENDOR_ID
        && collection.usage_page == WOOTING_CONFIG_USAGE_PAGE
        && declares(ReportKind::Feature, WOOTING_COMMAND_REPORT_ID, wooting_queries::MINIMUM_COMMAND_BYTES)
    {
        return Some(ProtocolFamily::Wooting);
    }
    let declares_razer_control_report = reports.iter().any(|report| {
        report.kind == ReportKind::Feature && report.report_id == 0 && report.byte_length == RAZER_REPORT_BYTES
    });
    if collection.vendor_id == RAZER_VENDOR_ID && declares_razer_control_report {
        return Some(ProtocolFamily::Razer);
    }
    None
}

pub type TransportOpener<'opener> =
    dyn Fn(&HidCollectionRecord, bool) -> Result<Box<dyn HidTransport>, String> + 'opener;

pub fn query_physical_device(device: &mut PhysicalDevice, open_transport: &TransportOpener, timing: QueryTiming) {
    let mut answered_families: Vec<ProtocolFamily> = Vec::new();
    for interface in &mut device.interfaces {
        let Ok(descriptor) = &interface.collection.report_descriptor else {
            continue;
        };
        let Ok(reports) = declared_reports(descriptor) else {
            continue;
        };
        let Some(family) = protocol_family_of(&interface.collection, &reports) else {
            continue;
        };
        if answered_families.contains(&family) {
            continue;
        }
        interface.queries = match open_transport(&interface.collection, uses_numbered_input_reports(&reports)) {
            Ok(transport) => run_family_queries(family, transport.as_ref(), &reports, timing),
            Err(error) => vec![QueryRecord::new(family, "Open the interface", &[]).failed(error)],
        };
        if interface.queries.iter().any(QueryRecord::succeeded) {
            answered_families.push(family);
        }
    }
}

fn run_family_queries(
    family: ProtocolFamily,
    transport: &dyn HidTransport,
    reports: &[DeclaredReport],
    timing: QueryTiming,
) -> Vec<QueryRecord> {
    match family {
        ProtocolFamily::Via => via_queries::run(transport, timing.reply_timeout),
        ProtocolFamily::Wooting => wooting_queries::run(transport, reports, timing.reply_timeout),
        ProtocolFamily::Razer => razer_queries::run(transport, timing.razer_polling),
    }
}

#[cfg(test)]
pub const TEST_TIMING: QueryTiming = QueryTiming {
    reply_timeout: Duration::from_millis(5),
    razer_polling: RazerPolling { interval: Duration::ZERO, maximum_polls: 3 },
};

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;
    use crate::hid_transport::InputReport;
    use crate::hid_transport::test_support::FakeTransport;
    use crate::physical_devices::group_into_physical_devices;
    use crate::physical_devices::test_support::{collection, interface};
    use crate::report_layout::test_support::*;

    fn collection_with_descriptor(
        vendor_id: u16,
        usage_page: u16,
        usage: u16,
        descriptor: &str,
    ) -> HidCollectionRecord {
        let mut record = collection(vendor_id, 0x1402, usage_page, usage);
        record.report_descriptor = Ok(hex_to_bytes(descriptor));
        record
    }

    fn family_of(record: &HidCollectionRecord) -> Option<ProtocolFamily> {
        protocol_family_of(record, &declared_reports(record.report_descriptor.as_ref().unwrap()).unwrap())
    }

    #[test]
    fn recognizes_each_family_by_its_reports() {
        let via = collection_with_descriptor(0x3434, 0xff60, 0x61, VIA_RAW_HID_DESCRIPTOR);
        let wooting = collection_with_descriptor(0x31e3, 0xff55, 0x01, WOOTING_80HE_CONFIG_DESCRIPTOR);
        let razer = collection_with_descriptor(0x1532, 0x0c, 0x01, RAZER_CONTROL_DESCRIPTOR);

        assert_eq!(family_of(&via), Some(ProtocolFamily::Via));
        assert_eq!(family_of(&wooting), Some(ProtocolFamily::Wooting));
        assert_eq!(family_of(&razer), Some(ProtocolFamily::Razer));
    }

    #[test]
    fn ignores_lookalikes_from_other_vendors() {
        let not_wooting = collection_with_descriptor(0x1234, 0xff55, 0x01, WOOTING_80HE_CONFIG_DESCRIPTOR);
        let not_razer = collection_with_descriptor(0x046d, 0x0c, 0x01, RAZER_CONTROL_DESCRIPTOR);
        let razer_media_keys = collection_with_descriptor(0x1532, 0x0c, 0x01, VIA_RAW_HID_DESCRIPTOR);

        assert_eq!(family_of(&not_wooting), None);
        assert_eq!(family_of(&not_razer), None);
        assert_eq!(family_of(&razer_media_keys), None);
    }

    #[test]
    fn queries_only_matching_interfaces() {
        let keyboard = collection(0x31e3, 0x1402, 0x01, 0x06);
        let config = collection_with_descriptor(0x31e3, 0xff55, 0x01, WOOTING_80HE_CONFIG_DESCRIPTOR);
        let mut device =
            group_into_physical_devices(vec![interface(keyboard, None), interface(config, None)]).remove(0);
        let opened = RefCell::new(Vec::new());
        let open = |record: &HidCollectionRecord, numbered: bool| -> Result<Box<dyn HidTransport>, String> {
            opened.borrow_mut().push((record.usage_page, numbered));
            let reply = hex_to_bytes("d1da0188030002 0e01".replace(' ', "").as_str());
            Ok(Box::new(FakeTransport::with_input_reports(vec![InputReport { report_id: 1, data: reply }])))
        };

        query_physical_device(&mut device, &open, TEST_TIMING);

        assert_eq!(*opened.borrow(), vec![(0xff55, true)]);
        assert!(device.interfaces[0].queries.is_empty());
        assert_eq!(device.interfaces[1].queries[0].result.as_deref(), Some("2.14.1"));
    }

    #[test]
    fn records_an_interface_that_cannot_be_opened() {
        let config = collection_with_descriptor(0x31e3, 0xff55, 0x01, WOOTING_80HE_CONFIG_DESCRIPTOR);
        let mut device = group_into_physical_devices(vec![interface(config, None)]).remove(0);
        let open = |_: &HidCollectionRecord, _: bool| -> Result<Box<dyn HidTransport>, String> {
            Err("Access is denied.".to_string())
        };

        query_physical_device(&mut device, &open, TEST_TIMING);

        assert_eq!(device.interfaces[0].queries[0].error.as_deref(), Some("Access is denied."));
        assert_eq!(device.interfaces[0].queries[0].request, "");
    }
}
