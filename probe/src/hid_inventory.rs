use std::collections::HashMap;

use hidapi::{BusType, DeviceInfo, HidApi};

const MAXIMUM_DESCRIPTOR_BYTES: usize = 4096;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HidCollectionRecord {
    pub path: String,
    pub vendor_id: u16,
    pub product_id: u16,
    pub release_number: u16,
    pub manufacturer: Option<String>,
    pub product: Option<String>,
    pub usage_page: u16,
    pub usage: u16,
    pub interface_number: i32,
    pub bus: String,
    pub report_descriptor: Result<Vec<u8>, String>,
}

pub fn enumerate_hid_collections(api: &HidApi) -> Vec<HidCollectionRecord> {
    let mut descriptors_by_path: HashMap<String, Result<Vec<u8>, String>> = HashMap::new();
    api.device_list()
        .map(|info| {
            let path = info.path().to_string_lossy().into_owned();
            let report_descriptor =
                descriptors_by_path.entry(path.clone()).or_insert_with(|| read_report_descriptor(api, info)).clone();
            record_of(info, path, report_descriptor)
        })
        .collect()
}

fn read_report_descriptor(api: &HidApi, info: &DeviceInfo) -> Result<Vec<u8>, String> {
    let device = api.open_path(info.path()).map_err(|error| format!("Could not open the collection: {error}"))?;
    let mut descriptor = vec![0; MAXIMUM_DESCRIPTOR_BYTES];
    let length = device
        .get_report_descriptor(&mut descriptor)
        .map_err(|error| format!("Could not read the report descriptor: {error}"))?;
    descriptor.truncate(length);
    Ok(descriptor)
}

fn record_of(info: &DeviceInfo, path: String, report_descriptor: Result<Vec<u8>, String>) -> HidCollectionRecord {
    HidCollectionRecord {
        path,
        vendor_id: info.vendor_id(),
        product_id: info.product_id(),
        release_number: info.release_number(),
        manufacturer: non_empty(info.manufacturer_string()),
        product: non_empty(info.product_string()),
        usage_page: info.usage_page(),
        usage: info.usage(),
        interface_number: info.interface_number(),
        bus: bus_name(info.bus_type()).to_string(),
        report_descriptor,
    }
}

fn non_empty(text: Option<&str>) -> Option<String> {
    text.map(str::trim).filter(|trimmed| !trimmed.is_empty()).map(str::to_string)
}

fn bus_name(bus: BusType) -> &'static str {
    match bus {
        BusType::Usb => "usb",
        BusType::Bluetooth => "bluetooth",
        BusType::I2c => "i2c",
        BusType::Spi => "spi",
        BusType::Unknown => "unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drops_blank_strings() {
        assert_eq!(non_empty(Some("  ")), None);
        assert_eq!(non_empty(Some(" Wooting ")), Some("Wooting".to_string()));
        assert_eq!(non_empty(None), None);
    }

    #[test]
    fn names_each_bus() {
        assert_eq!(bus_name(BusType::Usb), "usb");
        assert_eq!(bus_name(BusType::Bluetooth), "bluetooth");
    }
}
