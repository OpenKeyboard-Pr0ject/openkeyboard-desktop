pub fn hex_u16(value: u16) -> String {
    format!("0x{value:04x}")
}

pub fn hex_bytes(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
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
    fn writes_bytes_without_separators() {
        assert_eq!(hex_bytes(&[0x05, 0x0c, 0xa1]), "050ca1");
        assert_eq!(hex_bytes(&[]), "");
    }
}
