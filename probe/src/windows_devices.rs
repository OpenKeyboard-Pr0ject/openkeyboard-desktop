use serde::Serialize;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowsDevice {
    pub instance_id: String,
    pub enumerator: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bus_reported_description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manufacturer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub lower_filters: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub upper_filters: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub hardware_ids: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub compatible_ids: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub driver_provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub driver_version: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WindowsDeviceLookup {
    pub chain: Vec<WindowsDevice>,
    pub container_key: Option<String>,
}

const PERIPHERAL_ENUMERATORS: [&str; 5] = ["HID", "USB", "BTHENUM", "BTHLEDEVICE", "BTHLE"];
const HUB_CLASS_COMPATIBLE_ID_PREFIX: &str = "USB\\CLASS_09";
const ROOT_HUB_INSTANCE_ID_PREFIX: &str = "USB\\ROOT_HUB";

pub fn belongs_to_the_peripheral(raw_instance_id: &str, enumerator: &str, compatible_ids: &[String]) -> bool {
    let known_enumerator = PERIPHERAL_ENUMERATORS.iter().any(|known| enumerator.eq_ignore_ascii_case(known));
    let is_root_hub = raw_instance_id.to_ascii_uppercase().starts_with(ROOT_HUB_INSTANCE_ID_PREFIX);
    let is_hub = compatible_ids.iter().any(|id| id.to_ascii_uppercase().starts_with(HUB_CLASS_COMPATIBLE_ID_PREFIX));
    known_enumerator && !is_root_hub && !is_hub
}

#[cfg(windows)]
pub use crate::windows_device_tree::look_up_windows_devices;

#[cfg(not(windows))]
pub fn look_up_windows_devices(_device_path: &str) -> WindowsDeviceLookup {
    WindowsDeviceLookup::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follows_hid_and_usb_nodes() {
        assert!(belongs_to_the_peripheral(r"HID\VID_31E3&PID_1402&MI_02\x", "HID", &[]));
        assert!(belongs_to_the_peripheral(r"USB\VID_31E3&PID_1402\x", "USB", &[r"USB\COMPOSITE".to_string()]));
    }

    #[test]
    fn stops_at_a_hub() {
        let hub_ids = [r"USB\Class_09&SubClass_00".to_string(), r"USB\Class_09".to_string()];

        assert!(!belongs_to_the_peripheral(r"USB\VID_05E3&PID_0610\x", "USB", &hub_ids));
    }

    #[test]
    fn stops_at_the_root_hub() {
        assert!(!belongs_to_the_peripheral(r"USB\ROOT_HUB30\4&1234&0&0", "USB", &[]));
    }

    #[test]
    fn stops_at_the_host_controller() {
        assert!(!belongs_to_the_peripheral(r"PCI\VEN_8086&DEV_A36D\3&11583659&0&A0", "PCI", &[]));
    }
}
