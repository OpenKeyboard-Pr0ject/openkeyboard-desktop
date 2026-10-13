use std::collections::{BTreeSet, HashMap};
use std::ffi::CString;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;

use hidapi::{DeviceInfo, HidApi, HidDevice};
use serde::Serialize;

use crate::device_filter::{DeviceFilter, EnumeratedCollection, matches_any};

const MAXIMUM_DESCRIPTOR_BYTES: usize = 4096;
const MAXIMUM_INPUT_REPORT_BYTES: usize = 4096;
const INPUT_READ_TIMEOUT_MS: i32 = 100;
const NO_REPORT_ID: u8 = 0;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeDeviceInfo {
    pub device_key: String,
    pub vendor_id: u16,
    pub product_id: u16,
    pub product_name: String,
    pub report_descriptor: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InputReport {
    pub device_key: String,
    pub data: Vec<u8>,
}

pub type InputReportSink = Arc<dyn Fn(InputReport) + Send + Sync>;

struct OpenDevice {
    writer: Mutex<HidDevice>,
    stop_reading: Arc<AtomicBool>,
    reader: Option<JoinHandle<()>>,
}

impl Drop for OpenDevice {
    fn drop(&mut self) {
        self.stop_reading.store(true, Ordering::Relaxed);
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

pub struct HidService {
    api: Mutex<HidApi>,
    descriptors: Mutex<HashMap<String, NativeDeviceInfo>>,
    open_devices: Mutex<HashMap<String, OpenDevice>>,
}

impl HidService {
    pub fn new() -> Result<Self, String> {
        let api = HidApi::new().map_err(|error| format!("Could not start hidapi: {error}"))?;
        Ok(Self {
            api: Mutex::new(api),
            descriptors: Mutex::new(HashMap::new()),
            open_devices: Mutex::new(HashMap::new()),
        })
    }

    pub fn list_devices(&self, filters: &[DeviceFilter]) -> Result<Vec<NativeDeviceInfo>, String> {
        let mut api = lock(&self.api);
        api.refresh_devices().map_err(|error| format!("Could not list HID devices: {error}"))?;
        let matching: Vec<&DeviceInfo> =
            api.device_list().filter(|info| matches_any(filters, &enumerated_collection_of(info))).collect();
        let mut descriptors = lock(&self.descriptors);
        Ok(matching
            .into_iter()
            .filter_map(|info| {
                let device_key = device_key_of(info);
                if let Some(known) = descriptors.get(&device_key) {
                    return Some(known.clone());
                }
                let described = describe(&api, info)?;
                descriptors.insert(device_key, described.clone());
                Some(described)
            })
            .collect())
    }

    pub fn connected_device_keys(&self) -> BTreeSet<String> {
        let mut api = lock(&self.api);
        if api.refresh_devices().is_err() {
            return BTreeSet::new();
        }
        api.device_list().map(device_key_of).collect()
    }

    pub fn open(&self, device_key: &str, input_reports: InputReportSink) -> Result<(), String> {
        let mut open_devices = lock(&self.open_devices);
        if open_devices.contains_key(device_key) {
            return Ok(());
        }
        let api = lock(&self.api);
        let writer = open_path(&api, device_key)?;
        let reader = open_path(&api, device_key)?;
        let stop_reading = Arc::new(AtomicBool::new(false));
        let reader_thread = spawn_reader(device_key.to_string(), reader, Arc::clone(&stop_reading), input_reports);
        open_devices.insert(
            device_key.to_string(),
            OpenDevice { writer: Mutex::new(writer), stop_reading, reader: Some(reader_thread) },
        );
        Ok(())
    }

    pub fn close(&self, device_key: &str) {
        let removed = lock(&self.open_devices).remove(device_key);
        drop(removed);
    }

    pub fn send_output_report(&self, device_key: &str, report_id: u8, data: &[u8]) -> Result<(), String> {
        self.with_writer(device_key, |device| {
            device
                .write(&with_report_id(report_id, data))
                .map(|_| ())
                .map_err(|error| format!("Could not write output report {report_id}: {error}"))
        })
    }

    pub fn send_feature_report(&self, device_key: &str, report_id: u8, data: &[u8]) -> Result<(), String> {
        self.with_writer(device_key, |device| {
            device
                .send_feature_report(&with_report_id(report_id, data))
                .map_err(|error| format!("Could not send feature report {report_id}: {error}"))
        })
    }

    pub fn receive_feature_report(
        &self,
        device_key: &str,
        report_id: u8,
        byte_length: usize,
    ) -> Result<Vec<u8>, String> {
        self.with_writer(device_key, |device| {
            let mut buffer = vec![0; byte_length + 1];
            buffer[0] = report_id;
            let received = device
                .get_feature_report(&mut buffer)
                .map_err(|error| format!("Could not read feature report {report_id}: {error}"))?;
            Ok(without_unnumbered_report_id(&buffer[..received.min(buffer.len())], report_id, byte_length))
        })
    }

    fn with_writer<Value>(
        &self,
        device_key: &str,
        operation: impl FnOnce(&HidDevice) -> Result<Value, String>,
    ) -> Result<Value, String> {
        let open_devices = lock(&self.open_devices);
        let open_device =
            open_devices.get(device_key).ok_or_else(|| "Open the device before sending reports to it.".to_string())?;
        operation(&lock(&open_device.writer))
    }
}

pub fn without_unnumbered_report_id(report: &[u8], report_id: u8, byte_length: usize) -> Vec<u8> {
    if report_id == NO_REPORT_ID && report.len() > byte_length {
        return report[report.len() - byte_length..].to_vec();
    }
    report.to_vec()
}

fn spawn_reader(
    device_key: String,
    device: HidDevice,
    stop_reading: Arc<AtomicBool>,
    input_reports: InputReportSink,
) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let mut buffer = vec![0; MAXIMUM_INPUT_REPORT_BYTES];
        while !stop_reading.load(Ordering::Relaxed) {
            match device.read_timeout(&mut buffer, INPUT_READ_TIMEOUT_MS) {
                Ok(0) => continue,
                Ok(received) => {
                    input_reports(InputReport { device_key: device_key.clone(), data: buffer[..received].to_vec() })
                }
                Err(_) => break,
            }
        }
    })
}

fn describe(api: &HidApi, info: &DeviceInfo) -> Option<NativeDeviceInfo> {
    let device = api.open_path(info.path()).ok()?;
    let mut descriptor = vec![0; MAXIMUM_DESCRIPTOR_BYTES];
    let length = device.get_report_descriptor(&mut descriptor).ok()?;
    descriptor.truncate(length);
    Some(NativeDeviceInfo {
        device_key: device_key_of(info),
        vendor_id: info.vendor_id(),
        product_id: info.product_id(),
        product_name: info.product_string().unwrap_or("HID device").to_string(),
        report_descriptor: descriptor,
    })
}

fn open_path(api: &HidApi, device_key: &str) -> Result<HidDevice, String> {
    let path = CString::new(device_key).map_err(|_| "The device key is not a valid path.".to_string())?;
    api.open_path(&path).map_err(|error| format!("Could not open the device: {error}"))
}

fn enumerated_collection_of(info: &DeviceInfo) -> EnumeratedCollection {
    EnumeratedCollection {
        vendor_id: info.vendor_id(),
        product_id: info.product_id(),
        usage_page: info.usage_page(),
        usage: info.usage(),
    }
}

fn device_key_of(info: &DeviceInfo) -> String {
    info.path().to_string_lossy().into_owned()
}

fn with_report_id(report_id: u8, data: &[u8]) -> Vec<u8> {
    std::iter::once(report_id).chain(data.iter().copied()).collect()
}

fn lock<Value>(mutex: &Mutex<Value>) -> MutexGuard<'_, Value> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drops_the_leading_zero_of_an_unnumbered_feature_report() {
        let mut report = vec![0x00, 0x02, 0x1f];
        report.resize(91, 0);

        assert_eq!(without_unnumbered_report_id(&report, 0, 90)[..2], [0x02, 0x1f]);
    }

    #[test]
    fn keeps_a_report_that_already_has_the_declared_length() {
        let report = vec![0x02; 90];

        assert_eq!(without_unnumbered_report_id(&report, 0, 90).len(), 90);
    }

    #[test]
    fn keeps_the_report_id_of_a_numbered_report() {
        assert_eq!(without_unnumbered_report_id(&[0x01, 0xd1, 0xda], 1, 2), vec![0x01, 0xd1, 0xda]);
    }
}
