use crate::hex::hex_u16;
use crate::physical_devices::PhysicalDevice;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DeviceSelection {
    pub every_device: bool,
    pub vendor_id: Option<u16>,
    pub product_id: Option<u16>,
}

impl DeviceSelection {
    pub fn selects(&self, device: &PhysicalDevice) -> bool {
        let identity = device.identity();
        if self.vendor_id.is_some_and(|vendor_id| vendor_id != identity.vendor_id) {
            return false;
        }
        if self.product_id.is_some_and(|product_id| product_id != identity.product_id) {
            return false;
        }
        self.every_device || self.names_a_device() || device.has_keyboard_collection()
    }

    pub fn describe(&self) -> String {
        if self.names_a_device() {
            let vendor = self.vendor_id.map(|vendor_id| format!("vendor {}", hex_u16(vendor_id)));
            let product = self.product_id.map(|product_id| format!("product {}", hex_u16(product_id)));
            return [vendor, product].into_iter().flatten().collect::<Vec<_>>().join(", ");
        }
        if self.every_device { "every HID device" } else { "devices with a keyboard collection" }.to_string()
    }

    fn names_a_device(&self) -> bool {
        self.vendor_id.is_some() || self.product_id.is_some()
    }
}

pub fn select_devices(devices: Vec<PhysicalDevice>, selection: DeviceSelection) -> Vec<PhysicalDevice> {
    devices.into_iter().filter(|device| selection.selects(device)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::physical_devices::group_into_physical_devices;
    use crate::physical_devices::test_support::{collection, interface};

    fn device(vendor_id: u16, product_id: u16, usage_page: u16, usage: u16) -> PhysicalDevice {
        group_into_physical_devices(vec![interface(collection(vendor_id, product_id, usage_page, usage), None)])
            .remove(0)
    }

    #[test]
    fn selects_keyboards_by_default() {
        let selection = DeviceSelection::default();

        assert!(selection.selects(&device(0x31e3, 0x1402, 0x01, 0x06)));
        assert!(!selection.selects(&device(0x046d, 0xc52b, 0x01, 0x02)));
    }

    #[test]
    fn selects_everything_when_asked() {
        let selection = DeviceSelection { every_device: true, ..DeviceSelection::default() };

        assert!(selection.selects(&device(0x046d, 0xc52b, 0x01, 0x02)));
    }

    #[test]
    fn selects_a_named_vendor_even_without_a_keyboard_collection() {
        let selection = DeviceSelection { vendor_id: Some(0x1532), ..DeviceSelection::default() };

        assert!(selection.selects(&device(0x1532, 0x0271, 0xff00, 0x01)));
        assert!(!selection.selects(&device(0x31e3, 0x1402, 0x01, 0x06)));
    }

    #[test]
    fn narrows_to_a_product() {
        let selection = DeviceSelection { vendor_id: Some(0x1532), product_id: Some(0x0258), every_device: false };

        assert!(selection.selects(&device(0x1532, 0x0258, 0x0c, 0x01)));
        assert!(!selection.selects(&device(0x1532, 0x0271, 0x0c, 0x01)));
    }

    #[test]
    fn keeps_only_selected_devices() {
        let devices = vec![device(0x31e3, 0x1402, 0x01, 0x06), device(0x046d, 0xc52b, 0x01, 0x02)];

        let selected = select_devices(devices, DeviceSelection::default());

        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].identity().vendor_id, 0x31e3);
    }

    #[test]
    fn describes_itself() {
        assert_eq!(DeviceSelection::default().describe(), "devices with a keyboard collection");
        assert_eq!(DeviceSelection { every_device: true, ..DeviceSelection::default() }.describe(), "every HID device");
        let named = DeviceSelection { vendor_id: Some(0x1532), product_id: Some(0x0258), every_device: false };
        assert_eq!(named.describe(), "vendor 0x1532, product 0x0258");
    }
}
