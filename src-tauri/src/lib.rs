pub mod device_filter;
pub mod hid_service;

use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager, State};

use crate::device_filter::DeviceFilter;
use crate::hid_service::{HidService, InputReport, NativeDeviceInfo};

const INPUT_REPORT_EVENT: &str = "hid-input-report";
const DEVICES_CHANGED_EVENT: &str = "hid-devices-changed";
const DEVICE_POLL_INTERVAL: Duration = Duration::from_secs(2);

#[tauri::command]
async fn list_hid_devices(
    service: State<'_, Arc<HidService>>,
    filters: Vec<DeviceFilter>,
) -> Result<Vec<NativeDeviceInfo>, String> {
    service.list_devices(&filters)
}

#[tauri::command]
async fn open_hid_device(
    app: AppHandle,
    service: State<'_, Arc<HidService>>,
    device_key: String,
) -> Result<(), String> {
    let input_reports = Arc::new(move |report: InputReport| {
        let _ = app.emit(INPUT_REPORT_EVENT, report);
    });
    service.open(&device_key, input_reports)
}

#[tauri::command]
async fn close_hid_device(service: State<'_, Arc<HidService>>, device_key: String) -> Result<(), String> {
    service.close(&device_key);
    Ok(())
}

#[tauri::command]
async fn send_output_report(
    service: State<'_, Arc<HidService>>,
    device_key: String,
    report_id: u8,
    data: Vec<u8>,
) -> Result<(), String> {
    service.send_output_report(&device_key, report_id, &data)
}

#[tauri::command]
async fn send_feature_report(
    service: State<'_, Arc<HidService>>,
    device_key: String,
    report_id: u8,
    data: Vec<u8>,
) -> Result<(), String> {
    service.send_feature_report(&device_key, report_id, &data)
}

#[tauri::command]
async fn receive_feature_report(
    service: State<'_, Arc<HidService>>,
    device_key: String,
    report_id: u8,
    byte_length: usize,
) -> Result<Vec<u8>, String> {
    service.receive_feature_report(&device_key, report_id, byte_length)
}

fn watch_devices(app: AppHandle, service: Arc<HidService>) {
    std::thread::spawn(move || {
        let mut previous_keys = service.connected_device_keys();
        loop {
            std::thread::sleep(DEVICE_POLL_INTERVAL);
            let keys = service.connected_device_keys();
            if keys != previous_keys {
                let _ = app.emit(DEVICES_CHANGED_EVENT, ());
                previous_keys = keys;
            }
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let service = Arc::new(HidService::new()?);
            watch_devices(app.handle().clone(), Arc::clone(&service));
            app.manage(service);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            list_hid_devices,
            open_hid_device,
            close_hid_device,
            send_output_report,
            send_feature_report,
            receive_feature_report,
        ])
        .run(tauri::generate_context!())
        .expect("OpenKeyboard could not start");
}
