pub const LINKTYPE_USBPCAP: u32 = 249;

const PCAP_MICROSECOND_MAGIC: u32 = 0xa1b2_c3d4;
const PCAP_NANOSECOND_MAGIC: u32 = 0xa1b2_3c4d;
const PCAP_GLOBAL_HEADER_BYTES: usize = 24;
const PCAP_RECORD_HEADER_BYTES: usize = 16;
const PCAP_LINK_TYPE_OFFSET: usize = 20;

const PCAPNG_SECTION_HEADER: u32 = 0x0a0d_0d0a;
const PCAPNG_BYTE_ORDER_MAGIC: u32 = 0x1a2b_3c4d;
const PCAPNG_INTERFACE_DESCRIPTION: u32 = 1;
const PCAPNG_SIMPLE_PACKET: u32 = 3;
const PCAPNG_ENHANCED_PACKET: u32 = 6;
const PCAPNG_BLOCK_OVERHEAD_BYTES: usize = 12;
const PCAPNG_TIMESTAMP_RESOLUTION_OPTION: u16 = 9;
const PCAPNG_END_OF_OPTIONS: u16 = 0;
const PCAPNG_BINARY_RESOLUTION_FLAG: u8 = 0x80;
const DEFAULT_TIMESTAMP_UNITS_PER_SECOND: u128 = 1_000_000;

const MICROSECONDS_PER_SECOND: u128 = 1_000_000;
const NANOSECONDS_PER_MICROSECOND: u64 = 1_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedPacket {
    pub link_type: u32,
    pub timestamp_micros: u64,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ByteOrder {
    Little,
    Big,
}

impl ByteOrder {
    fn u16_at(self, bytes: &[u8], offset: usize) -> Option<u16> {
        let field: [u8; 2] = bytes.get(offset..offset + 2)?.try_into().ok()?;
        Some(match self {
            Self::Little => u16::from_le_bytes(field),
            Self::Big => u16::from_be_bytes(field),
        })
    }

    fn u32_at(self, bytes: &[u8], offset: usize) -> Option<u32> {
        let field: [u8; 4] = bytes.get(offset..offset + 4)?.try_into().ok()?;
        Some(match self {
            Self::Little => u32::from_le_bytes(field),
            Self::Big => u32::from_be_bytes(field),
        })
    }
}

pub fn read_capture(bytes: &[u8]) -> Result<Vec<CapturedPacket>, String> {
    let little_endian_magic = ByteOrder::Little.u32_at(bytes, 0).ok_or("The capture file is empty.")?;
    match little_endian_magic {
        PCAPNG_SECTION_HEADER => Ok(read_pcapng(bytes)),
        _ => read_pcap(bytes),
    }
}

fn read_pcap(bytes: &[u8]) -> Result<Vec<CapturedPacket>, String> {
    let (byte_order, nanosecond_timestamps) = pcap_format(bytes)?;
    let link_type = byte_order.u32_at(bytes, PCAP_LINK_TYPE_OFFSET).ok_or("The pcap header is incomplete.")?;
    let mut packets = Vec::new();
    let mut offset = PCAP_GLOBAL_HEADER_BYTES;
    while let (Some(seconds), Some(fraction), Some(captured_length)) =
        (byte_order.u32_at(bytes, offset), byte_order.u32_at(bytes, offset + 4), byte_order.u32_at(bytes, offset + 8))
    {
        let data_start = offset + PCAP_RECORD_HEADER_BYTES;
        let Some(data) = bytes.get(data_start..data_start + captured_length as usize) else {
            break;
        };
        let fraction_micros =
            if nanosecond_timestamps { u64::from(fraction) / NANOSECONDS_PER_MICROSECOND } else { u64::from(fraction) };
        packets.push(CapturedPacket {
            link_type,
            timestamp_micros: u64::from(seconds) * MICROSECONDS_PER_SECOND as u64 + fraction_micros,
            data: data.to_vec(),
        });
        offset = data_start + captured_length as usize;
    }
    Ok(packets)
}

fn pcap_format(bytes: &[u8]) -> Result<(ByteOrder, bool), String> {
    for byte_order in [ByteOrder::Little, ByteOrder::Big] {
        match byte_order.u32_at(bytes, 0) {
            Some(PCAP_MICROSECOND_MAGIC) => return Ok((byte_order, false)),
            Some(PCAP_NANOSECOND_MAGIC) => return Ok((byte_order, true)),
            _ => {}
        }
    }
    Err("The file is neither a pcap nor a pcapng capture.".to_string())
}

struct PcapngInterface {
    link_type: u32,
    units_per_second: u128,
}

fn read_pcapng(bytes: &[u8]) -> Vec<CapturedPacket> {
    let mut packets = Vec::new();
    let mut interfaces: Vec<PcapngInterface> = Vec::new();
    let mut byte_order = ByteOrder::Little;
    let mut offset = 0;
    while let Some(block_type) = byte_order.u32_at(bytes, offset) {
        if block_type == PCAPNG_SECTION_HEADER {
            let Some(order) = section_byte_order(bytes, offset) else {
                break;
            };
            byte_order = order;
            interfaces.clear();
        }
        let Some(block_length) = byte_order.u32_at(bytes, offset + 4).map(|length| length as usize) else {
            break;
        };
        if block_length < PCAPNG_BLOCK_OVERHEAD_BYTES {
            break;
        }
        let Some(body) = bytes.get(offset + 8..offset + block_length - 4) else {
            break;
        };
        match block_type {
            PCAPNG_INTERFACE_DESCRIPTION => {
                if let Some(interface) = interface_of(body, byte_order) {
                    interfaces.push(interface);
                }
            }
            PCAPNG_ENHANCED_PACKET => {
                if let Some(packet) = enhanced_packet_of(body, byte_order, &interfaces) {
                    packets.push(packet);
                }
            }
            PCAPNG_SIMPLE_PACKET => {
                if let Some(packet) = simple_packet_of(body, byte_order, &interfaces) {
                    packets.push(packet);
                }
            }
            _ => {}
        }
        offset += block_length;
    }
    packets
}

fn section_byte_order(bytes: &[u8], offset: usize) -> Option<ByteOrder> {
    [ByteOrder::Little, ByteOrder::Big]
        .into_iter()
        .find(|order| order.u32_at(bytes, offset + 8) == Some(PCAPNG_BYTE_ORDER_MAGIC))
}

fn interface_of(body: &[u8], byte_order: ByteOrder) -> Option<PcapngInterface> {
    let link_type = u32::from(byte_order.u16_at(body, 0)?);
    let mut units_per_second = DEFAULT_TIMESTAMP_UNITS_PER_SECOND;
    let mut offset = 8;
    while let (Some(code), Some(length)) = (byte_order.u16_at(body, offset), byte_order.u16_at(body, offset + 2)) {
        if code == PCAPNG_END_OF_OPTIONS {
            break;
        }
        let value = body.get(offset + 4..offset + 4 + usize::from(length))?;
        if code == PCAPNG_TIMESTAMP_RESOLUTION_OPTION
            && let Some(resolution) = value.first()
        {
            units_per_second = units_per_second_of(*resolution);
        }
        offset += 4 + usize::from(length).next_multiple_of(4);
    }
    Some(PcapngInterface { link_type, units_per_second })
}

fn units_per_second_of(resolution: u8) -> u128 {
    let exponent = u32::from(resolution & !PCAPNG_BINARY_RESOLUTION_FLAG);
    if resolution & PCAPNG_BINARY_RESOLUTION_FLAG == 0 { 10_u128.pow(exponent) } else { 2_u128.pow(exponent) }
}

fn enhanced_packet_of(body: &[u8], byte_order: ByteOrder, interfaces: &[PcapngInterface]) -> Option<CapturedPacket> {
    let interface = interfaces.get(byte_order.u32_at(body, 0)? as usize)?;
    let timestamp = (u64::from(byte_order.u32_at(body, 4)?) << 32) | u64::from(byte_order.u32_at(body, 8)?);
    let captured_length = byte_order.u32_at(body, 12)? as usize;
    let data = body.get(20..20 + captured_length)?;
    let timestamp_micros = u128::from(timestamp) * MICROSECONDS_PER_SECOND / interface.units_per_second;
    Some(CapturedPacket {
        link_type: interface.link_type,
        timestamp_micros: u64::try_from(timestamp_micros).ok()?,
        data: data.to_vec(),
    })
}

fn simple_packet_of(body: &[u8], byte_order: ByteOrder, interfaces: &[PcapngInterface]) -> Option<CapturedPacket> {
    let interface = interfaces.first()?;
    let original_length = byte_order.u32_at(body, 0)? as usize;
    let data = body.get(4..(4 + original_length).min(body.len()))?;
    Some(CapturedPacket { link_type: interface.link_type, timestamp_micros: 0, data: data.to_vec() })
}

#[cfg(test)]
pub mod test_support {
    use super::*;

    pub struct CaptureRecord {
        pub timestamp_micros: u64,
        pub data: Vec<u8>,
    }

    pub fn pcap_file(link_type: u32, records: &[CaptureRecord]) -> Vec<u8> {
        let mut file = Vec::new();
        file.extend_from_slice(&PCAP_MICROSECOND_MAGIC.to_le_bytes());
        file.extend_from_slice(&2_u16.to_le_bytes());
        file.extend_from_slice(&4_u16.to_le_bytes());
        file.extend_from_slice(&[0; 8]);
        file.extend_from_slice(&65_535_u32.to_le_bytes());
        file.extend_from_slice(&link_type.to_le_bytes());
        for record in records {
            let length = record.data.len() as u32;
            file.extend_from_slice(&((record.timestamp_micros / 1_000_000) as u32).to_le_bytes());
            file.extend_from_slice(&((record.timestamp_micros % 1_000_000) as u32).to_le_bytes());
            file.extend_from_slice(&length.to_le_bytes());
            file.extend_from_slice(&length.to_le_bytes());
            file.extend_from_slice(&record.data);
        }
        file
    }

    pub fn pcapng_file(link_type: u32, records: &[CaptureRecord]) -> Vec<u8> {
        let mut file = Vec::new();
        let mut section_body = Vec::new();
        section_body.extend_from_slice(&PCAPNG_BYTE_ORDER_MAGIC.to_le_bytes());
        section_body.extend_from_slice(&1_u16.to_le_bytes());
        section_body.extend_from_slice(&0_u16.to_le_bytes());
        section_body.extend_from_slice(&u64::MAX.to_le_bytes());
        push_block(&mut file, PCAPNG_SECTION_HEADER, &section_body);
        let mut interface_body = Vec::new();
        interface_body.extend_from_slice(&(link_type as u16).to_le_bytes());
        interface_body.extend_from_slice(&0_u16.to_le_bytes());
        interface_body.extend_from_slice(&0_u32.to_le_bytes());
        interface_body.extend_from_slice(&PCAPNG_TIMESTAMP_RESOLUTION_OPTION.to_le_bytes());
        interface_body.extend_from_slice(&1_u16.to_le_bytes());
        interface_body.extend_from_slice(&[9, 0, 0, 0]);
        interface_body.extend_from_slice(&[0; 4]);
        push_block(&mut file, PCAPNG_INTERFACE_DESCRIPTION, &interface_body);
        for record in records {
            let nanoseconds = record.timestamp_micros * NANOSECONDS_PER_MICROSECOND;
            let mut packet_body = Vec::new();
            packet_body.extend_from_slice(&0_u32.to_le_bytes());
            packet_body.extend_from_slice(&((nanoseconds >> 32) as u32).to_le_bytes());
            packet_body.extend_from_slice(&(nanoseconds as u32).to_le_bytes());
            packet_body.extend_from_slice(&(record.data.len() as u32).to_le_bytes());
            packet_body.extend_from_slice(&(record.data.len() as u32).to_le_bytes());
            packet_body.extend_from_slice(&record.data);
            packet_body.resize(packet_body.len().next_multiple_of(4), 0);
            push_block(&mut file, PCAPNG_ENHANCED_PACKET, &packet_body);
        }
        file
    }

    fn push_block(file: &mut Vec<u8>, block_type: u32, body: &[u8]) {
        let total_length = (body.len() + PCAPNG_BLOCK_OVERHEAD_BYTES) as u32;
        file.extend_from_slice(&block_type.to_le_bytes());
        file.extend_from_slice(&total_length.to_le_bytes());
        file.extend_from_slice(body);
        file.extend_from_slice(&total_length.to_le_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::*;
    use super::*;

    fn records() -> Vec<CaptureRecord> {
        vec![
            CaptureRecord { timestamp_micros: 1_792_108_800_000_001, data: vec![0x1c, 0x00, 0xaa] },
            CaptureRecord { timestamp_micros: 1_792_108_801_500_000, data: vec![0x1c, 0x00] },
        ]
    }

    #[test]
    fn reads_a_pcap_file() {
        let packets = read_capture(&pcap_file(LINKTYPE_USBPCAP, &records())).unwrap();

        assert_eq!(packets.len(), 2);
        assert_eq!(packets[0].link_type, LINKTYPE_USBPCAP);
        assert_eq!(packets[0].timestamp_micros, 1_792_108_800_000_001);
        assert_eq!(packets[1].data, vec![0x1c, 0x00]);
    }

    #[test]
    fn reads_a_pcapng_file_with_nanosecond_timestamps() {
        let packets = read_capture(&pcapng_file(LINKTYPE_USBPCAP, &records())).unwrap();

        assert_eq!(packets.len(), 2);
        assert_eq!(packets[0].timestamp_micros, 1_792_108_800_000_001);
        assert_eq!(packets[0].data, vec![0x1c, 0x00, 0xaa]);
        assert_eq!(packets[1].timestamp_micros, 1_792_108_801_500_000);
    }

    #[test]
    fn keeps_the_packets_before_a_truncated_record() {
        let mut file = pcap_file(LINKTYPE_USBPCAP, &records());
        file.truncate(file.len() - 1);

        assert_eq!(read_capture(&file).unwrap().len(), 1);
    }

    #[test]
    fn rejects_other_files() {
        assert!(read_capture(b"PK\x03\x04 not a capture").is_err());
        assert!(read_capture(&[]).is_err());
    }

    #[test]
    fn converts_binary_timestamp_resolutions() {
        assert_eq!(units_per_second_of(6), 1_000_000);
        assert_eq!(units_per_second_of(0x80 | 10), 1_024);
    }
}
