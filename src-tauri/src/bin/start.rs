#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use fs2::available_space;
use std::{
    fs::{self, OpenOptions},
    path::{Component, Path, PathBuf},
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
const AZAHAR_PATH_MARKER: &str = "saves\\3ds\\azahar\\";

#[derive(Debug, Default)]
struct M3uRewriteSummary {
    scanned: usize,
    rewritten: usize,
    warnings: Vec<String>,
}

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

fn regular_windows_path(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    if let Some(rest) = text.strip_prefix(r"\\?\") {
        return PathBuf::from(rest);
    }
    path.to_path_buf()
}

fn drive_root() -> Result<PathBuf, String> {
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    executable
        .parent()
        .ok_or_else(|| "START.exe has no parent directory".to_string())?
        .canonicalize()
        .map(|path| regular_windows_path(&path))
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
    for hive in [
        RegKey::predef(HKEY_LOCAL_MACHINE),
        RegKey::predef(HKEY_CURRENT_USER),
    ] {
        for base in [
            "SOFTWARE\\Microsoft\\EdgeUpdate\\Clients",
            "SOFTWARE\\WOW6432Node\\Microsoft\\EdgeUpdate\\Clients",
        ] {
            if let Ok(clients) = hive.open_subkey(base) {
                for name in clients.enum_keys().flatten() {
                    if let Ok(client) = clients.open_subkey(name) {
                        let product: String = client.get_value("name").unwrap_or_default();
                        let version: String = client.get_value("pv").unwrap_or_default();
                        if product.to_ascii_lowercase().contains("webview2") && version != "0.0.0.0"
                        {
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
        .filter_map(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
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

fn collect_m3u_files(directory: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    if !directory.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(directory)
        .map_err(|error| format!("Unable to scan {}: {error}", directory.display()))?
    {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        if path.is_dir() {
            collect_m3u_files(&path, files)?;
        } else if path
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("m3u"))
        {
            files.push(path);
        }
    }
    Ok(())
}

fn current_azahar_app_path(root: &Path, line: &str) -> Result<Option<PathBuf>, String> {
    let candidate = line.trim().trim_matches('"');
    if candidate.is_empty() || candidate.starts_with('#') {
        return Ok(None);
    }
    let normalized = candidate.replace('/', "\\");
    let lower = normalized.to_ascii_lowercase();
    let Some(marker_index) = lower.find(AZAHAR_PATH_MARKER) else {
        return Ok(None);
    };
    if marker_index > 0 && normalized.as_bytes()[marker_index - 1] != b'\\' {
        return Ok(None);
    }
    let relative = Path::new(&normalized[marker_index..]);
    if relative
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(format!("Unsafe Azahar path in playlist: {candidate}"));
    }
    if !relative
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("app"))
    {
        return Ok(None);
    }
    let target = root.join("RetroBat").join(relative);
    if !target.is_file() {
        return Err(format!(
            "Installed Azahar app is missing: {}",
            target.display()
        ));
    }
    Ok(Some(target))
}

fn rewrite_azahar_m3u_text(root: &Path, source: &str) -> Result<(String, bool), String> {
    let newline = if source.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let trailing_newline = source.ends_with('\n');
    let mut changed = false;
    let mut lines = Vec::new();
    for line in source.lines() {
        if let Some(target) = current_azahar_app_path(root, line)? {
            let replacement = target.to_string_lossy().into_owned();
            changed |= !line.trim().eq_ignore_ascii_case(&replacement);
            lines.push(replacement);
        } else {
            lines.push(line.to_string());
        }
    }
    let mut rewritten = lines.join(newline);
    if trailing_newline {
        rewritten.push_str(newline);
    }
    Ok((rewritten, changed))
}

fn rewrite_azahar_m3u_files(root: &Path) -> M3uRewriteSummary {
    let mut summary = M3uRewriteSummary::default();
    let mut files = Vec::new();
    if let Err(error) =
        collect_m3u_files(&root.join("RetroBat").join("roms").join("3ds"), &mut files)
    {
        summary.warnings.push(error);
        return summary;
    }
    for path in files {
        summary.scanned += 1;
        let result = fs::read_to_string(&path)
            .map_err(|error| format!("Unable to read {}: {error}", path.display()))
            .and_then(|source| {
                let (rewritten, changed) = rewrite_azahar_m3u_text(root, &source)?;
                if changed {
                    fs::write(&path, rewritten.as_bytes())
                        .map_err(|error| format!("Unable to update {}: {error}", path.display()))?;
                }
                Ok(changed)
            });
        match result {
            Ok(true) => summary.rewritten += 1,
            Ok(false) => {}
            Err(error) => summary.warnings.push(error),
        }
    }
    summary
}

fn write_start_log(root: &Path, summary: &M3uRewriteSummary) {
    let mut lines = vec![format!(
        "azahar-m3u scanned={} rewritten={} warnings={}",
        summary.scanned,
        summary.rewritten,
        summary.warnings.len()
    )];
    lines.extend(
        summary
            .warnings
            .iter()
            .map(|warning| format!("warning: {warning}")),
    );
    let _ = fs::write(
        root.join("Launcher").join("data").join("start.log"),
        lines.join("\r\n"),
    );
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
    let root =
        drive_root().unwrap_or_else(|error| fail(format!("Unable to locate RetroDrive: {error}")));
    let launcher = root.join("Launcher").join("RetroDriveLauncher.exe");
    if !launcher.is_file() {
        fail("Launcher\\RetroDriveLauncher.exe is missing.");
    }
    if windows_build().unwrap_or_default() < 19045 {
        fail("RetroDrive requires Windows 10 build 19045 or a newer supported Windows release.");
    }
    require_write_access(&root).unwrap_or_else(|error| fail(error));
    let m3u_summary = rewrite_azahar_m3u_files(&root);
    write_start_log(&root, &m3u_summary);
    if !m3u_summary.warnings.is_empty() {
        let details = m3u_summary
            .warnings
            .iter()
            .take(5)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n");
        message(
            &format!("Some installed Nintendo 3DS shortcuts could not be refreshed:\n\n{details}"),
            MB_OK | MB_ICONWARNING,
        );
    }
    let free = available_space(&root)
        .unwrap_or_else(|error| fail(format!("Unable to read free space: {error}")));
    if free < MIN_FREE_BYTES {
        fail(format!(
            "RetroDrive free space is below the reserved 75,015,860,429 bytes. Current free space: {free} bytes."
        ));
    }
    require_runtime_files().unwrap_or_else(|error| fail(error));
    if !webview2_installed() && !recover_webview2(&root) {
        fail(
            "Microsoft Edge WebView2 Runtime is unavailable. See Launcher\\release\\USER-GUIDE.md.",
        );
    }
    let result = Command::new(&launcher)
        .arg(&root)
        .current_dir(root.join("Launcher"))
        .spawn();
    if let Err(error) = result {
        fail(format!("Unable to start RetroDrive Launcher: {error}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_root(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "retrodrive-start-{name}-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("test root should be created");
        root
    }

    fn installed_app(root: &Path, title: &str) -> PathBuf {
        let app = root
            .join("RetroBat")
            .join("saves")
            .join("3ds")
            .join("azahar")
            .join("sdmc")
            .join("Nintendo 3DS")
            .join("id0")
            .join("id1")
            .join("title")
            .join("00040000")
            .join(title)
            .join("content")
            .join("00000000.app");
        fs::create_dir_all(app.parent().expect("app should have a parent"))
            .expect("app directory should be created");
        fs::write(&app, b"NCCH test").expect("app should be created");
        app
    }

    fn playlist(root: &Path, name: &str) -> PathBuf {
        let path = root
            .join("RetroBat")
            .join("roms")
            .join("3ds")
            .join(format!("{name}.m3u"));
        fs::create_dir_all(path.parent().expect("playlist should have a parent"))
            .expect("playlist directory should be created");
        path
    }

    #[test]
    fn removes_windows_verbatim_prefixes_from_portable_paths() {
        assert_eq!(
            regular_windows_path(Path::new(r"\\?\D:\RetroBat\saves\3ds")),
            PathBuf::from(r"D:\RetroBat\saves\3ds")
        );
        assert_eq!(
            regular_windows_path(Path::new(r"\\?\UNC\server\share\RetroBat")),
            PathBuf::from(r"\\server\share\RetroBat")
        );
    }

    #[test]
    fn rewrites_relative_installed_app_path() {
        let root = test_root("relative");
        let app = installed_app(&root, "0008b400");
        let m3u = playlist(&root, "Mario Kart 7");
        fs::write(
            &m3u,
            "..\\..\\saves\\3ds\\azahar\\sdmc\\Nintendo 3DS\\id0\\id1\\title\\00040000\\0008b400\\content\\00000000.app\n",
        )
        .expect("playlist should be written");

        let summary = rewrite_azahar_m3u_files(&root);

        assert_eq!(summary.scanned, 1);
        assert_eq!(summary.rewritten, 1);
        assert!(summary.warnings.is_empty());
        assert_eq!(
            fs::read_to_string(&m3u).unwrap(),
            format!("{}\n", app.display())
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rebases_an_old_drive_absolute_path() {
        let root = test_root("rebase");
        let app = installed_app(&root, "00089f00");
        let m3u = playlist(&root, "Super Mario 3D Land");
        fs::write(
            &m3u,
            r"Z:\RetroBat\saves\3ds\azahar\sdmc\Nintendo 3DS\id0\id1\title\00040000\00089f00\content\00000000.app",
        )
        .expect("playlist should be written");

        let summary = rewrite_azahar_m3u_files(&root);

        assert_eq!(summary.rewritten, 1);
        assert!(summary.warnings.is_empty());
        assert_eq!(fs::read_to_string(&m3u).unwrap(), app.to_string_lossy());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn leaves_unrelated_playlist_entries_unchanged() {
        let root = test_root("unrelated");
        let m3u = playlist(&root, "Retail ROM");
        let original = "Games\\Retail Game.3ds\n#EXTINF:0,Example\n";
        fs::write(&m3u, original).expect("playlist should be written");

        let summary = rewrite_azahar_m3u_files(&root);

        assert_eq!(summary.scanned, 1);
        assert_eq!(summary.rewritten, 0);
        assert!(summary.warnings.is_empty());
        assert_eq!(fs::read_to_string(&m3u).unwrap(), original);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn reports_a_missing_installed_app_without_rewriting() {
        let root = test_root("missing");
        let m3u = playlist(&root, "Missing Game");
        let original = "..\\..\\saves\\3ds\\azahar\\missing\\00000000.app\n";
        fs::write(&m3u, original).expect("playlist should be written");

        let summary = rewrite_azahar_m3u_files(&root);

        assert_eq!(summary.rewritten, 0);
        assert_eq!(summary.warnings.len(), 1);
        assert_eq!(fs::read_to_string(&m3u).unwrap(), original);
        let _ = fs::remove_dir_all(root);
    }
}
