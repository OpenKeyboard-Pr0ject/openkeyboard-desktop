use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstalledSoftware {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub publisher: Option<String>,
}

const PERIPHERAL_SOFTWARE_KEYWORDS: &[&str] = &[
    "ajazz",
    "akko",
    "alienware",
    "armoury crate",
    "atk",
    "attack shark",
    "aula",
    "bloody",
    "cherry",
    "cooler master",
    "corsair",
    "dareu",
    "dark project",
    "das keyboard",
    "drunkdeer",
    "ducky",
    "endgame gear",
    "epomaker",
    "fnatic",
    "g skill",
    "gamakay",
    "glorious",
    "hator",
    "hyperx",
    "icue",
    "kemove",
    "keychron",
    "lamzu",
    "legion",
    "leobog",
    "lofree",
    "logitech",
    "machenike",
    "mad catz",
    "madlions",
    "masterplus",
    "melgeek",
    "monsgeek",
    "mountain",
    "msi center",
    "ngenuity",
    "nuphy",
    "nzxt",
    "omen",
    "openrgb",
    "qmk",
    "razer",
    "redragon",
    "roccat",
    "rog",
    "royal kludge",
    "sharkoon",
    "signalrgb",
    "skyloong",
    "steelseries",
    "thermaltake",
    "turtle beach",
    "varmilo",
    "vial",
    "vxe",
    "womier",
    "wooting",
    "wootility",
    "zowie",
];

pub fn is_peripheral_software(name: &str, publisher: Option<&str>) -> bool {
    let words = format!(" {} ", normalized_words(&format!("{name} {}", publisher.unwrap_or_default())));
    PERIPHERAL_SOFTWARE_KEYWORDS.iter().any(|keyword| words.contains(&format!(" {keyword} ")))
}

fn normalized_words(text: &str) -> String {
    text.to_lowercase()
        .split(|character: char| !character.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(windows)]
pub fn installed_peripheral_software() -> Vec<InstalledSoftware> {
    use std::collections::BTreeSet;

    use winreg::RegKey;
    use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_32KEY, KEY_WOW64_64KEY};

    const UNINSTALL_KEY: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall";

    let uninstall_lists = [
        (HKEY_LOCAL_MACHINE, KEY_READ | KEY_WOW64_64KEY),
        (HKEY_LOCAL_MACHINE, KEY_READ | KEY_WOW64_32KEY),
        (HKEY_CURRENT_USER, KEY_READ),
    ];
    let mut found = BTreeSet::new();
    for (root, access) in uninstall_lists {
        let Ok(uninstall) = RegKey::predef(root).open_subkey_with_flags(UNINSTALL_KEY, access) else {
            continue;
        };
        for entry_name in uninstall.enum_keys().flatten() {
            let Ok(entry) = uninstall.open_subkey_with_flags(&entry_name, access) else {
                continue;
            };
            let Ok(name) = entry.get_value::<String, _>("DisplayName") else {
                continue;
            };
            let publisher: Option<String> = entry.get_value("Publisher").ok();
            if is_peripheral_software(&name, publisher.as_deref()) {
                found.insert(InstalledSoftware { name, version: entry.get_value("DisplayVersion").ok(), publisher });
            }
        }
    }
    found.into_iter().collect()
}

#[cfg(not(windows))]
pub fn installed_peripheral_software() -> Vec<InstalledSoftware> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_vendor_tools_by_name() {
        assert!(is_peripheral_software("Razer Synapse", None));
        assert!(is_peripheral_software("Wootility", None));
        assert!(is_peripheral_software("ARMOURY CRATE Service", None));
    }

    #[test]
    fn recognizes_vendor_tools_by_publisher() {
        assert!(is_peripheral_software("iCUE", Some("Corsair Memory, Inc.")));
        assert!(is_peripheral_software("Peripheral Driver", Some("G.SKILL International")));
    }

    #[test]
    fn matches_whole_words_only() {
        assert!(!is_peripheral_software("Programmer's Notepad", Some("Programmer")));
        assert!(!is_peripheral_software("Microsoft Edge", Some("Microsoft Corporation")));
        assert!(!is_peripheral_software("Atkinson Hyperlegible", None));
    }
}
