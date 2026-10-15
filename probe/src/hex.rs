pub fn hex_u16(value: u16) -> String {
    format!("0x{value:04x}")
}

pub fn hex_bytes(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn trimmed_hex(bytes: &[u8]) -> String {
    let used_length = bytes.iter().rposition(|byte| *byte != 0).map_or(0, |last| last + 1);
    hex_bytes(&bytes[..used_length])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_ids_with_four_lowercase_digits() {
        assert_eq!(hex_u16(0x31e3), "0x31e3");
        assert_eq!(hex_u16(0x0c), "0x000c");
    }

    #[test]
    fn trims_trailing_zero_bytes() {
        assert_eq!(trimmed_hex(&[0xd1, 0xda, 0x01, 0x00, 0x00]), "d1da01");
        assert_eq!(trimmed_hex(&[0x00, 0x00]), "");
    }

    #[test]
    fn writes_bytes_without_separators() {
        assert_eq!(hex_bytes(&[0x05, 0x0c, 0xa1]), "050ca1");
        assert_eq!(hex_bytes(&[]), "");
    }
}
