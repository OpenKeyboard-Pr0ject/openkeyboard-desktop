use std::collections::BTreeMap;

use crate::usb_transfers::UsbTransfer;

const GET_DESCRIPTOR_REQUEST_TYPE: u8 = 0x80;
const GET_DESCRIPTOR_REQUEST: u8 = 0x06;
const DEVICE_DESCRIPTOR_TYPE: u8 = 0x01;
const CONFIGURATION_DESCRIPTOR_TYPE: u8 = 0x02;
const INTERFACE_DESCRIPTOR_TYPE: u8 = 0x04;
const ENDPOINT_DESCRIPTOR_TYPE: u8 = 0x05;
const DEVICE_DESCRIPTOR_MINIMUM_BYTES: usize = 14;

pub type UsbAddress = (u16, u16);

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct UsbDeviceDescription {
    pub vendor_id: u16,
    pub product_id: u16,
    pub release_number: u16,
    pub endpoint_interfaces: BTreeMap<u8, u8>,
}

impl UsbDeviceDescription {
    pub fn interface_of_endpoint(&self, endpoint: u8) -> Option<u8> {
        self.endpoint_interfaces.get(&endpoint).copied()
    }
}

pub fn describe_devices(transfers: &[UsbTransfer]) -> BTreeMap<UsbAddress, UsbDeviceDescription> {
    let mut identities: BTreeMap<UsbAddress, (u16, u16, u16)> = BTreeMap::new();
    let mut configurations: BTreeMap<UsbAddress, Vec<u8>> = BTreeMap::new();
    for transfer in transfers {
        let UsbTransfer::Control { setup, in_data, .. } = transfer else {
            continue;
        };
        if setup.request_type != GET_DESCRIPTOR_REQUEST_TYPE || setup.request != GET_DESCRIPTOR_REQUEST {
            continue;
        }
        let address = transfer.device_address();
        match (setup.value >> 8) as u8 {
            DEVICE_DESCRIPTOR_TYPE => {
                if let Some(identity) = device_identity(in_data) {
                    identities.insert(address, identity);
                }
            }
            CONFIGURATION_DESCRIPTOR_TYPE => {
                let known = configurations.entry(address).or_default();
                if in_data.len() > known.len() {
                    *known = in_data.clone();
                }
            }
            _ => {}
        }
    }
    identities
        .into_iter()
        .map(|(address, (vendor_id, product_id, release_number))| {
            let endpoint_interfaces =
                configurations.get(&address).map(|bytes| endpoint_interfaces(bytes)).unwrap_or_default();
            (address, UsbDeviceDescription { vendor_id, product_id, release_number, endpoint_interfaces })
        })
        .collect()
}

fn device_identity(descriptor: &[u8]) -> Option<(u16, u16, u16)> {
    if descriptor.len() < DEVICE_DESCRIPTOR_MINIMUM_BYTES || descriptor[1] != DEVICE_DESCRIPTOR_TYPE {
        return None;
    }
    let field = |offset: usize| u16::from_le_bytes([descriptor[offset], descriptor[offset + 1]]);
    Some((field(8), field(10), field(12)))
}

fn endpoint_interfaces(configuration: &[u8]) -> BTreeMap<u8, u8> {
    let mut endpoints = BTreeMap::new();
    let mut current_interface = None;
    let mut offset = 0;
    while let Some(&length) = configuration.get(offset) {
        if length < 2 {
            break;
        }
        let descriptor = &configuration[offset..(offset + usize::from(length)).min(configuration.len())];
        match descriptor.get(1).copied() {
            Some(INTERFACE_DESCRIPTOR_TYPE) => current_interface = descriptor.get(2).copied(),
            Some(ENDPOINT_DESCRIPTOR_TYPE) => {
                if let (Some(interface), Some(&endpoint)) = (current_interface, descriptor.get(2)) {
                    endpoints.insert(endpoint, interface);
                }
            }
            _ => {}
        }
        offset += usize::from(length);
    }
    endpoints
}

#[cfg(test)]
pub mod test_support {
    pub fn device_descriptor(vendor_id: u16, product_id: u16, release_number: u16) -> Vec<u8> {
        let mut descriptor = vec![18, 0x01, 0x00, 0x02, 0x00, 0x00, 0x00, 0x40];
        descriptor.extend_from_slice(&vendor_id.to_le_bytes());
        descriptor.extend_from_slice(&product_id.to_le_bytes());
        descriptor.extend_from_slice(&release_number.to_le_bytes());
        descriptor.extend_from_slice(&[0x01, 0x02, 0x00, 0x01]);
        descriptor
    }

    pub fn configuration_descriptor(interfaces: &[(u8, &[u8])]) -> Vec<u8> {
        let mut body = Vec::new();
        for (interface_number, endpoints) in interfaces {
            body.extend_from_slice(&[9, 0x04, *interface_number, 0, endpoints.len() as u8, 0x03, 0x00, 0x00, 0]);
            body.extend_from_slice(&[9, 0x21, 0x11, 0x01, 0x00, 0x01, 0x22, 0x40, 0x00]);
            for endpoint in *endpoints {
                body.extend_from_slice(&[7, 0x05, *endpoint, 0x03, 0x40, 0x00, 0x01]);
            }
        }
        let total_length = (9 + body.len()) as u16;
        let mut descriptor = vec![9, 0x02];
        descriptor.extend_from_slice(&total_length.to_le_bytes());
        descriptor.extend_from_slice(&[interfaces.len() as u8, 0x01, 0x00, 0xa0, 0xfa]);
        descriptor.extend_from_slice(&body);
        descriptor
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::*;
    use super::*;
    use crate::usb_transfers::assemble_transfers;
    use crate::usb_transfers::test_support::{control_records, parsed, setup_bytes};

    fn descriptor_transfers(address: u16, device: Vec<u8>, configuration: Vec<u8>) -> Vec<UsbTransfer> {
        let mut records = control_records(10, 1, address, setup_bytes(0x80, 0x06, 0x0100, 0, 18), &[], &device);
        records.extend(control_records(
            20,
            2,
            address,
            setup_bytes(0x80, 0x06, 0x0200, 0, 9),
            &[],
            &configuration[..9],
        ));
        records.extend(control_records(
            30,
            3,
            address,
            setup_bytes(0x80, 0x06, 0x0200, 0, configuration.len() as u16),
            &[],
            &configuration,
        ));
        assemble_transfers(&parsed(&records))
    }

    #[test]
    fn identifies_a_device_by_its_injected_descriptors() {
        let configuration = configuration_descriptor(&[(0, &[0x81]), (2, &[0x83, 0x03]), (4, &[0x85])]);
        let transfers = descriptor_transfers(7, device_descriptor(0x31e3, 0x1402, 0x0250), configuration);

        let devices = describe_devices(&transfers);

        let wooting = &devices[&(1, 7)];
        assert_eq!((wooting.vendor_id, wooting.product_id, wooting.release_number), (0x31e3, 0x1402, 0x0250));
        assert_eq!(wooting.interface_of_endpoint(0x83), Some(2));
        assert_eq!(wooting.interface_of_endpoint(0x03), Some(2));
        assert_eq!(wooting.interface_of_endpoint(0x85), Some(4));
        assert_eq!(wooting.interface_of_endpoint(0x86), None);
    }

    #[test]
    fn ignores_a_short_device_descriptor() {
        let short_device_descriptor = device_descriptor(0x31e3, 0x1402, 0x0250)[..8].to_vec();
        let transfers = descriptor_transfers(7, short_device_descriptor, configuration_descriptor(&[]));

        assert!(describe_devices(&transfers).is_empty());
    }
}
