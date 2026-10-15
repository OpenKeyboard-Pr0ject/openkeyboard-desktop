use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::hid_traffic::CaptureMark;

pub const LIVE_CAPTURE_SOURCE: &str = "usbpcap";
const INTERFACE_VALUE_PREFIX: &str = "{value=";

pub struct LiveCapture {
    pub files: Vec<Vec<u8>>,
    pub started_micros: u64,
    pub marks: Vec<CaptureMark>,
}

pub fn root_hub_interfaces(extcap_listing: &str) -> Vec<String> {
    extcap_listing
        .lines()
        .filter(|line| line.starts_with("interface "))
        .filter_map(|line| {
            let value_start = line.find(INTERFACE_VALUE_PREFIX)? + INTERFACE_VALUE_PREFIX.len();
            let value_length = line[value_start..].find('}')?;
            Some(line[value_start..value_start + value_length].to_string())
        })
        .collect()
}

pub fn usbpcap_command_candidates(program_folders: &[PathBuf]) -> Vec<PathBuf> {
    program_folders
        .iter()
        .flat_map(|folder| {
            [
                folder.join("USBPcap").join("USBPcapCMD.exe"),
                folder.join("Wireshark").join("extcap").join("USBPcapCMD.exe"),
            ]
        })
        .collect()
}

fn now_micros() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|elapsed| elapsed.as_micros() as u64).unwrap_or(0)
}

#[cfg(windows)]
pub fn record_live_capture(input: &mut impl BufRead, output: &mut impl Write) -> Result<LiveCapture, String> {
    use std::path::Path;
    use std::process::{Child, Command, Stdio};
    use std::time::Duration;

    use crate::capture_marks::prompt_for_marks;

    const ELEVATION_REQUIRED: i32 = 740;
    const STARTUP_CHECK_DELAY: Duration = Duration::from_millis(700);
    const PROGRAM_FOLDER_VARIABLES: [&str; 3] = ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"];
    const ADMINISTRATOR_HINT: &str = "Run the probe from a terminal opened with Run as administrator.";
    const INSTALL_HINT: &str = "USBPcap is not installed. Install it from https://desowin.org/usbpcap/ or tick \
        USBPcap in the Wireshark installer, restart Windows, then run the probe again from a terminal opened with \
        Run as administrator.";

    struct HubCapture {
        hub: String,
        file: PathBuf,
        process: Child,
    }

    fn start_hub_capture(command: &Path, hub: &str, file: &Path) -> Result<Child, String> {
        Command::new(command)
            .arg("-d")
            .arg(hub)
            .arg("-o")
            .arg(file)
            .arg("-A")
            .arg("--inject-descriptors")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|error| match error.raw_os_error() {
                Some(ELEVATION_REQUIRED) => format!("USBPcap needs administrator rights. {ADMINISTRATOR_HINT}"),
                _ => format!("Could not start USBPcapCMD: {error}"),
            })
    }

    fn stop(captures: Vec<HubCapture>) -> Vec<Vec<u8>> {
        captures
            .into_iter()
            .filter_map(|mut capture| {
                let _ = capture.process.kill();
                let _ = capture.process.wait();
                std::fs::read(&capture.file).ok()
            })
            .collect()
    }

    let program_folders: Vec<PathBuf> =
        PROGRAM_FOLDER_VARIABLES.iter().filter_map(std::env::var_os).map(PathBuf::from).collect();
    let command = usbpcap_command_candidates(&program_folders)
        .into_iter()
        .find(|candidate| candidate.is_file())
        .ok_or(INSTALL_HINT)?;
    let listing = Command::new(&command)
        .arg("--extcap-interfaces")
        .output()
        .map_err(|error| format!("Could not ask USBPcapCMD for root hubs: {error}"))?;
    let hubs = root_hub_interfaces(&String::from_utf8_lossy(&listing.stdout));
    if hubs.is_empty() {
        return Err("USBPcap lists no root hubs. Restart Windows after installing USBPcap.".to_string());
    }
    let folder = std::env::temp_dir().join(format!("openkeyboard-probe-{}", std::process::id()));
    std::fs::create_dir_all(&folder).map_err(|error| format!("Could not create {}: {error}", folder.display()))?;
    let started_micros = now_micros();
    let mut captures = Vec::new();
    for (index, hub) in hubs.iter().enumerate() {
        let file = folder.join(format!("root-hub-{}.pcap", index + 1));
        match start_hub_capture(&command, hub, &file) {
            Ok(process) => captures.push(HubCapture { hub: hub.clone(), file, process }),
            Err(error) => {
                stop(captures);
                let _ = std::fs::remove_dir_all(&folder);
                return Err(error);
            }
        }
    }
    std::thread::sleep(STARTUP_CHECK_DELAY);
    let stopped_hub = captures.iter_mut().find_map(|capture| {
        let status = capture.process.try_wait().ok().flatten()?;
        Some(format!("USBPcapCMD stopped right away on {} ({status}). {ADMINISTRATOR_HINT}", capture.hub))
    });
    if let Some(error) = stopped_hub {
        stop(captures);
        let _ = std::fs::remove_dir_all(&folder);
        return Err(error);
    }
    let marks = prompt_for_marks(input, output, || now_micros().saturating_sub(started_micros));
    let files = stop(captures);
    let _ = std::fs::remove_dir_all(&folder);
    Ok(LiveCapture { files, started_micros, marks })
}

#[cfg(not(windows))]
pub fn record_live_capture(_input: &mut impl BufRead, _output: &mut impl Write) -> Result<LiveCapture, String> {
    let _ = now_micros;
    Err("Live capture works on Windows with USBPcap. Record a capture there and pass it to --import-capture."
        .to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_root_hubs_from_the_extcap_listing() {
        let listing = "extcap {version=1.5.4.0}{help=https://desowin.org/usbpcap/}\n\
                       interface {value=\\\\.\\USBPcap1}{display=USBPcap1}\n\
                       interface {value=\\\\.\\USBPcap2}{display=USBPcap2}\n";

        assert_eq!(root_hub_interfaces(listing), vec![r"\\.\USBPcap1", r"\\.\USBPcap2"]);
    }

    #[test]
    fn ignores_a_listing_without_interfaces() {
        assert!(root_hub_interfaces("extcap {version=1.5.4.0}\n").is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn looks_in_usbpcap_and_wireshark_folders() {
        let candidates = usbpcap_command_candidates(&[PathBuf::from(r"C:\Program Files")]);

        assert_eq!(
            candidates,
            vec![
                PathBuf::from(r"C:\Program Files\USBPcap\USBPcapCMD.exe"),
                PathBuf::from(r"C:\Program Files\Wireshark\extcap\USBPcapCMD.exe"),
            ]
        );
    }
}
