use std::path::PathBuf;

use crate::device_report::ReportRequest;
use crate::device_selection::DeviceSelection;

pub const DEFAULT_OUTPUT_FILE: &str = "openkeyboard-device-report.json";

pub const USAGE: &str = "\
Usage: openkeyboard-probe [options]

Writes a report about your keyboards that helps OpenKeyboard support them.
By default it only lists devices and reads their descriptors. It sends no commands to any device.

Options:
  --read-versions         Also ask VIA, Vial, Wooting and Razer devices for their protocol and firmware
                          versions. Only read commands from a fixed list are sent, and nothing is changed.
  --all                   Include every HID device, not only devices with a keyboard collection
  --vendor-id <hex>       Include only devices with this vendor id, for example 1532
  --product-id <hex>      Include only devices with this product id, for example 0258
  --output <file>         Write the report to this file (default: openkeyboard-device-report.json)
  --help                  Show this help";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Collect(CollectOptions),
    ShowHelp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollectOptions {
    pub request: ReportRequest,
    pub output_path: PathBuf,
}

pub fn parse_arguments(arguments: &[String]) -> Result<Command, String> {
    let mut selection = DeviceSelection::default();
    let mut read_versions = false;
    let mut output_path = PathBuf::from(DEFAULT_OUTPUT_FILE);
    let mut remaining = arguments.iter();
    while let Some(argument) = remaining.next() {
        match argument.as_str() {
            "--help" | "-h" => return Ok(Command::ShowHelp),
            "--all" => selection.every_device = true,
            "--read-versions" => read_versions = true,
            "--vendor-id" => selection.vendor_id = Some(parse_hex_id(value_after(argument, remaining.next())?)?),
            "--product-id" => selection.product_id = Some(parse_hex_id(value_after(argument, remaining.next())?)?),
            "--output" => output_path = PathBuf::from(value_after(argument, remaining.next())?),
            unknown => return Err(format!("Unknown option {unknown}.")),
        }
    }
    Ok(Command::Collect(CollectOptions { request: ReportRequest { selection, read_versions }, output_path }))
}

fn value_after<'argument>(option: &str, value: Option<&'argument String>) -> Result<&'argument str, String> {
    value.map(String::as_str).ok_or_else(|| format!("{option} needs a value."))
}

fn parse_hex_id(text: &str) -> Result<u16, String> {
    let digits = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")).unwrap_or(text);
    u16::from_str_radix(digits, 16).map_err(|_| format!("{text} is not a hexadecimal id from 0 to ffff."))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(arguments: &[&str]) -> Result<Command, String> {
        parse_arguments(&arguments.iter().map(|argument| argument.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn collects_keyboards_into_the_default_file_without_options() {
        assert_eq!(
            parse(&[]),
            Ok(Command::Collect(CollectOptions {
                request: ReportRequest::default(),
                output_path: PathBuf::from(DEFAULT_OUTPUT_FILE),
            }))
        );
    }

    #[test]
    fn reads_every_option() {
        let Ok(Command::Collect(options)) =
            parse(&["--all", "--read-versions", "--vendor-id", "0x1532", "--product-id", "0258", "--output", "r.json"])
        else {
            panic!("expected collect options");
        };

        assert_eq!(
            options.request.selection,
            DeviceSelection { every_device: true, vendor_id: Some(0x1532), product_id: Some(0x0258) }
        );
        assert!(options.request.read_versions);
        assert_eq!(options.output_path, PathBuf::from("r.json"));
    }

    #[test]
    fn shows_help() {
        assert_eq!(parse(&["--all", "--help"]), Ok(Command::ShowHelp));
    }

    #[test]
    fn rejects_bad_input() {
        assert_eq!(parse(&["--vendor-id"]), Err("--vendor-id needs a value.".to_string()));
        assert_eq!(parse(&["--vendor-id", "xyz"]), Err("xyz is not a hexadecimal id from 0 to ffff.".to_string()));
        assert_eq!(parse(&["--verbose"]), Err("Unknown option --verbose.".to_string()));
    }
}
