#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use fs2::available_space;
use std::{
    fs::{self, OpenOptions},
    path::{Path, PathBuf},
    process::Command,
};
use windows::{
    core::PCWSTR,
    Win32::UI::WindowsAndMessaging::{
        MessageBoxW, IDYES, MB_ICONERROR, MB_ICONWARNING, MB_OK, MB_YESNO,
    },
};
use winreg::{enums::*, RegKey};

const MIN_FREE_BYTES: u64 = 75_015_860_429;

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn message(text: &str, flags: windows::Win32::UI::WindowsAndMessaging::MESSAGEBOX_STYLE) -> i32 {
    let text = wide(text);
    let title = wide("RetroDrive");
    unsafe { MessageBoxW(None, PCWSTR(text.as_ptr()), PCWSTR(title.as_ptr()), flags).0 }
}

fn fail(text: impl AsRef<str>) -> ! {
    message(text.as_ref(), MB_OK | MB_ICONERROR);
    std::process::exit(1)
}

fn drive_root() -> Result<PathBuf, String> {
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    executable
        .parent()
        .ok_or_else(|| "START.exe has no parent directory".to_string())?
        .canonicalize()
        .map_err(|error| error.to_string())
}

fn windows_build() -> Option<u32> {
    let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
    let key = hklm
        .open_subkey("SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion")
        .ok()?;
    key.get_value::<String, _>("CurrentBuildNumber")
        .ok()?
        .parse()
        .ok()
}

fn webview2_installed() -> bool {
    let program_files_x86 = std::env::var_os("ProgramFiles(x86)")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Program Files (x86)"));
    if program_files_x86
        .join("Microsoft")
        .join("EdgeWebView")
        .join("Application")
        .is_dir()
    {
        return true;
    }
    for hive in [RegKey::predef(HKEY_LOCAL_MACHINE), RegKey::predef(HKEY_CURRENT_USER)] {
        for base in [
            "SOFTWARE\\Microsoft\\EdgeUpdate\\Clients",
            "SOFTWARE\\WOW6432Node\\Microsoft\\EdgeUpdate\\Clients",
        ] {
            if let Ok(clients) = hive.open_subkey(base) {
                for name in clients.enum_keys().flatten() {
                    if let Ok(client) = clients.open_subkey(name) {
                        let product: String = client.get_value("name").unwrap_or_default();
                        let version: String = client.get_value("pv").unwrap_or_default();
                        if product.to_ascii_lowercase().contains("webview2") && version != "0.0.0.0" {
                            return true;
                        }
                    }
                }
            }
        }
    }
    false
}

fn require_runtime_files() -> Result<(), String> {
    let windows = std::env::var_os("WINDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    let required = [
        windows.join("System32").join("vcruntime140.dll"),
        windows.join("SysWOW64").join("vcruntime140.dll"),
        windows.join("System32").join("d3dx9_43.dll"),
        windows.join("System32").join("XAudio2_7.dll"),
    ];
    let missing: Vec<String> = required
        .iter()
        .filter(|path| !path.is_file())
        .filter_map(|path| path.file_name().map(|name| name.to_string_lossy().into_owned()))
        .collect();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "Required Microsoft runtime files are missing: {}. See Launcher\\release\\USER-GUIDE.md.",
            missing.join(", ")
        ))
    }
}

fn require_write_access(root: &Path) -> Result<(), String> {
    let data = root.join("Launcher").join("data");
    fs::create_dir_all(&data).map_err(|error| error.to_string())?;
    let probe = data.join(format!(".write-test-{}", std::process::id()));
    OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&probe)
        .map_err(|error| format!("Launcher data is not writable: {error}"))?;
    fs::remove_file(&probe).map_err(|error| format!("Write probe cleanup failed: {error}"))?;
    Ok(())
}

fn recover_webview2(root: &Path) -> bool {
    let installer = root
        .join("Launcher")
        .join("runtime")
        .join("MicrosoftEdgeWebView2RuntimeInstallerX64.exe");
    if !installer.is_file() {
        return false;
    }
    let answer = message(
        "Microsoft Edge WebView2 Runtime is required. Run the official offline installer now? Administrator permission may be requested.",
        MB_YESNO | MB_ICONWARNING,
    );
    if answer != IDYES.0 {
        return false;
    }
    Command::new(installer)
        .current_dir(root.join("Launcher").join("runtime"))
        .spawn()
        .and_then(|mut child| child.wait())
        .map(|status| status.success())
        .unwrap_or(false)
        && webview2_installed()
}

fn main() {
    if !cfg!(all(target_os = "windows", target_arch = "x86_64")) {
        fail("RetroDrive requires 64-bit Windows.");
    }
    let root = drive_root().unwrap_or_else(|error| fail(format!("Unable to locate RetroDrive: {error}")));
    let launcher = root.join("Launcher").join("RetroDriveLauncher.exe");
    if !launcher.is_file() {
        fail("Launcher\\RetroDriveLauncher.exe is missing.");
    }
    if windows_build().unwrap_or_default() < 19045 {
        fail("RetroDrive requires Windows 10 build 19045 or a newer supported Windows release.");
    }
    require_write_access(&root).unwrap_or_else(|error| fail(error));
    let free = available_space(&root).unwrap_or_else(|error| fail(format!("Unable to read free space: {error}")));
    if free < MIN_FREE_BYTES {
        fail(format!(
            "RetroDrive free space is below the reserved 75,015,860,429 bytes. Current free space: {free} bytes."
        ));
    }
    require_runtime_files().unwrap_or_else(|error| fail(error));
    if !webview2_installed() && !recover_webview2(&root) {
        fail("Microsoft Edge WebView2 Runtime is unavailable. See Launcher\\release\\USER-GUIDE.md.");
    }
    let result = Command::new(&launcher)
        .arg(&root)
        .current_dir(root.join("Launcher"))
        .spawn();
    if let Err(error) = result {
        fail(format!("Unable to start RetroDrive Launcher: {error}"));
    }
}
