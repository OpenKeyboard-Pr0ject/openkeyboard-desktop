use std::ffi::CString;
use std::time::{Duration, Instant};

use hidapi::{HidApi, HidDevice};

use crate::hid_inventory::HidCollectionRecord;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputReport {
    pub report_id: u8,
    pub data: Vec<u8>,
}

pub trait HidTransport {
    fn write_output_report(&self, report_id: u8, data: &[u8]) -> Result<(), String>;
    fn send_feature_report(&self, report_id: u8, data: &[u8]) -> Result<(), String>;
    fn receive_feature_report(&self, report_id: u8, byte_length: usize) -> Result<Vec<u8>, String>;
    fn read_input_report(&self, timeout: Duration) -> Result<Option<InputReport>, String>;
}

const MAXIMUM_INPUT_REPORT_BYTES: usize = 4096;

pub struct HidapiTransport {
    device: HidDevice,
    numbered_input_reports: bool,
}

impl HidapiTransport {
    pub fn new(device: HidDevice, numbered_input_reports: bool) -> Self {
        Self { device, numbered_input_reports }
    }
}

impl HidTransport for HidapiTransport {
    fn write_output_report(&self, report_id: u8, data: &[u8]) -> Result<(), String> {
        self.device
            .write(&with_report_id(report_id, data))
            .map(|_| ())
            .map_err(|error| format!("Could not write output report {report_id}: {error}"))
    }

    fn send_feature_report(&self, report_id: u8, data: &[u8]) -> Result<(), String> {
        self.device
            .send_feature_report(&with_report_id(report_id, data))
            .map_err(|error| format!("Could not send feature report {report_id}: {error}"))
    }

    fn receive_feature_report(&self, report_id: u8, byte_length: usize) -> Result<Vec<u8>, String> {
        let mut buffer = vec![0; byte_length + 1];
        buffer[0] = report_id;
        let received = self
            .device
            .get_feature_report(&mut buffer)
            .map_err(|error| format!("Could not read feature report {report_id}: {error}"))?;
        Ok(without_report_id(&buffer[..received.min(buffer.len())], byte_length))
    }

    fn read_input_report(&self, timeout: Duration) -> Result<Option<InputReport>, String> {
        let mut buffer = vec![0; MAXIMUM_INPUT_REPORT_BYTES];
        let timeout_ms = i32::try_from(timeout.as_millis()).unwrap_or(i32::MAX);
        let received = self
            .device
            .read_timeout(&mut buffer, timeout_ms)
            .map_err(|error| format!("Could not read an input report: {error}"))?;
        if received == 0 {
            return Ok(None);
        }
        Ok(Some(split_input_report(&buffer[..received], self.numbered_input_reports)))
    }
}

pub fn open_hidapi_transport(
    api: &HidApi,
    collection: &HidCollectionRecord,
    numbered_input_reports: bool,
) -> Result<Box<dyn HidTransport>, String> {
    let path = CString::new(collection.path.as_str()).map_err(|_| "The device path is not valid.".to_string())?;
    let device = api.open_path(&path).map_err(|error| format!("Could not open the interface: {error}"))?;
    Ok(Box::new(HidapiTransport::new(device, numbered_input_reports)))
}

pub fn wait_for_input_report(
    transport: &dyn HidTransport,
    timeout: Duration,
    answers_request: impl Fn(&InputReport) -> bool,
) -> Result<Option<InputReport>, String> {
    let deadline = Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Ok(None);
        }
        if let Some(report) = transport.read_input_report(remaining)?
            && answers_request(&report)
        {
            return Ok(Some(report));
        }
    }
}

pub fn without_report_id(report: &[u8], byte_length: usize) -> Vec<u8> {
    if report.len() > byte_length {
        return report[report.len() - byte_length..].to_vec();
    }
    report.to_vec()
}

pub fn split_input_report(report: &[u8], numbered_input_reports: bool) -> InputReport {
    match report.split_first() {
        Some((report_id, data)) if numbered_input_reports => InputReport { report_id: *report_id, data: data.to_vec() },
        _ => InputReport { report_id: 0, data: report.to_vec() },
    }
}

fn with_report_id(report_id: u8, data: &[u8]) -> Vec<u8> {
    std::iter::once(report_id).chain(data.iter().copied()).collect()
}

#[cfg(test)]
pub mod test_support {
    use std::cell::RefCell;
    use std::collections::VecDeque;

    use super::*;

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum SentReport {
        Output { report_id: u8, data: Vec<u8> },
        Feature { report_id: u8, data: Vec<u8> },
    }

    pub type FeatureResponder = Box<dyn Fn(&[SentReport]) -> Vec<u8>>;

    pub struct FakeTransport {
        pub sent: RefCell<Vec<SentReport>>,
        pub input_reports: RefCell<VecDeque<InputReport>>,
        pub feature_responder: FeatureResponder,
        pub fail_sending: bool,
    }

    impl FakeTransport {
        pub fn with_input_reports(input_reports: Vec<InputReport>) -> Self {
            Self {
                sent: RefCell::new(Vec::new()),
                input_reports: RefCell::new(input_reports.into()),
                feature_responder: Box::new(|_| Vec::new()),
                fail_sending: false,
            }
        }

        pub fn with_feature_responder(feature_responder: FeatureResponder) -> Self {
            Self { feature_responder, ..Self::with_input_reports(Vec::new()) }
        }

        pub fn sent_reports(&self) -> Vec<SentReport> {
            self.sent.borrow().clone()
        }

        fn record(&self, report: SentReport) -> Result<(), String> {
            if self.fail_sending {
                return Err("The device was unplugged.".to_string());
            }
            self.sent.borrow_mut().push(report);
            Ok(())
        }
    }

    impl HidTransport for FakeTransport {
        fn write_output_report(&self, report_id: u8, data: &[u8]) -> Result<(), String> {
            self.record(SentReport::Output { report_id, data: data.to_vec() })
        }

        fn send_feature_report(&self, report_id: u8, data: &[u8]) -> Result<(), String> {
            self.record(SentReport::Feature { report_id, data: data.to_vec() })
        }

        fn receive_feature_report(&self, _report_id: u8, _byte_length: usize) -> Result<Vec<u8>, String> {
            Ok((self.feature_responder)(&self.sent.borrow()))
        }

        fn read_input_report(&self, _timeout: Duration) -> Result<Option<InputReport>, String> {
            Ok(self.input_reports.borrow_mut().pop_front())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drops_the_leading_report_id_of_a_feature_report() {
        let mut report = vec![0x00, 0x02, 0x1f];
        report.resize(91, 0);

        assert_eq!(without_report_id(&report, 90)[..2], [0x02, 0x1f]);
    }

    #[test]
    fn keeps_a_feature_report_of_the_declared_length() {
        assert_eq!(without_report_id(&[0x02; 90], 90).len(), 90);
    }

    #[test]
    fn splits_a_numbered_input_report() {
        assert_eq!(split_input_report(&[0x01, 0xd1, 0xda], true), InputReport { report_id: 1, data: vec![0xd1, 0xda] });
    }

    #[test]
    fn keeps_an_unnumbered_input_report_whole() {
        assert_eq!(split_input_report(&[0x01, 0x00, 0x0c], false), InputReport { report_id: 0, data: vec![1, 0, 12] });
    }
}
