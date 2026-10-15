use std::ptr::null_mut;

use windows_sys::Win32::Devices::DeviceAndDriverInstallation::{
    CM_Get_DevNode_PropertyW, CM_Get_Device_Interface_PropertyW, CM_Get_Parent, CM_LOCATE_DEVNODE_NORMAL,
    CM_Locate_DevNodeW, CONFIGRET, CR_BUFFER_SMALL, CR_SUCCESS,
};
use windows_sys::Win32::Devices::Properties::{
    DEVPKEY_Device_BusReportedDeviceDesc, DEVPKEY_Device_CompatibleIds, DEVPKEY_Device_ContainerId,
    DEVPKEY_Device_DeviceDesc, DEVPKEY_Device_DriverProvider, DEVPKEY_Device_DriverVersion,
    DEVPKEY_Device_EnumeratorName, DEVPKEY_Device_HardwareIds, DEVPKEY_Device_InstanceId, DEVPKEY_Device_LowerFilters,
    DEVPKEY_Device_Manufacturer, DEVPKEY_Device_Service, DEVPKEY_Device_UpperFilters, DEVPROP_TYPE_STRING,
    DEVPROP_TYPE_STRING_LIST, DEVPROPTYPE,
};
use windows_sys::Win32::Foundation::DEVPROPKEY;

use crate::device_id_redaction::{redact_addresses, redact_instance_id};
use crate::hex::hex_bytes;
use crate::windows_devices::{WindowsDevice, WindowsDeviceLookup, belongs_to_the_peripheral};

const MAXIMUM_TREE_DEPTH: usize = 6;

type DeviceNode = u32;

struct PropertyValue {
    property_type: DEVPROPTYPE,
    bytes: Vec<u8>,
}

impl PropertyValue {
    fn text(&self) -> Option<String> {
        if self.property_type != DEVPROP_TYPE_STRING {
            return None;
        }
        utf16_strings(&self.bytes).into_iter().next()
    }

    fn text_list(&self) -> Vec<String> {
        if self.property_type != DEVPROP_TYPE_STRING_LIST && self.property_type != DEVPROP_TYPE_STRING {
            return Vec::new();
        }
        utf16_strings(&self.bytes)
    }
}

pub fn look_up_windows_devices(device_path: &str) -> WindowsDeviceLookup {
    let Some(first_node) = interface_instance_id(device_path).and_then(|instance_id| locate_device_node(&instance_id))
    else {
        return WindowsDeviceLookup::default();
    };
    let container_key = node_property(first_node, &DEVPKEY_Device_ContainerId).map(|value| hex_bytes(&value.bytes));
    WindowsDeviceLookup { chain: device_chain(first_node), container_key }
}

fn device_chain(first_node: DeviceNode) -> Vec<WindowsDevice> {
    let mut chain = Vec::new();
    let mut next_node = Some(first_node);
    while let Some(node) = next_node {
        if chain.len() == MAXIMUM_TREE_DEPTH {
            break;
        }
        let Some(device) = describe_peripheral_node(node) else {
            break;
        };
        chain.push(device);
        next_node = parent_of(node);
    }
    chain
}

fn describe_peripheral_node(node: DeviceNode) -> Option<WindowsDevice> {
    let raw_instance_id = node_text(node, &DEVPKEY_Device_InstanceId)?;
    let enumerator = node_text(node, &DEVPKEY_Device_EnumeratorName).unwrap_or_default();
    let compatible_ids = node_text_list(node, &DEVPKEY_Device_CompatibleIds);
    if !belongs_to_the_peripheral(&raw_instance_id, &enumerator, &compatible_ids) {
        return None;
    }
    Some(WindowsDevice {
        instance_id: redact_instance_id(&raw_instance_id),
        enumerator,
        description: node_text(node, &DEVPKEY_Device_DeviceDesc),
        bus_reported_description: node_text(node, &DEVPKEY_Device_BusReportedDeviceDesc),
        manufacturer: node_text(node, &DEVPKEY_Device_Manufacturer),
        service: node_text(node, &DEVPKEY_Device_Service),
        lower_filters: node_text_list(node, &DEVPKEY_Device_LowerFilters),
        upper_filters: node_text_list(node, &DEVPKEY_Device_UpperFilters),
        hardware_ids: redacted_ids(node_text_list(node, &DEVPKEY_Device_HardwareIds)),
        compatible_ids: redacted_ids(compatible_ids),
        driver_provider: node_text(node, &DEVPKEY_Device_DriverProvider),
        driver_version: node_text(node, &DEVPKEY_Device_DriverVersion),
    })
}

fn redacted_ids(ids: Vec<String>) -> Vec<String> {
    ids.iter().map(|id| redact_addresses(id)).collect()
}

fn interface_instance_id(device_path: &str) -> Option<String> {
    let wide_path = wide_null_terminated(device_path);
    read_property(|property_type, buffer, size| unsafe {
        CM_Get_Device_Interface_PropertyW(
            wide_path.as_ptr(),
            &DEVPKEY_Device_InstanceId,
            property_type,
            buffer,
            size,
            0,
        )
    })
    .and_then(|value| value.text())
}

fn locate_device_node(instance_id: &str) -> Option<DeviceNode> {
    let wide_instance_id = wide_null_terminated(instance_id);
    let mut node: DeviceNode = 0;
    let result = unsafe { CM_Locate_DevNodeW(&mut node, wide_instance_id.as_ptr(), CM_LOCATE_DEVNODE_NORMAL) };
    (result == CR_SUCCESS).then_some(node)
}

fn parent_of(node: DeviceNode) -> Option<DeviceNode> {
    let mut parent: DeviceNode = 0;
    let result = unsafe { CM_Get_Parent(&mut parent, node, 0) };
    (result == CR_SUCCESS).then_some(parent)
}

fn node_text(node: DeviceNode, key: &DEVPROPKEY) -> Option<String> {
    node_property(node, key).and_then(|value| value.text()).filter(|text| !text.is_empty())
}

fn node_text_list(node: DeviceNode, key: &DEVPROPKEY) -> Vec<String> {
    node_property(node, key).map(|value| value.text_list()).unwrap_or_default()
}

fn node_property(node: DeviceNode, key: &DEVPROPKEY) -> Option<PropertyValue> {
    read_property(|property_type, buffer, size| unsafe {
        CM_Get_DevNode_PropertyW(node, key, property_type, buffer, size, 0)
    })
}

fn read_property(query: impl Fn(*mut DEVPROPTYPE, *mut u8, *mut u32) -> CONFIGRET) -> Option<PropertyValue> {
    let mut property_type: DEVPROPTYPE = 0;
    let mut size: u32 = 0;
    if query(&mut property_type, null_mut(), &mut size) != CR_BUFFER_SMALL || size == 0 {
        return None;
    }
    let mut bytes = vec![0_u8; size as usize];
    if query(&mut property_type, bytes.as_mut_ptr(), &mut size) != CR_SUCCESS {
        return None;
    }
    bytes.truncate(size as usize);
    Some(PropertyValue { property_type, bytes })
}

fn wide_null_terminated(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

fn utf16_strings(bytes: &[u8]) -> Vec<String> {
    let units: Vec<u16> = bytes.as_chunks::<2>().0.iter().map(|pair| u16::from_le_bytes(*pair)).collect();
    units.split(|unit| *unit == 0).filter(|part| !part.is_empty()).map(String::from_utf16_lossy).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utf16_bytes(text: &str) -> Vec<u8> {
        text.encode_utf16().flat_map(u16::to_le_bytes).collect()
    }

    #[test]
    fn reads_a_single_string() {
        assert_eq!(utf16_strings(&utf16_bytes("HidUsb\0")), vec!["HidUsb"]);
    }

    #[test]
    fn reads_a_string_list() {
        assert_eq!(utf16_strings(&utf16_bytes("RzDev_0258\0kbdclass\0\0")), vec!["RzDev_0258", "kbdclass"]);
    }

    #[test]
    fn ignores_a_trailing_odd_byte() {
        let mut bytes = utf16_bytes("USB\0");
        bytes.push(0x41);

        assert_eq!(utf16_strings(&bytes), vec!["USB"]);
    }
}
