use std::collections::HashSet;

use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ};
use winreg::{HKEY, RegKey};

use crate::installed_app::InstalledApp;

struct RegistryRoot {
    hive: HKEY,
    hive_label: &'static str,
    path: &'static str,
}

const UNINSTALL_ROOTS: &[RegistryRoot] = &[
    RegistryRoot {
        hive: HKEY_LOCAL_MACHINE,
        hive_label: "HKLM",
        path: r"Software\Microsoft\Windows\CurrentVersion\Uninstall",
    },
    RegistryRoot {
        hive: HKEY_LOCAL_MACHINE,
        hive_label: "HKLM",
        path: r"Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall",
    },
    RegistryRoot {
        hive: HKEY_CURRENT_USER,
        hive_label: "HKCU",
        path: r"Software\Microsoft\Windows\CurrentVersion\Uninstall",
    },
    RegistryRoot {
        hive: HKEY_CURRENT_USER,
        hive_label: "HKCU",
        path: r"Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall",
    },
];

pub fn scan_installed_apps() -> Result<Vec<InstalledApp>, String> {
    let mut apps = Vec::new();
    let mut seen = HashSet::new();

    for root in UNINSTALL_ROOTS {
        scan_root(root, &mut apps, &mut seen)?;
    }

    apps.sort_by(|left, right| {
        left.display_name
            .to_lowercase()
            .cmp(&right.display_name.to_lowercase())
            .then_with(|| left.source_hive.cmp(&right.source_hive))
    });

    Ok(apps)
}

fn scan_root(
    root: &RegistryRoot,
    apps: &mut Vec<InstalledApp>,
    seen: &mut HashSet<String>,
) -> Result<(), String> {
    let hive = RegKey::predef(root.hive);
    let uninstall_key = match hive.open_subkey_with_flags(root.path, KEY_READ) {
        Ok(key) => key,
        Err(_) => return Ok(()),
    };

    for subkey_name in uninstall_key.enum_keys().filter_map(Result::ok) {
        let Ok(app_key) = uninstall_key.open_subkey_with_flags(&subkey_name, KEY_READ) else {
            continue;
        };

        let Some(display_name) = get_string(&app_key, "DisplayName") else {
            continue;
        };
        let Some(uninstall_string) = get_string(&app_key, "UninstallString") else {
            continue;
        };

        if display_name.trim().is_empty() || uninstall_string.trim().is_empty() {
            continue;
        }

        let dedupe_key = format!(
            "{}|{}",
            display_name.trim().to_lowercase(),
            uninstall_string.trim().to_lowercase()
        );
        if !seen.insert(dedupe_key) {
            continue;
        }

        let registry_path = format!(r"{}\{}\{}", root.hive_label, root.path, subkey_name);
        let id = registry_path.to_lowercase();
        apps.push(InstalledApp::new(
            id,
            display_name.trim().to_owned(),
            get_string(&app_key, "Publisher"),
            get_string(&app_key, "DisplayVersion"),
            get_string(&app_key, "InstallLocation"),
            get_dword(&app_key, "EstimatedSize"),
            get_string(&app_key, "InstallDate"),
            uninstall_string.trim().to_owned(),
            registry_path,
            root.hive_label.to_owned(),
            get_dword(&app_key, "SystemComponent").unwrap_or(0) == 1,
            get_dword(&app_key, "NoRemove").unwrap_or(0) == 1,
        ));
    }

    Ok(())
}

fn get_string(key: &RegKey, name: &str) -> Option<String> {
    key.get_value::<String, _>(name)
        .ok()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

fn get_dword(key: &RegKey, name: &str) -> Option<u32> {
    key.get_value::<u32, _>(name).ok()
}
