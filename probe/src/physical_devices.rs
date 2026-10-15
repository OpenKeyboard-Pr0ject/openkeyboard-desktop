use std::collections::BTreeMap;

use crate::hid_inventory::HidCollectionRecord;
use crate::read_only_queries::QueryRecord;
use crate::windows_devices::WindowsDevice;

const GENERIC_DESKTOP_USAGE_PAGE: u16 = 0x01;
const KEYBOARD_USAGE: u16 = 0x06;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredInterface {
    pub collection: HidCollectionRecord,
    pub windows_devices: Vec<WindowsDevice>,
    pub container_key: Option<String>,
    pub queries: Vec<QueryRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhysicalDevice {
    pub interfaces: Vec<DiscoveredInterface>,
}

impl PhysicalDevice {
    pub fn identity(&self) -> &HidCollectionRecord {
        &self.interfaces[0].collection
    }

    pub fn has_keyboard_collection(&self) -> bool {
        self.interfaces.iter().any(|interface| {
            interface.collection.usage_page == GENERIC_DESKTOP_USAGE_PAGE
                && interface.collection.usage == KEYBOARD_USAGE
        })
    }
}

type PhysicalDeviceKey = (u16, u16, Option<String>);

pub fn group_into_physical_devices(interfaces: Vec<DiscoveredInterface>) -> Vec<PhysicalDevice> {
    let mut groups: BTreeMap<PhysicalDeviceKey, Vec<DiscoveredInterface>> = BTreeMap::new();
    for interface in interfaces {
        let key = (interface.collection.vendor_id, interface.collection.product_id, interface.container_key.clone());
        groups.entry(key).or_default().push(interface);
    }
    groups
        .into_values()
        .map(|mut interfaces| {
            interfaces.sort_by_key(|interface| {
                (interface.collection.interface_number, interface.collection.usage_page, interface.collection.usage)
            });
            PhysicalDevice { interfaces }
        })
        .collect()
}

#[cfg(test)]
pub mod test_support {
    use super::*;

    pub fn collection(vendor_id: u16, product_id: u16, usage_page: u16, usage: u16) -> HidCollectionRecord {
        HidCollectionRecord {
            path: format!(r"\\?\HID#VID_{vendor_id:04X}&PID_{product_id:04X}#{usage_page:04x}{usage:04x}"),
            vendor_id,
            product_id,
            release_number: 0x0250,
            manufacturer: Some("Wooting".to_string()),
            product: Some("Wooting 80HE TenZ".to_string()),
            usage_page,
            usage,
            interface_number: 0,
            bus: "usb".to_string(),
            report_descriptor: Ok(vec![0x06, 0x55, 0xff, 0x09, 0x01, 0xa1, 0x01, 0xc0]),
        }
    }

    pub fn interface(collection: HidCollectionRecord, container_key: Option<&str>) -> DiscoveredInterface {
        DiscoveredInterface {
            collection,
            windows_devices: Vec::new(),
            container_key: container_key.map(str::to_string),
            queries: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::{collection, interface};
    use super::*;

    #[test]
    fn groups_the_collections_of_one_keyboard() {
        let devices = group_into_physical_devices(vec![
            interface(collection(0x31e3, 0x1402, 0xff55, 0x01), Some("a")),
            interface(collection(0x31e3, 0x1402, 0x01, 0x06), Some("a")),
            interface(collection(0x046d, 0xc52b, 0x01, 0x02), Some("b")),
        ]);

        assert_eq!(devices.len(), 2);
        assert_eq!(devices[1].interfaces.len(), 2);
    }

    #[test]
    fn keeps_two_identical_keyboards_apart() {
        let devices = group_into_physical_devices(vec![
            interface(collection(0x31e3, 0x1402, 0x01, 0x06), Some("first")),
            interface(collection(0x31e3, 0x1402, 0x01, 0x06), Some("second")),
        ]);

        assert_eq!(devices.len(), 2);
    }

    #[test]
    fn orders_interfaces_by_number_then_usage() {
        let mut vendor = collection(0x31e3, 0x1402, 0xff55, 0x01);
        vendor.interface_number = 2;
        let keyboard = collection(0x31e3, 0x1402, 0x01, 0x06);
        let devices = group_into_physical_devices(vec![interface(vendor, None), interface(keyboard, None)]);

        assert_eq!(devices[0].identity().usage_page, 0x01);
    }

    #[test]
    fn recognizes_a_keyboard_collection() {
        let keyboard = group_into_physical_devices(vec![interface(collection(0x31e3, 0x1402, 0x01, 0x06), None)]);
        let mouse = group_into_physical_devices(vec![interface(collection(0x046d, 0xc52b, 0x01, 0x02), None)]);

        assert!(keyboard[0].has_keyboard_collection());
        assert!(!mouse[0].has_keyboard_collection());
    }
}
