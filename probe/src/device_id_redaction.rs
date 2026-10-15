const REDACTED: &str = "<redacted>";
const ADDRESS_HEX_DIGITS: usize = 12;
const INSTANCE_ID_SEPARATOR: char = '\\';

pub fn redact_instance_id(instance_id: &str) -> String {
    let mut parts = instance_id.splitn(3, INSTANCE_ID_SEPARATOR);
    let enumerator = parts.next().unwrap_or_default();
    match (parts.next(), parts.next()) {
        (Some(device_id), Some(_)) => format!("{enumerator}\\{}\\{REDACTED}", redact_addresses(device_id)),
        (Some(device_id), None) => format!("{enumerator}\\{}", redact_addresses(device_id)),
        _ => redact_addresses(enumerator),
    }
}

pub fn redact_addresses(device_id: &str) -> String {
    let mut redacted = String::with_capacity(device_id.len());
    let mut alphanumeric_run = String::new();
    let mut brace_depth = 0_usize;
    for character in device_id.chars() {
        if brace_depth == 0 && character.is_ascii_alphanumeric() {
            alphanumeric_run.push(character);
            continue;
        }
        append_run(&mut redacted, &mut alphanumeric_run);
        match character {
            '{' => brace_depth += 1,
            '}' => brace_depth = brace_depth.saturating_sub(1),
            _ => {}
        }
        redacted.push(character);
    }
    append_run(&mut redacted, &mut alphanumeric_run);
    redacted
}

fn append_run(target: &mut String, run: &mut String) {
    if looks_like_hardware_address(run) {
        target.push_str(REDACTED);
    } else {
        target.push_str(run);
    }
    run.clear();
}

fn looks_like_hardware_address(run: &str) -> bool {
    run.len() == ADDRESS_HEX_DIGITS && run.chars().all(|character| character.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_the_usb_serial_number() {
        assert_eq!(redact_instance_id(r"USB\VID_31E3&PID_1402\A1B2C3D4E5"), r"USB\VID_31E3&PID_1402\<redacted>");
    }

    #[test]
    fn removes_the_instance_part_of_a_hid_collection() {
        assert_eq!(
            redact_instance_id(r"HID\VID_1532&PID_0258&MI_03&COL01\7&2D2A1B3C&0&0000"),
            r"HID\VID_1532&PID_0258&MI_03&COL01\<redacted>"
        );
    }

    #[test]
    fn removes_a_bluetooth_address_from_the_device_id() {
        assert_eq!(
            redact_instance_id(r"BTHLE\DEV_A4C138E1F2A3\8&1234&0&A4C138E1F2A3"),
            r"BTHLE\DEV_<redacted>\<redacted>"
        );
    }

    #[test]
    fn keeps_service_guids_intact() {
        let hardware_id = r"HID\{00001812-0000-1000-8000-00805f9b34fb}_Dev_VID&02046d_PID&b023_REV&0007_a4c138e1f2a3";

        assert_eq!(
            redact_addresses(hardware_id),
            r"HID\{00001812-0000-1000-8000-00805f9b34fb}_Dev_VID&02046d_PID&b023_REV&0007_<redacted>"
        );
    }

    #[test]
    fn keeps_ordinary_hardware_ids() {
        assert_eq!(redact_addresses(r"USB\VID_1532&PID_0258&REV_0200&MI_03"), r"USB\VID_1532&PID_0258&REV_0200&MI_03");
    }

    #[test]
    fn keeps_a_bare_enumerator() {
        assert_eq!(redact_instance_id("ROOT"), "ROOT");
    }
}
