use std::time::Duration;

use crate::hex::hex_bytes;
use crate::hid_transport::{HidTransport, wait_for_input_report};
use crate::read_only_queries::{ProtocolFamily, QueryRecord};

const REPORT_ID: u8 = 0;
const REPORT_LENGTH: usize = 32;
const UNHANDLED_COMMAND: u8 = 0xff;
const GET_PROTOCOL_VERSION: [u8; 1] = [0x01];
const GET_FIRMWARE_VERSION: [u8; 2] = [0x02, 0x04];
const GET_VIAL_KEYBOARD_ID: [u8; 2] = [0xfe, 0x00];
const VIAL_PROTOCOL_BYTES: usize = 4;
const VIAL_KEYBOARD_UID_BYTES: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViaQuery {
    ProtocolVersion,
    FirmwareVersion,
    VialKeyboardId,
}

impl ViaQuery {
    pub const ALL: [Self; 3] = [Self::ProtocolVersion, Self::FirmwareVersion, Self::VialKeyboardId];

    pub fn name(self) -> &'static str {
        match self {
            Self::ProtocolVersion => "VIA protocol version",
            Self::FirmwareVersion => "VIA firmware version",
            Self::VialKeyboardId => "Vial keyboard id",
        }
    }

    pub fn request(self) -> [u8; REPORT_LENGTH] {
        let command: &[u8] = match self {
            Self::ProtocolVersion => &GET_PROTOCOL_VERSION,
            Self::FirmwareVersion => &GET_FIRMWARE_VERSION,
            Self::VialKeyboardId => &GET_VIAL_KEYBOARD_ID,
        };
        let mut request = [0; REPORT_LENGTH];
        request[..command.len()].copy_from_slice(command);
        request
    }

    pub fn answers(self, reply: &[u8]) -> bool {
        match self {
            Self::VialKeyboardId => !reply.is_empty(),
            Self::ProtocolVersion | Self::FirmwareVersion => {
                reply.first().is_some_and(|command| *command == self.request()[0] || *command == UNHANDLED_COMMAND)
            }
        }
    }

    pub fn decode(self, reply: &[u8]) -> Result<String, String> {
        match self {
            Self::ProtocolVersion => {
                reject_unhandled(reply)?;
                Ok(big_endian(field(reply, 1, 2)?).to_string())
            }
            Self::FirmwareVersion => {
                reject_unhandled(reply)?;
                Ok(format!("0x{:08x}", big_endian(field(reply, 2, 4)?)))
            }
            Self::VialKeyboardId => decode_vial_keyboard_id(reply),
        }
    }
}

pub fn run(transport: &dyn HidTransport, reply_timeout: Duration) -> Vec<QueryRecord> {
    let mut records = Vec::new();
    for query in ViaQuery::ALL {
        let record = ask(transport, query, reply_timeout);
        let speaks_via = record.succeeded() || query != ViaQuery::ProtocolVersion;
        records.push(record);
        if !speaks_via {
            break;
        }
    }
    records
}

fn ask(transport: &dyn HidTransport, query: ViaQuery, reply_timeout: Duration) -> QueryRecord {
    let request = query.request();
    let record = QueryRecord::new(ProtocolFamily::Via, query.name(), &request);
    if let Err(error) = transport.write_output_report(REPORT_ID, &request) {
        return record.failed(error);
    }
    match wait_for_input_report(transport, reply_timeout, |report| query.answers(&report.data)) {
        Ok(Some(reply)) => record.answered(&reply.data, query.decode(&reply.data)),
        Ok(None) => record.failed("The keyboard did not answer in time.".to_string()),
        Err(error) => record.failed(error),
    }
}

fn decode_vial_keyboard_id(reply: &[u8]) -> Result<String, String> {
    let protocol_bytes = field(reply, 0, VIAL_PROTOCOL_BYTES)?;
    if protocol_bytes[0] == UNHANDLED_COMMAND && protocol_bytes[1..].iter().all(|byte| *byte == 0) {
        return Err("The keyboard does not speak Vial.".to_string());
    }
    let protocol_version =
        u32::from_le_bytes([protocol_bytes[0], protocol_bytes[1], protocol_bytes[2], protocol_bytes[3]]);
    let keyboard_uid = field(reply, VIAL_PROTOCOL_BYTES, VIAL_KEYBOARD_UID_BYTES)?;
    Ok(format!("Vial protocol {protocol_version}, keyboard uid {}", hex_bytes(keyboard_uid)))
}

fn reject_unhandled(reply: &[u8]) -> Result<(), String> {
    if reply.first() == Some(&UNHANDLED_COMMAND) {
        return Err("The keyboard does not support this command.".to_string());
    }
    Ok(())
}

fn field(reply: &[u8], offset: usize, length: usize) -> Result<&[u8], String> {
    reply.get(offset..offset + length).ok_or_else(|| format!("The reply carries only {} bytes.", reply.len()))
}

fn big_endian(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0, |value, byte| (value << 8) | u64::from(*byte))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hid_transport::InputReport;
    use crate::hid_transport::test_support::{FakeTransport, SentReport};
    use crate::read_only_queries::TEST_TIMING;

    fn reply(bytes: &[u8]) -> InputReport {
        let mut data = bytes.to_vec();
        data.resize(REPORT_LENGTH, 0);
        InputReport { report_id: 0, data }
    }

    #[test]
    fn asks_for_versions_and_the_vial_keyboard_id() {
        let transport = FakeTransport::with_input_reports(vec![
            reply(&[0x01, 0x00, 0x0c]),
            reply(&[0x02, 0x04, 0x00, 0x00, 0x00, 0x01]),
            reply(&[0x06, 0x00, 0x00, 0x00, 0x8f, 0x3b, 0x6e, 0x5a, 0x9c, 0xd2, 0x41, 0x17]),
        ]);

        let records = run(&transport, TEST_TIMING.reply_timeout);

        let results: Vec<_> = records.iter().map(|record| record.result.as_deref()).collect();
        assert_eq!(
            results,
            vec![Some("12"), Some("0x00000001"), Some("Vial protocol 6, keyboard uid 8f3b6e5a9cd24117")]
        );
        assert_eq!(
            transport.sent_reports()[2],
            SentReport::Output { report_id: 0, data: ViaQuery::VialKeyboardId.request().to_vec() }
        );
    }

    #[test]
    fn reports_a_plain_via_keyboard() {
        let transport = FakeTransport::with_input_reports(vec![
            reply(&[0x01, 0x00, 0x0b]),
            reply(&[0xff, 0x04]),
            reply(&[0xff, 0x00]),
        ]);

        let records = run(&transport, TEST_TIMING.reply_timeout);

        assert_eq!(records[1].error.as_deref(), Some("The keyboard does not support this command."));
        assert_eq!(records[2].error.as_deref(), Some("The keyboard does not speak Vial."));
    }

    #[test]
    fn stops_when_the_protocol_version_gets_no_answer() {
        let transport = FakeTransport::with_input_reports(Vec::new());

        let records = run(&transport, TEST_TIMING.reply_timeout);

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].error.as_deref(), Some("The keyboard did not answer in time."));
        assert_eq!(records[0].request, "01");
    }

    #[test]
    fn skips_replies_to_other_commands() {
        let transport = FakeTransport::with_input_reports(vec![reply(&[0x12, 0x00]), reply(&[0x01, 0x00, 0x09])]);

        let records = run(&transport, TEST_TIMING.reply_timeout);

        assert_eq!(records[0].result.as_deref(), Some("9"));
    }
}
