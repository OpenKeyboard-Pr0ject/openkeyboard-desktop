use serde::Serialize;

const LONG_ITEM_PREFIX: u8 = 0xfe;
const LONG_ITEM_HEADER_BYTES: usize = 3;
const DATA_SIZES: [usize; 4] = [0, 1, 2, 4];
const BITS_PER_BYTE: usize = 8;

const MAIN_ITEM: u8 = 0;
const GLOBAL_ITEM: u8 = 1;

const INPUT_TAG: u8 = 0x8;
const OUTPUT_TAG: u8 = 0x9;
const FEATURE_TAG: u8 = 0xb;

const REPORT_SIZE_TAG: u8 = 0x7;
const REPORT_ID_TAG: u8 = 0x8;
const REPORT_COUNT_TAG: u8 = 0x9;
const PUSH_TAG: u8 = 0xa;
const POP_TAG: u8 = 0xb;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ReportKind {
    Input,
    Output,
    Feature,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeclaredReport {
    pub kind: ReportKind,
    pub report_id: u8,
    pub byte_length: usize,
}

#[derive(Debug, Clone, Copy, Default)]
struct GlobalState {
    report_size: usize,
    report_count: usize,
    report_id: u8,
}

pub fn declared_reports(descriptor: &[u8]) -> Result<Vec<DeclaredReport>, String> {
    let mut bits_by_report: Vec<(ReportKind, u8, usize)> = Vec::new();
    let mut global = GlobalState::default();
    let mut global_stack: Vec<GlobalState> = Vec::new();
    let mut offset = 0;
    while offset < descriptor.len() {
        let prefix = descriptor[offset];
        if prefix == LONG_ITEM_PREFIX {
            offset += LONG_ITEM_HEADER_BYTES + usize::from(descriptor.get(offset + 1).copied().unwrap_or(0));
            continue;
        }
        let size = DATA_SIZES[usize::from(prefix & 0x03)];
        let value_bytes = descriptor
            .get(offset + 1..offset + 1 + size)
            .ok_or_else(|| "The report descriptor ends in the middle of an item.".to_string())?;
        let value = little_endian(value_bytes);
        let item_type = (prefix >> 2) & 0x03;
        let tag = prefix >> 4;
        if item_type == GLOBAL_ITEM {
            match tag {
                REPORT_SIZE_TAG => global.report_size = value,
                REPORT_COUNT_TAG => global.report_count = value,
                REPORT_ID_TAG => global.report_id = value as u8,
                PUSH_TAG => global_stack.push(global),
                POP_TAG => global = global_stack.pop().unwrap_or(global),
                _ => {}
            }
        } else if item_type == MAIN_ITEM
            && let Some(kind) = report_kind_of(tag)
        {
            add_bits(&mut bits_by_report, kind, global.report_id, global.report_size * global.report_count);
        }
        offset += 1 + size;
    }
    let mut reports: Vec<DeclaredReport> = bits_by_report
        .into_iter()
        .map(|(kind, report_id, bits)| DeclaredReport { kind, report_id, byte_length: bits.div_ceil(BITS_PER_BYTE) })
        .collect();
    reports.sort_by_key(|report| (report.kind, report.report_id));
    Ok(reports)
}

pub fn find_report(reports: &[DeclaredReport], kind: ReportKind, report_id: u8) -> Option<&DeclaredReport> {
    reports.iter().find(|report| report.kind == kind && report.report_id == report_id)
}

pub fn uses_numbered_input_reports(reports: &[DeclaredReport]) -> bool {
    reports.iter().any(|report| report.kind == ReportKind::Input && report.report_id != 0)
}

fn report_kind_of(tag: u8) -> Option<ReportKind> {
    match tag {
        INPUT_TAG => Some(ReportKind::Input),
        OUTPUT_TAG => Some(ReportKind::Output),
        FEATURE_TAG => Some(ReportKind::Feature),
        _ => None,
    }
}

fn add_bits(bits_by_report: &mut Vec<(ReportKind, u8, usize)>, kind: ReportKind, report_id: u8, bits: usize) {
    match bits_by_report.iter_mut().find(|(known_kind, known_id, _)| *known_kind == kind && *known_id == report_id) {
        Some((_, _, total)) => *total += bits,
        None => bits_by_report.push((kind, report_id, bits)),
    }
}

fn little_endian(bytes: &[u8]) -> usize {
    bytes.iter().rev().fold(0, |value, byte| (value << BITS_PER_BYTE) | usize::from(*byte))
}

#[cfg(test)]
pub mod test_support {
    pub fn hex_to_bytes(hex: &str) -> Vec<u8> {
        (0..hex.len()).step_by(2).map(|index| u8::from_str_radix(&hex[index..index + 2], 16).unwrap()).collect()
    }

    pub const WOOTING_80HE_CONFIG_DESCRIPTOR: &str = concat!(
        "0655ff0901a1018501090c150026ff0075089520810285020916150026ff007508953e810285030920150026ff00",
        "750895fe81028504092a150026ff00750896fe01810285050934150026ff00750896fe0381028506093e150026ff",
        "00750896fe0781028501090e150026ff0075089520910285020918150026ff007508953e910285030922150026ff",
        "00750895fe91028504092c150026ff00750896fe01910285050936150026ff00750896fe03910285060940150026",
        "ff00750896fe07910285010910150026ff0075089507b102c0",
    );

    pub const RAZER_CONTROL_DESCRIPTOR: &str = "050c0901a1017508955ab103c0";

    pub const VIA_RAW_HID_DESCRIPTOR: &str = "0660ff0961a1010962150026ff009520750881020963150026ff00952075089102c0";
}

#[cfg(test)]
mod tests {
    use super::test_support::*;
    use super::*;

    fn report(kind: ReportKind, report_id: u8, byte_length: usize) -> DeclaredReport {
        DeclaredReport { kind, report_id, byte_length }
    }

    #[test]
    fn reads_the_wooting_80he_config_collection() {
        let reports = declared_reports(&hex_to_bytes(WOOTING_80HE_CONFIG_DESCRIPTOR)).unwrap();

        assert_eq!(find_report(&reports, ReportKind::Feature, 1), Some(&report(ReportKind::Feature, 1, 7)));
        assert_eq!(find_report(&reports, ReportKind::Output, 3), Some(&report(ReportKind::Output, 3, 254)));
        assert_eq!(find_report(&reports, ReportKind::Input, 6), Some(&report(ReportKind::Input, 6, 2046)));
        assert_eq!(reports.len(), 13);
        assert!(uses_numbered_input_reports(&reports));
    }

    #[test]
    fn counts_constant_fields_of_the_razer_control_report() {
        let reports = declared_reports(&hex_to_bytes(RAZER_CONTROL_DESCRIPTOR)).unwrap();

        assert_eq!(reports, vec![report(ReportKind::Feature, 0, 90)]);
        assert!(!uses_numbered_input_reports(&reports));
    }

    #[test]
    fn reads_the_qmk_raw_hid_collection() {
        let reports = declared_reports(&hex_to_bytes(VIA_RAW_HID_DESCRIPTOR)).unwrap();

        assert_eq!(reports, vec![report(ReportKind::Input, 0, 32), report(ReportKind::Output, 0, 32)]);
    }

    #[test]
    fn restores_global_state_after_pop() {
        let descriptor = [0x75, 0x08, 0x95, 0x04, 0xa4, 0x95, 0x10, 0x81, 0x02, 0xb4, 0x91, 0x02];

        assert_eq!(
            declared_reports(&descriptor).unwrap(),
            vec![report(ReportKind::Input, 0, 16), report(ReportKind::Output, 0, 4)]
        );
    }

    #[test]
    fn skips_long_items() {
        let descriptor = [0xfe, 0x02, 0x10, 0xaa, 0xbb, 0x75, 0x08, 0x95, 0x02, 0xb1, 0x02];

        assert_eq!(declared_reports(&descriptor).unwrap(), vec![report(ReportKind::Feature, 0, 2)]);
    }

    #[test]
    fn rejects_a_truncated_descriptor() {
        assert!(declared_reports(&[0x75]).is_err());
    }
}
