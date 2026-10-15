use std::path::Path;

use crate::capture_file::read_capture;
use crate::hex::hex_u16;
use crate::hid_traffic::{CaptureTimeline, TrafficCapture, TrafficScope, extract_traffic_captures, listed_devices};
use crate::usb_transfers::{UsbTransfer, assemble_transfers, parse_usbpcap_packet};

pub const CAPTURE_FILE_SOURCE: &str = "capture-file";

pub struct DecodedCapture {
    pub transfers: Vec<UsbTransfer>,
    pub first_timestamp_micros: u64,
}

pub fn decode_usbpcap_capture(bytes: &[u8]) -> Result<DecodedCapture, String> {
    let captured_packets = read_capture(bytes)?;
    let packets: Vec<_> = captured_packets.iter().filter_map(parse_usbpcap_packet).collect();
    if packets.is_empty() {
        return Err("The capture holds no USBPcap packets. Record it with USBPcap on Windows.".to_string());
    }
    Ok(DecodedCapture {
        first_timestamp_micros: packets.iter().map(|packet| packet.timestamp_micros).min().unwrap_or(0),
        transfers: assemble_transfers(&packets),
    })
}

pub fn captures_for_scope(
    decoded: &DecodedCapture,
    scope: &TrafficScope,
    timeline: &CaptureTimeline,
) -> Result<Vec<TrafficCapture>, String> {
    let captures = extract_traffic_captures(&decoded.transfers, scope, timeline);
    if captures.is_empty() {
        return Err(missing_device_message(scope, &decoded.transfers));
    }
    Ok(captures)
}

pub fn import_capture_file(path: &Path, scope: &TrafficScope) -> Result<Vec<TrafficCapture>, String> {
    let bytes = std::fs::read(path).map_err(|error| format!("Could not read {}: {error}", path.display()))?;
    let decoded = decode_usbpcap_capture(&bytes)?;
    let timeline =
        CaptureTimeline { source: CAPTURE_FILE_SOURCE, started_micros: decoded.first_timestamp_micros, marks: &[] };
    captures_for_scope(&decoded, scope, &timeline)
}

fn missing_device_message(scope: &TrafficScope, transfers: &[UsbTransfer]) -> String {
    let wanted = match scope.product_id {
        Some(product_id) => format!("{}:{}", hex_u16(scope.vendor_id), hex_u16(product_id)),
        None => format!("vendor {}", hex_u16(scope.vendor_id)),
    };
    let listed: Vec<String> = listed_devices(transfers)
        .into_iter()
        .map(|(vendor_id, product_id)| format!("{}:{}", hex_u16(vendor_id), hex_u16(product_id)))
        .collect();
    let seen = if listed.is_empty() { "no device descriptors at all".to_string() } else { listed.join(", ") };
    format!(
        "The capture does not describe {wanted}; it holds {seen}. Start USBPcap with \"Inject already connected \
         devices descriptors\" turned on, or plug the keyboard in after the capture has started."
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture_file::LINKTYPE_USBPCAP;
    use crate::capture_file::test_support::{CaptureRecord, pcap_file, pcapng_file};
    use crate::hid_traffic::TrafficKind;
    use crate::hid_traffic::test_support::{STARTED_MICROS, wooting_scope, wooting_session};
    use crate::usb_transfers::test_support::usbpcap_bytes;

    fn session_records() -> Vec<CaptureRecord> {
        wooting_session()
            .iter()
            .map(|record| CaptureRecord { timestamp_micros: record.timestamp_micros, data: usbpcap_bytes(record) })
            .collect()
    }

    fn import(bytes: &[u8], scope: &TrafficScope) -> Result<Vec<TrafficCapture>, String> {
        let decoded = decode_usbpcap_capture(bytes)?;
        let timeline =
            CaptureTimeline { source: CAPTURE_FILE_SOURCE, started_micros: decoded.first_timestamp_micros, marks: &[] };
        captures_for_scope(&decoded, scope, &timeline)
    }

    #[test]
    fn imports_a_pcapng_file_written_by_wireshark() {
        let captures = import(&pcapng_file(LINKTYPE_USBPCAP, &session_records()), &wooting_scope()).unwrap();

        assert_eq!(captures[0].source, "capture-file");
        assert_eq!(captures[0].entries[0].kind, TrafficKind::SetFeature);
        assert_eq!(captures[0].entries[0].at_micros, 1_000_000 - 10);
    }

    #[test]
    fn imports_a_pcap_file_written_by_usbpcapcmd() {
        let captures = import(&pcap_file(LINKTYPE_USBPCAP, &session_records()), &wooting_scope()).unwrap();

        assert_eq!(captures[0].entries.len(), 3);
        assert_eq!(captures[0].release_number, "0x0250");
    }

    #[test]
    fn names_the_devices_it_found_when_the_keyboard_is_missing() {
        let scope = TrafficScope { vendor_id: 0x1532, product_id: Some(0x0258), ..TrafficScope::default() };

        let error = import(&pcap_file(LINKTYPE_USBPCAP, &session_records()), &scope).unwrap_err();

        assert!(
            error.starts_with("The capture does not describe 0x1532:0x0258; it holds 0x046d:0xc52b, 0x31e3:0x1402.")
        );
    }

    #[test]
    #[ignore = "writes target/usbpcap-sample.pcapng so the fixture can be checked with Wireshark or tshark"]
    fn writes_a_sample_capture_for_wireshark() {
        let destination = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("target/usbpcap-sample.pcapng");

        std::fs::write(&destination, pcapng_file(LINKTYPE_USBPCAP, &session_records())).unwrap();
    }

    #[test]
    fn rejects_a_capture_of_another_link_type() {
        let ethernet = pcap_file(1, &[CaptureRecord { timestamp_micros: STARTED_MICROS, data: vec![0; 60] }]);

        assert_eq!(
            import(&ethernet, &wooting_scope()).unwrap_err(),
            "The capture holds no USBPcap packets. Record it with USBPcap on Windows."
        );
    }
}
