use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Environment {
    pub operating_system: String,
    pub architecture: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub windows: Option<WindowsVersion>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowsVersion {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub product_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_version: Option<String>,
    pub build: String,
}

const FIRST_WINDOWS_11_BUILD: u32 = 22_000;

pub fn corrected_product_name(product_name: &str, build_number: &str) -> String {
    let is_windows_11 = build_number.parse::<u32>().is_ok_and(|build| build >= FIRST_WINDOWS_11_BUILD);
    if is_windows_11 { product_name.replacen("Windows 10", "Windows 11", 1) } else { product_name.to_string() }
}

pub fn current_environment() -> Environment {
    Environment {
        operating_system: std::env::consts::OS.to_string(),
        architecture: std::env::consts::ARCH.to_string(),
        windows: windows_version(),
    }
}

#[cfg(windows)]
fn windows_version() -> Option<WindowsVersion> {
    use winreg::RegKey;
    use winreg::enums::HKEY_LOCAL_MACHINE;

    const CURRENT_VERSION_KEY: &str = r"SOFTWARE\Microsoft\Windows NT\CurrentVersion";

    let key = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey(CURRENT_VERSION_KEY).ok()?;
    let build_number: String = key.get_value("CurrentBuildNumber").ok()?;
    let update_revision: Option<u32> = key.get_value("UBR").ok();
    let product_name: Option<String> = key.get_value("ProductName").ok();
    Some(WindowsVersion {
        product_name: product_name.map(|name| corrected_product_name(&name, &build_number)),
        display_version: key.get_value("DisplayVersion").ok(),
        build: update_revision.map_or(build_number.clone(), |revision| format!("{build_number}.{revision}")),
    })
}

#[cfg(not(windows))]
fn windows_version() -> Option<WindowsVersion> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_windows_11_by_its_build() {
        assert_eq!(corrected_product_name("Windows 10 Pro", "26200"), "Windows 11 Pro");
    }

    #[test]
    fn keeps_windows_10() {
        assert_eq!(corrected_product_name("Windows 10 Pro", "19045"), "Windows 10 Pro");
    }
}
