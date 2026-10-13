use std::sync::Arc;
use std::time::Duration;

use openkeyboard_desktop_lib::device_filter::DeviceFilter;
use openkeyboard_desktop_lib::hid_service::HidService;

const RAZER_REPORT_LENGTH: usize = 90;
const CONTROL_REPORT_COUNT_ITEM: [u8; 2] = [0x95, 0x5a];
const REPLY_DELAY: Duration = Duration::from_millis(20);

fn firmware_version_request() -> Vec<u8> {
    let mut request = vec![0; RAZER_REPORT_LENGTH];
    request[1] = 0x1f;
    request[5] = 0x02;
    request[7] = 0x81;
    request[88] = 0x02 ^ 0x81;
    request
}

#[test]
#[ignore = "needs a Razer BlackWidow V3 Mini connected with the cable"]
fn reads_the_firmware_version_of_the_wired_blackwidow_v3_mini() {
    let service = HidService::new().unwrap();
    let filter =
        DeviceFilter { vendor_id: Some(0x1532), product_id: Some(0x0258), usage_page: Some(0x0c), usage: Some(0x01) };
    let devices = service.list_devices(&[filter]).unwrap();
    let control = devices
        .iter()
        .find(|device| device.report_descriptor.windows(2).any(|item| item == CONTROL_REPORT_COUNT_ITEM))
        .expect("the control interface is listed with its descriptor");

    service.open(&control.device_key, Arc::new(|_| {})).unwrap();
    service.send_feature_report(&control.device_key, 0, &firmware_version_request()).unwrap();
    std::thread::sleep(REPLY_DELAY);
    let reply = service.receive_feature_report(&control.device_key, 0, RAZER_REPORT_LENGTH).unwrap();
    service.close(&control.device_key);

    assert_eq!(reply.len(), RAZER_REPORT_LENGTH);
    assert_eq!(reply[..10], [0x02, 0x1f, 0x00, 0x00, 0x00, 0x02, 0x00, 0x81, 0x02, 0x03]);
}
