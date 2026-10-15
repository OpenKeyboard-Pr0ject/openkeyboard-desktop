use std::time::Duration;

use crate::hid_transport::HidTransport;
use crate::read_only_queries::{ProtocolFamily, QueryRecord};

const REPORT_ID: u8 = 0;
const REPORT_LENGTH: usize = 90;
pub const TRANSACTION_IDS: [u8; 4] = [0x1f, 0x3f, 0x9f, 0xff];

const STATUS_OFFSET: usize = 0;
const TRANSACTION_ID_OFFSET: usize = 1;
const DATA_SIZE_OFFSET: usize = 5;
const COMMAND_CLASS_OFFSET: usize = 6;
const COMMAND_ID_OFFSET: usize = 7;
const ARGUMENTS_OFFSET: usize = 8;
const CHECKSUM_OFFSET: usize = 88;
const CHECKSUM_START_OFFSET: usize = 2;

const STATUS_NEW: u8 = 0x00;
const STATUS_BUSY: u8 = 0x01;
const STATUS_SUCCESS: u8 = 0x02;
const STATUS_NO_RESPONSE: u8 = 0x04;
const STATUS_NOT_SUPPORTED: u8 = 0x05;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RazerPolling {
    pub interval: Duration,
    pub maximum_polls: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RazerQuery {
    FirmwareVersion,
}

impl RazerQuery {
    fn command_class(self) -> u8 {
        match self {
            Self::FirmwareVersion => 0x00,
        }
    }

    fn command_id(self) -> u8 {
        match self {
            Self::FirmwareVersion => 0x81,
        }
    }

    fn data_size(self) -> u8 {
        match self {
            Self::FirmwareVersion => 0x02,
        }
    }

    pub fn name(self, transaction_id: u8) -> String {
        match self {
            Self::FirmwareVersion => format!("Razer firmware version, transaction id 0x{transaction_id:02x}"),
        }
    }

    pub fn request(self, transaction_id: u8) -> [u8; REPORT_LENGTH] {
        let mut request = [0; REPORT_LENGTH];
        request[TRANSACTION_ID_OFFSET] = transaction_id;
        request[DATA_SIZE_OFFSET] = self.data_size();
        request[COMMAND_CLASS_OFFSET] = self.command_class();
        request[COMMAND_ID_OFFSET] = self.command_id();
        request[CHECKSUM_OFFSET] = checksum_of(&request);
        request
    }

    pub fn is_pending(self, reply: &[u8], transaction_id: u8) -> bool {
        if reply.len() < REPORT_LENGTH {
            return true;
        }
        let status = reply[STATUS_OFFSET];
        status == STATUS_NEW || status == STATUS_BUSY || !self.answers(reply, transaction_id)
    }

    pub fn decode(self, reply: &[u8]) -> Result<String, String> {
        if reply[CHECKSUM_OFFSET] != checksum_of(reply) {
            return Err("The reply has a wrong checksum.".to_string());
        }
        match reply[STATUS_OFFSET] {
            STATUS_SUCCESS => {}
            STATUS_NOT_SUPPORTED => return Err("The device does not support this command.".to_string()),
            STATUS_NO_RESPONSE => return Err("The receiver could not reach the device.".to_string()),
            status => return Err(format!("The device answered with status 0x{status:02x}.")),
        }
        match self {
            Self::FirmwareVersion => {
                let major = reply[ARGUMENTS_OFFSET];
                let minor = reply[ARGUMENTS_OFFSET + 1];
                Ok(format!("{major}.{minor}"))
            }
        }
    }

    fn answers(self, reply: &[u8], transaction_id: u8) -> bool {
        reply[TRANSACTION_ID_OFFSET] == transaction_id
            && reply[COMMAND_CLASS_OFFSET] == self.command_class()
            && reply[COMMAND_ID_OFFSET] == self.command_id()
    }
}

pub fn run(transport: &dyn HidTransport, polling: RazerPolling) -> Vec<QueryRecord> {
    let mut records = Vec::new();
    for transaction_id in TRANSACTION_IDS {
        let (record, answered) = ask(transport, RazerQuery::FirmwareVersion, transaction_id, polling);
        records.push(record);
        if answered {
            break;
        }
    }
    records
}

fn ask(
    transport: &dyn HidTransport,
    query: RazerQuery,
    transaction_id: u8,
    polling: RazerPolling,
) -> (QueryRecord, bool) {
    let request = query.request(transaction_id);
    let record = QueryRecord::new(ProtocolFamily::Razer, &query.name(transaction_id), &request);
    if let Err(error) = transport.send_feature_report(REPORT_ID, &request) {
        return (record.failed(error), true);
    }
    for _ in 0..polling.maximum_polls {
        std::thread::sleep(polling.interval);
        let reply = match transport.receive_feature_report(REPORT_ID, REPORT_LENGTH) {
            Ok(reply) => reply,
            Err(error) => return (record.failed(error), true),
        };
        if !query.is_pending(&reply, transaction_id) {
            return (record.answered(&reply, query.decode(&reply)), true);
        }
    }
    (record.failed("The device did not answer this transaction id.".to_string()), false)
}

fn checksum_of(report: &[u8]) -> u8 {
    report[CHECKSUM_START_OFFSET..CHECKSUM_OFFSET].iter().fold(0, |checksum, byte| checksum ^ byte)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hid_transport::test_support::{FakeTransport, SentReport};
    use crate::read_only_queries::TEST_TIMING;

    fn blackwidow_v3_mini_reply(transaction_id: u8) -> Vec<u8> {
        let mut reply = vec![0; REPORT_LENGTH];
        reply[..10].copy_from_slice(&[0x02, transaction_id, 0x00, 0x00, 0x00, 0x02, 0x00, 0x81, 0x02, 0x03]);
        reply[CHECKSUM_OFFSET] = checksum_of(&reply);
        reply
    }

    fn device_answering(transaction_id: u8) -> FakeTransport {
        FakeTransport::with_feature_responder(Box::new(move |sent| match sent.last() {
            Some(SentReport::Feature { data, .. }) if data[TRANSACTION_ID_OFFSET] == transaction_id => {
                blackwidow_v3_mini_reply(transaction_id)
            }
            Some(SentReport::Feature { data, .. }) => data.clone(),
            _ => Vec::new(),
        }))
    }

    #[test]
    fn builds_the_request_that_openkeyboard_sends() {
        let request = RazerQuery::FirmwareVersion.request(0x1f);

        assert_eq!(request[..8], [0x00, 0x1f, 0x00, 0x00, 0x00, 0x02, 0x00, 0x81]);
        assert_eq!(request[CHECKSUM_OFFSET], 0x83);
    }

    #[test]
    fn reads_the_firmware_version_with_the_first_transaction_id() {
        let transport = device_answering(0x1f);

        let records = run(&transport, TEST_TIMING.razer_polling);

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].result.as_deref(), Some("2.3"));
        assert_eq!(records[0].name, "Razer firmware version, transaction id 0x1f");
    }

    #[test]
    fn tries_the_next_transaction_id_until_one_is_answered() {
        let transport = device_answering(0x3f);

        let records = run(&transport, TEST_TIMING.razer_polling);

        assert_eq!(records.len(), 2);
        assert_eq!(records[0].error.as_deref(), Some("The device did not answer this transaction id."));
        assert_eq!(records[1].result.as_deref(), Some("2.3"));
    }

    #[test]
    fn stops_at_a_receiver_without_its_device() {
        let transport = FakeTransport::with_feature_responder(Box::new(|_| {
            let mut reply = blackwidow_v3_mini_reply(0x1f);
            reply[STATUS_OFFSET] = STATUS_NO_RESPONSE;
            reply[CHECKSUM_OFFSET] = checksum_of(&reply);
            reply
        }));

        let records = run(&transport, TEST_TIMING.razer_polling);

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].error.as_deref(), Some("The receiver could not reach the device."));
    }

    #[test]
    fn rejects_a_wrong_checksum() {
        let mut reply = blackwidow_v3_mini_reply(0x1f);
        reply[CHECKSUM_OFFSET] ^= 0xff;

        assert_eq!(RazerQuery::FirmwareVersion.decode(&reply), Err("The reply has a wrong checksum.".to_string()));
    }

    #[test]
    fn stops_when_sending_fails() {
        let mut transport = device_answering(0x1f);
        transport.fail_sending = true;

        let records = run(&transport, TEST_TIMING.razer_polling);

        assert_eq!(records.len(), 1);
        assert_eq!(records[0].error.as_deref(), Some("The device was unplugged."));
    }
}
