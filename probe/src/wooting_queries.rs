use std::time::Duration;

use crate::hid_transport::{HidTransport, wait_for_input_report};
use crate::read_only_queries::{ProtocolFamily, QueryRecord};
use crate::report_layout::{DeclaredReport, ReportKind, find_report};

const COMMAND_REPORT_ID: u8 = 1;
const COMMAND_MAGIC: [u8; 2] = [0xd1, 0xda];
const COMMAND_PARAMETER_BYTES: usize = 4;
pub const MINIMUM_COMMAND_BYTES: usize = COMMAND_MAGIC.len() + 1 + COMMAND_PARAMETER_BYTES;

const REPLY_COMMAND_OFFSET: usize = 2;
const REPLY_STATUS_OFFSET: usize = 3;
const REPLY_PAYLOAD_LENGTH_OFFSET: usize = 4;
const REPLY_PAYLOAD_OFFSET: usize = 6;
const REPLY_SUCCESS_STATUS: u8 = 0x88;
const FIRMWARE_VERSION_BYTES: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WootingQuery {
    FirmwareVersion,
}

impl WootingQuery {
    pub fn name(self) -> &'static str {
        match self {
            Self::FirmwareVersion => "Wooting firmware version",
        }
    }

    pub fn command(self) -> u8 {
        match self {
            Self::FirmwareVersion => 0x01,
        }
    }

    pub fn request(self, feature_report_bytes: usize) -> Vec<u8> {
        let mut request = vec![0; feature_report_bytes.max(MINIMUM_COMMAND_BYTES)];
        request[..COMMAND_MAGIC.len()].copy_from_slice(&COMMAND_MAGIC);
        request[REPLY_COMMAND_OFFSET] = self.command();
        request
    }

    pub fn answers(self, reply: &[u8]) -> bool {
        reply.starts_with(&COMMAND_MAGIC) && reply.get(REPLY_COMMAND_OFFSET) == Some(&self.command())
    }

    pub fn decode(self, reply: &[u8]) -> Result<String, String> {
        let status = *reply.get(REPLY_STATUS_OFFSET).ok_or("The reply has no status byte.")?;
        if status != REPLY_SUCCESS_STATUS {
            return Err(format!("The keyboard answered with status 0x{status:02x}."));
        }
        let length_bytes =
            reply.get(REPLY_PAYLOAD_LENGTH_OFFSET..REPLY_PAYLOAD_OFFSET).ok_or("The reply has no payload length.")?;
        let payload_length = usize::from(u16::from_le_bytes([length_bytes[0], length_bytes[1]]));
        let payload = reply
            .get(REPLY_PAYLOAD_OFFSET..REPLY_PAYLOAD_OFFSET + payload_length)
            .ok_or("The reply declares a longer payload than it carries.")?;
        match self {
            Self::FirmwareVersion => {
                let version = payload
                    .get(..FIRMWARE_VERSION_BYTES)
                    .ok_or_else(|| format!("The firmware version reply carries only {} bytes.", payload.len()))?;
                Ok(format!("{}.{}.{}", version[0], version[1], version[2]))
            }
        }
    }
}

pub fn run(transport: &dyn HidTransport, reports: &[DeclaredReport], reply_timeout: Duration) -> Vec<QueryRecord> {
    let Some(command_report) = find_report(reports, ReportKind::Feature, COMMAND_REPORT_ID) else {
        return Vec::new();
    };
    vec![ask(transport, WootingQuery::FirmwareVersion, command_report.byte_length, reply_timeout)]
}

fn ask(transport: &dyn HidTransport, query: WootingQuery, feature_bytes: usize, timeout: Duration) -> QueryRecord {
    let request = query.request(feature_bytes);
    let record = QueryRecord::new(ProtocolFamily::Wooting, query.name(), &request);
    if let Err(error) = transport.send_feature_report(COMMAND_REPORT_ID, &request) {
        return record.failed(error);
    }
    match wait_for_input_report(transport, timeout, |report| query.answers(&report.data)) {
        Ok(Some(reply)) => record.answered(&reply.data, query.decode(&reply.data)),
        Ok(None) => record.failed("The keyboard did not answer in time.".to_string()),
        Err(error) => record.failed(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hid_transport::InputReport;
    use crate::hid_transport::test_support::{FakeTransport, SentReport};
    use crate::read_only_queries::TEST_TIMING;

    fn command_report() -> Vec<DeclaredReport> {
        vec![DeclaredReport { kind: ReportKind::Feature, report_id: 1, byte_length: 7 }]
    }

    fn input(data: &[u8]) -> InputReport {
        InputReport { report_id: 1, data: data.to_vec() }
    }

    #[test]
    fn reads_the_firmware_version_as_wootility_does() {
        let transport =
            FakeTransport::with_input_reports(vec![input(&[0xd1, 0xda, 0x01, 0x88, 0x03, 0x00, 0x02, 0x0e, 0x01])]);

        let records = run(&transport, &command_report(), TEST_TIMING.reply_timeout);

        assert_eq!(records[0].result.as_deref(), Some("2.14.1"));
        assert_eq!(
            transport.sent_reports(),
            vec![SentReport::Feature { report_id: 1, data: vec![0xd1, 0xda, 0x01, 0, 0, 0, 0] }]
        );
    }

    #[test]
    fn skips_replies_meant_for_another_app() {
        let transport = FakeTransport::with_input_reports(vec![
            input(&[0xd1, 0xda, 0x4a, 0x88, 0x00, 0x00]),
            input(&[0xd1, 0xda, 0x01, 0x88, 0x03, 0x00, 0x02, 0x0e, 0x00]),
        ]);

        let records = run(&transport, &command_report(), TEST_TIMING.reply_timeout);

        assert_eq!(records[0].result.as_deref(), Some("2.14.0"));
    }

    #[test]
    fn reports_a_rejected_command() {
        let transport = FakeTransport::with_input_reports(vec![input(&[0xd1, 0xda, 0x01, 0xff, 0x00, 0x00])]);

        let records = run(&transport, &command_report(), TEST_TIMING.reply_timeout);

        assert_eq!(records[0].error.as_deref(), Some("The keyboard answered with status 0xff."));
        assert_eq!(records[0].reply.as_deref(), Some("d1da01ff"));
    }

    #[test]
    fn asks_nothing_without_the_command_report() {
        let transport = FakeTransport::with_input_reports(Vec::new());

        assert!(run(&transport, &[], TEST_TIMING.reply_timeout).is_empty());
        assert!(transport.sent_reports().is_empty());
    }
}
