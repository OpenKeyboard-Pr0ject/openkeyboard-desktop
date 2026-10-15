use std::collections::HashMap;

use crate::capture_file::{CapturedPacket, LINKTYPE_USBPCAP};

const MINIMUM_HEADER_BYTES: usize = 27;
const CONTROL_HEADER_BYTES: usize = 28;
const COMPLETION_FLAG: u8 = 0x01;
const DIRECTION_IN_FLAG: u8 = 0x80;
const SETUP_PACKET_BYTES: usize = 8;

const TRANSFER_INTERRUPT: u8 = 1;
const TRANSFER_CONTROL: u8 = 2;

const STAGE_SETUP: u8 = 0;
const STAGE_DATA: u8 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsbPcapPacket {
    pub timestamp_micros: u64,
    pub irp_id: u64,
    pub status: u32,
    pub completion: bool,
    pub bus: u16,
    pub address: u16,
    pub endpoint: u8,
    pub transfer: u8,
    pub stage: Option<u8>,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SetupPacket {
    pub request_type: u8,
    pub request: u8,
    pub value: u16,
    pub index: u16,
    pub length: u16,
}

impl SetupPacket {
    fn from_bytes(bytes: &[u8]) -> Option<Self> {
        let field = |offset: usize| Some(u16::from_le_bytes([*bytes.get(offset)?, *bytes.get(offset + 1)?]));
        Some(Self {
            request_type: *bytes.first()?,
            request: *bytes.get(1)?,
            value: field(2)?,
            index: field(4)?,
            length: field(6)?,
        })
    }

    pub fn is_device_to_host(&self) -> bool {
        self.request_type & DIRECTION_IN_FLAG != 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UsbTransfer {
    Control {
        timestamp_micros: u64,
        bus: u16,
        address: u16,
        setup: SetupPacket,
        out_data: Vec<u8>,
        in_data: Vec<u8>,
        status: Option<u32>,
    },
    Interrupt {
        timestamp_micros: u64,
        bus: u16,
        address: u16,
        endpoint: u8,
        data: Vec<u8>,
        status: u32,
    },
}

impl UsbTransfer {
    pub fn timestamp_micros(&self) -> u64 {
        match self {
            Self::Control { timestamp_micros, .. } | Self::Interrupt { timestamp_micros, .. } => *timestamp_micros,
        }
    }

    pub fn device_address(&self) -> (u16, u16) {
        match self {
            Self::Control { bus, address, .. } | Self::Interrupt { bus, address, .. } => (*bus, *address),
        }
    }
}

pub fn parse_usbpcap_packet(packet: &CapturedPacket) -> Option<UsbPcapPacket> {
    if packet.link_type != LINKTYPE_USBPCAP {
        return None;
    }
    let bytes = &packet.data;
    let u16_at = |offset: usize| Some(u16::from_le_bytes(bytes.get(offset..offset + 2)?.try_into().ok()?));
    let u32_at = |offset: usize| Some(u32::from_le_bytes(bytes.get(offset..offset + 4)?.try_into().ok()?));
    let header_length = usize::from(u16_at(0)?);
    if header_length < MINIMUM_HEADER_BYTES {
        return None;
    }
    let transfer = *bytes.get(22)?;
    let stage = (transfer == TRANSFER_CONTROL && header_length >= CONTROL_HEADER_BYTES).then(|| bytes[27]);
    let data_length = u32_at(23)? as usize;
    let data_start = header_length.min(bytes.len());
    let data_end = (header_length + data_length).min(bytes.len());
    Some(UsbPcapPacket {
        timestamp_micros: packet.timestamp_micros,
        irp_id: u64::from_le_bytes(bytes.get(2..10)?.try_into().ok()?),
        status: u32_at(10)?,
        completion: bytes.get(16)? & COMPLETION_FLAG != 0,
        bus: u16_at(17)?,
        address: u16_at(19)?,
        endpoint: *bytes.get(21)?,
        transfer,
        stage,
        data: bytes[data_start..data_end].to_vec(),
    })
}

struct PendingControl {
    timestamp_micros: u64,
    bus: u16,
    address: u16,
    setup: SetupPacket,
    out_data: Vec<u8>,
}

pub fn assemble_transfers(packets: &[UsbPcapPacket]) -> Vec<UsbTransfer> {
    let mut transfers = Vec::new();
    let mut pending_controls: HashMap<(u16, u64), PendingControl> = HashMap::new();
    for packet in packets {
        match packet.transfer {
            TRANSFER_CONTROL => assemble_control(packet, &mut pending_controls, &mut transfers),
            TRANSFER_INTERRUPT => {
                if let Some(transfer) = interrupt_transfer_of(packet) {
                    transfers.push(transfer);
                }
            }
            _ => {}
        }
    }
    transfers.extend(pending_controls.into_values().map(|pending| UsbTransfer::Control {
        timestamp_micros: pending.timestamp_micros,
        bus: pending.bus,
        address: pending.address,
        setup: pending.setup,
        out_data: pending.out_data,
        in_data: Vec::new(),
        status: None,
    }));
    transfers.sort_by_key(UsbTransfer::timestamp_micros);
    transfers
}

fn assemble_control(
    packet: &UsbPcapPacket,
    pending_controls: &mut HashMap<(u16, u64), PendingControl>,
    transfers: &mut Vec<UsbTransfer>,
) {
    let key = (packet.bus, packet.irp_id);
    if !packet.completion {
        match packet.stage {
            Some(STAGE_SETUP) => {
                let Some(setup) = SetupPacket::from_bytes(&packet.data) else {
                    return;
                };
                let out_data = packet.data.get(SETUP_PACKET_BYTES..).unwrap_or_default().to_vec();
                let pending = PendingControl {
                    timestamp_micros: packet.timestamp_micros,
                    bus: packet.bus,
                    address: packet.address,
                    setup,
                    out_data,
                };
                pending_controls.insert(key, pending);
            }
            Some(STAGE_DATA) => {
                if let Some(pending) = pending_controls.get_mut(&key) {
                    pending.out_data.extend_from_slice(&packet.data);
                }
            }
            _ => {}
        }
        return;
    }
    let Some(pending) = pending_controls.remove(&key) else {
        return;
    };
    let in_data = if pending.setup.is_device_to_host() { packet.data.clone() } else { Vec::new() };
    transfers.push(UsbTransfer::Control {
        timestamp_micros: pending.timestamp_micros,
        bus: pending.bus,
        address: pending.address,
        setup: pending.setup,
        out_data: pending.out_data,
        in_data,
        status: Some(packet.status),
    });
}

fn interrupt_transfer_of(packet: &UsbPcapPacket) -> Option<UsbTransfer> {
    let device_to_host = packet.endpoint & DIRECTION_IN_FLAG != 0;
    let carries_data = if device_to_host { packet.completion } else { !packet.completion };
    (carries_data && !packet.data.is_empty()).then(|| UsbTransfer::Interrupt {
        timestamp_micros: packet.timestamp_micros,
        bus: packet.bus,
        address: packet.address,
        endpoint: packet.endpoint,
        data: packet.data.clone(),
        status: packet.status,
    })
}

#[cfg(test)]
pub mod test_support {
    use super::*;

    pub struct UsbPcapRecord {
        pub timestamp_micros: u64,
        pub irp_id: u64,
        pub completion: bool,
        pub address: u16,
        pub endpoint: u8,
        pub transfer: u8,
        pub stage: Option<u8>,
        pub data: Vec<u8>,
    }

    pub const BUS: u16 = 1;
    pub const INTERRUPT: u8 = TRANSFER_INTERRUPT;
    pub const CONTROL: u8 = TRANSFER_CONTROL;
    pub const SETUP_STAGE: u8 = STAGE_SETUP;
    pub const DATA_STAGE: u8 = STAGE_DATA;
    pub const COMPLETE_STAGE: u8 = 3;
    const URB_FUNCTION_CONTROL_TRANSFER: u16 = 0x0008;
    const URB_FUNCTION_BULK_OR_INTERRUPT_TRANSFER: u16 = 0x0009;

    pub fn usbpcap_bytes(record: &UsbPcapRecord) -> Vec<u8> {
        let header_length: u16 = if record.stage.is_some() { 28 } else { 27 };
        let function = if record.transfer == CONTROL {
            URB_FUNCTION_CONTROL_TRANSFER
        } else {
            URB_FUNCTION_BULK_OR_INTERRUPT_TRANSFER
        };
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&header_length.to_le_bytes());
        bytes.extend_from_slice(&record.irp_id.to_le_bytes());
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.extend_from_slice(&function.to_le_bytes());
        bytes.push(u8::from(record.completion));
        bytes.extend_from_slice(&BUS.to_le_bytes());
        bytes.extend_from_slice(&record.address.to_le_bytes());
        bytes.push(record.endpoint);
        bytes.push(record.transfer);
        bytes.extend_from_slice(&(record.data.len() as u32).to_le_bytes());
        if let Some(stage) = record.stage {
            bytes.push(stage);
        }
        bytes.extend_from_slice(&record.data);
        bytes
    }

    pub fn captured(record: &UsbPcapRecord) -> CapturedPacket {
        CapturedPacket {
            link_type: LINKTYPE_USBPCAP,
            timestamp_micros: record.timestamp_micros,
            data: usbpcap_bytes(record),
        }
    }

    pub fn setup_bytes(request_type: u8, request: u8, value: u16, index: u16, length: u16) -> Vec<u8> {
        let mut bytes = vec![request_type, request];
        bytes.extend_from_slice(&value.to_le_bytes());
        bytes.extend_from_slice(&index.to_le_bytes());
        bytes.extend_from_slice(&length.to_le_bytes());
        bytes
    }

    pub fn control_records(
        timestamp_micros: u64,
        irp_id: u64,
        address: u16,
        setup: Vec<u8>,
        out_data: &[u8],
        in_data: &[u8],
    ) -> Vec<UsbPcapRecord> {
        let mut setup_and_out = setup;
        setup_and_out.extend_from_slice(out_data);
        vec![
            UsbPcapRecord {
                timestamp_micros,
                irp_id,
                completion: false,
                address,
                endpoint: 0x00,
                transfer: CONTROL,
                stage: Some(SETUP_STAGE),
                data: setup_and_out,
            },
            UsbPcapRecord {
                timestamp_micros: timestamp_micros + 500,
                irp_id,
                completion: true,
                address,
                endpoint: if in_data.is_empty() { 0x00 } else { 0x80 },
                transfer: CONTROL,
                stage: Some(COMPLETE_STAGE),
                data: in_data.to_vec(),
            },
        ]
    }

    pub fn interrupt_record(
        timestamp_micros: u64,
        irp_id: u64,
        address: u16,
        endpoint: u8,
        data: &[u8],
    ) -> UsbPcapRecord {
        UsbPcapRecord {
            timestamp_micros,
            irp_id,
            completion: endpoint & DIRECTION_IN_FLAG != 0,
            address,
            endpoint,
            transfer: INTERRUPT,
            stage: None,
            data: data.to_vec(),
        }
    }

    pub fn parsed(records: &[UsbPcapRecord]) -> Vec<UsbPcapPacket> {
        records.iter().filter_map(|record| parse_usbpcap_packet(&captured(record))).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::*;
    use super::*;

    #[test]
    fn reads_the_usbpcap_header() {
        let record = interrupt_record(42, 0xdead_beef, 7, 0x84, &[0xa0, 0x01]);

        let packet = parse_usbpcap_packet(&captured(&record)).unwrap();

        assert_eq!(packet.irp_id, 0xdead_beef);
        assert!(packet.completion);
        assert_eq!((packet.bus, packet.address, packet.endpoint), (BUS, 7, 0x84));
        assert_eq!(packet.transfer, INTERRUPT);
        assert_eq!(packet.stage, None);
        assert_eq!(packet.data, vec![0xa0, 0x01]);
    }

    #[test]
    fn ignores_other_link_types() {
        let mut packet = captured(&interrupt_record(42, 1, 7, 0x84, &[0xa0]));
        packet.link_type = 1;

        assert_eq!(parse_usbpcap_packet(&packet), None);
    }

    #[test]
    fn joins_a_set_report_with_its_completion() {
        let setup = setup_bytes(0x21, 0x09, 0x0300, 3, 90);
        let packets = parsed(&control_records(1_000, 9, 7, setup, &[0x00, 0x1f, 0x00], &[]));

        let transfers = assemble_transfers(&packets);

        let [UsbTransfer::Control { setup, out_data, in_data, status, timestamp_micros, .. }] = transfers.as_slice()
        else {
            panic!("expected one control transfer, got {transfers:?}");
        };
        assert_eq!((setup.request_type, setup.request, setup.value, setup.index), (0x21, 0x09, 0x0300, 3));
        assert_eq!(out_data, &vec![0x00, 0x1f, 0x00]);
        assert!(in_data.is_empty());
        assert_eq!(*status, Some(0));
        assert_eq!(*timestamp_micros, 1_000);
    }

    #[test]
    fn collects_out_data_from_a_separate_data_stage() {
        let mut records = control_records(1_000, 9, 7, setup_bytes(0x21, 0x09, 0x0300, 3, 2), &[], &[]);
        records.insert(
            1,
            UsbPcapRecord {
                timestamp_micros: 1_100,
                irp_id: 9,
                completion: false,
                address: 7,
                endpoint: 0x00,
                transfer: CONTROL,
                stage: Some(DATA_STAGE),
                data: vec![0xd1, 0xda],
            },
        );

        let transfers = assemble_transfers(&parsed(&records));

        let [UsbTransfer::Control { out_data, .. }] = transfers.as_slice() else {
            panic!("expected one control transfer");
        };
        assert_eq!(out_data, &vec![0xd1, 0xda]);
    }

    #[test]
    fn takes_in_data_from_the_completion() {
        let setup = setup_bytes(0xa1, 0x01, 0x0300, 3, 90);
        let transfers = assemble_transfers(&parsed(&control_records(1_000, 9, 7, setup, &[], &[0x02, 0x1f])));

        let [UsbTransfer::Control { in_data, .. }] = transfers.as_slice() else {
            panic!("expected one control transfer");
        };
        assert_eq!(in_data, &vec![0x02, 0x1f]);
    }

    #[test]
    fn keeps_a_control_transfer_whose_completion_is_missing() {
        let mut records = control_records(1_000, 9, 7, setup_bytes(0x21, 0x09, 0x0300, 3, 1), &[0x01], &[]);
        records.pop();

        let transfers = assemble_transfers(&parsed(&records));

        assert!(matches!(transfers.as_slice(), [UsbTransfer::Control { status: None, .. }]));
    }

    #[test]
    fn keeps_interrupt_data_in_the_direction_it_travels() {
        let records = vec![
            interrupt_record(1_000, 1, 7, 0x03, &[0x01, 0xd1]),
            UsbPcapRecord { completion: true, data: Vec::new(), ..interrupt_record(1_200, 1, 7, 0x03, &[]) },
            UsbPcapRecord { completion: false, data: Vec::new(), ..interrupt_record(1_300, 2, 7, 0x83, &[]) },
            interrupt_record(1_400, 2, 7, 0x83, &[0x01, 0xd1, 0xda]),
        ];

        let transfers = assemble_transfers(&parsed(&records));

        assert_eq!(transfers.len(), 2);
        assert!(matches!(&transfers[0], UsbTransfer::Interrupt { endpoint: 0x03, .. }));
        assert!(matches!(&transfers[1], UsbTransfer::Interrupt { endpoint: 0x83, data, .. } if data.len() == 3));
    }
}
