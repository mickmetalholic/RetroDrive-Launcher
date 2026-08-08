use fs2::{available_space, total_space};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    os::windows::process::CommandExt,
    path::{Component, Path, PathBuf},
    process::Command,
    sync::Mutex,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tauri::{Emitter, Manager};
use windows::Win32::{
    Foundation::CloseHandle,
    System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    },
};

mod skill_sync;
#[path = "update_common.rs"]
mod update_common;

use update_common::{verify_package, UpdateManifest, UPDATE_MANIFEST_URL};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

const LOG_LIMIT: u64 = 256 * 1024;

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    schema_version: u32,
    default_entry: String,
    ui: ManifestUi,
    entries: Vec<ManifestEntry>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManifestUi {
    title: String,
    start_launches_default: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManifestEntry {
    id: String,
    #[serde(rename = "type")]
    entry_type: String,
    group: String,
    title: String,
    icon: String,
    executable: String,
    working_directory: String,
    arguments: Vec<String>,
    primary: bool,
    wait: WaitPolicy,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct WaitPolicy {
    mode: String,
    #[serde(default)]
    process_name: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PublicEntry {
    id: String,
    title: String,
    primary: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PublicState {
    version: &'static str,
    default_entry: String,
    active_entry: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PublicEmulator {
    name: String,
    path: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PublicPlatform {
    name: String,
    game_files: u64,
    bytes: u64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PublicDriveOverview {
    root: String,
    drive: String,
    total_bytes: u64,
    free_bytes: u64,
    used_bytes: u64,
    rom_root: String,
    platform_count: u64,
    systems_with_games: u64,
    game_files: u64,
    game_bytes: u64,
    save_bytes: u64,
    emulator_count: u64,
    retro_arch_core_count: u64,
    emulators: Vec<PublicEmulator>,
    platforms: Vec<PublicPlatform>,
    warnings: Vec<String>,
    scanned_at_unix: u64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PublicUpdate {
    current_version: &'static str,
    available_version: Option<String>,
    published_at: Option<String>,
    notes: Option<String>,
    checked_at_unix: Option<u64>,
    status: String,
    detail: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ChildEnded {
    entry_id: String,
    exit_code: Option<i32>,
}

struct LauncherState {
    root: PathBuf,
    manifest: Manifest,
    active: Mutex<Option<(String, u32)>>,
    log_path: PathBuf,
}

fn safe_relative(path: &str) -> Result<PathBuf, String> {
    if path.trim().is_empty() || path.contains(':') || path.starts_with(['/', '\\']) {
        return Err("Path must be a non-empty relative path".into());
    }
    let candidate = PathBuf::from(path.replace('/', "\\"));
    if candidate.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return Err("Path traversal is not allowed".into());
    }
    Ok(candidate)
}

fn contained_path(root: &Path, relative: &str, must_be_file: bool) -> Result<PathBuf, String> {
    let relative = safe_relative(relative)?;
    let candidate = root.join(relative);
    let canonical_root = root
        .canonicalize()
        .map_err(|error| format!("RetroDrive root is unavailable: {error}"))?;
    let canonical = candidate
        .canonicalize()
        .map_err(|error| format!("Configured path is unavailable: {error}"))?;
    if !canonical.starts_with(&canonical_root) {
        return Err("Configured path escapes the RetroDrive root".into());
    }
    if must_be_file && !canonical.is_file() {
        return Err("Configured executable is not a regular file".into());
    }
    Ok(canonical)
}

fn process_running(process_name: &str) -> Result<bool, String> {
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) }
        .map_err(|error| format!("Unable to enumerate processes: {error}"))?;
    let mut entry = PROCESSENTRY32W {
        dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
        ..Default::default()
    };
    let mut found = false;
    if unsafe { Process32FirstW(snapshot, &mut entry) }.is_ok() {
        loop {
            let length = entry
                .szExeFile
                .iter()
                .position(|character| *character == 0)
                .unwrap_or(entry.szExeFile.len());
            let executable = String::from_utf16_lossy(&entry.szExeFile[..length]);
            if executable.eq_ignore_ascii_case(process_name) {
                found = true;
                break;
            }
            if unsafe { Process32NextW(snapshot, &mut entry) }.is_err() {
                break;
            }
        }
    }
    let _ = unsafe { CloseHandle(snapshot) };
    Ok(found)
}

fn wait_for_process_session(process_name: &str) -> Result<(), String> {
    let start_deadline = Instant::now() + Duration::from_secs(30);
    while !process_running(process_name)? {
        if Instant::now() >= start_deadline {
            return Err(format!("Tracked process did not start: {process_name}"));
        }
        thread::sleep(Duration::from_millis(200));
    }
    while process_running(process_name)? {
        thread::sleep(Duration::from_millis(500));
    }
    Ok(())
}

fn validate_manifest(manifest: &Manifest) -> Result<(), String> {
    if manifest.schema_version != 1 {
        return Err(format!(
            "Unsupported schema version {}",
            manifest.schema_version
        ));
    }
    if manifest.ui.title != "RetroDrive" || !manifest.ui.start_launches_default {
        return Err("Unsupported launcher UI contract".into());
    }
    let mut ids = std::collections::HashSet::new();
    for entry in &manifest.entries {
        if !ids.insert(entry.id.as_str()) {
            return Err(format!("Duplicate entry id: {}", entry.id));
        }
        if entry.entry_type != "application" || entry.group != "home" {
            return Err(format!("Unsupported entry type or group: {}", entry.id));
        }
        if entry.wait.mode != "direct" && entry.wait.mode != "processName" {
            return Err(format!("Unsupported wait mode: {}", entry.wait.mode));
        }
        if entry.wait.mode == "processName"
            && entry
                .wait
                .process_name
                .as_deref()
                .unwrap_or_default()
                .is_empty()
        {
            return Err(format!("Missing process name: {}", entry.id));
        }
        safe_relative(&entry.executable)?;
        safe_relative(&entry.working_directory)?;
        safe_relative(&entry.icon)?;
        if Path::new(&entry.executable)
            .extension()
            .and_then(|extension| extension.to_str())
            .map(|extension| !extension.eq_ignore_ascii_case("exe"))
            .unwrap_or(true)
        {
            return Err(format!("Only .exe targets are allowed: {}", entry.id));
        }
    }
    if !ids.contains(manifest.default_entry.as_str()) {
        return Err("Default entry is missing".into());
    }
    Ok(())
}

fn resolve_root() -> Result<PathBuf, String> {
    if let Some(argument) = std::env::args_os().nth(1) {
        let root = PathBuf::from(argument);
        return root
            .canonicalize()
            .map_err(|error| format!("Invalid RetroDrive root: {error}"));
    }
    let executable = std::env::current_exe().map_err(|error| error.to_string())?;
    executable
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| String::from("Unable to resolve RetroDrive root"))?
        .canonicalize()
        .map_err(|error| error.to_string())
}

fn load_state() -> Result<LauncherState, String> {
    let root = resolve_root()?;
    let manifest_path = root.join("launcher.json");
    let manifest: Manifest = serde_json::from_slice(
        &fs::read(&manifest_path)
            .map_err(|error| format!("Unable to read launcher.json: {error}"))?,
    )
    .map_err(|error| format!("Invalid launcher.json: {error}"))?;
    validate_manifest(&manifest)?;
    let data_dir = root.join("Launcher").join("data");
    fs::create_dir_all(&data_dir).map_err(|error| error.to_string())?;
    Ok(LauncherState {
        root,
        manifest,
        active: Mutex::new(None),
        log_path: data_dir.join("launcher.log"),
    })
}

fn write_log(state: &LauncherState, message: &str) {
    if state
        .log_path
        .metadata()
        .map(|meta| meta.len())
        .unwrap_or(0)
        >= LOG_LIMIT
    {
        let rotated = state.log_path.with_extension("log.1");
        let _ = fs::remove_file(&rotated);
        let _ = fs::rename(&state.log_path, rotated);
    }
    if let Ok(mut file) = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&state.log_path)
    {
        let seconds = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|value| value.as_secs())
            .unwrap_or_default();
        let _ = writeln!(file, "{seconds} {message}");
        let _ = file.flush();
    }
}

const GAME_EXTENSIONS: &[&str] = &[
    "3ds", "7z", "app", "bin", "ccd", "cdi", "chd", "cia", "cso", "cue", "dsk", "elf", "gba", "gb",
    "gbc", "gdi", "img", "iso", "m3u", "md", "nes", "nds", "nsp", "pbp", "pkg", "psvita", "rar",
    "rvz", "sfc", "smc", "sms", "sub", "v64", "vpk", "wad", "wbfs", "wux", "xci", "z64", "zip",
];

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

fn push_scan_warning(warnings: &mut Vec<String>, message: String) {
    if warnings.len() < 20 {
        warnings.push(message);
    }
}

fn is_game_file(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| {
            GAME_EXTENSIONS
                .iter()
                .any(|accepted| extension.eq_ignore_ascii_case(accepted))
        })
        .unwrap_or(false)
}

fn scan_platform(
    directory: &Path,
    game_files: &mut u64,
    game_bytes: &mut u64,
    warnings: &mut Vec<String>,
) {
    let mut pending = vec![directory.to_path_buf()];
    while let Some(current) = pending.pop() {
        let entries = match fs::read_dir(&current) {
            Ok(entries) => entries,
            Err(error) => {
                push_scan_warning(
                    warnings,
                    format!("Unable to scan {}: {error}", current.display()),
                );
                continue;
            }
        };
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    push_scan_warning(warnings, error.to_string());
                    continue;
                }
            };
            let path = entry.path();
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(error) => {
                    push_scan_warning(
                        warnings,
                        format!("Unable to inspect {}: {error}", path.display()),
                    );
                    continue;
                }
            };
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                pending.push(path);
            } else if file_type.is_file() && is_game_file(&path) {
                match entry.metadata() {
                    Ok(metadata) => {
                        *game_files += 1;
                        *game_bytes = game_bytes.saturating_add(metadata.len());
                    }
                    Err(error) => push_scan_warning(
                        warnings,
                        format!("Unable to read {}: {error}", path.display()),
                    ),
                }
            }
        }
    }
}

fn scan_directory_bytes(directory: &Path, warnings: &mut Vec<String>) -> u64 {
    let mut pending = vec![directory.to_path_buf()];
    let mut bytes: u64 = 0;
    while let Some(current) = pending.pop() {
        let entries = match fs::read_dir(&current) {
            Ok(entries) => entries,
            Err(error) => {
                if current.exists() {
                    push_scan_warning(
                        warnings,
                        format!("Unable to scan {}: {error}", current.display()),
                    );
                }
                continue;
            }
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                pending.push(path);
            } else if file_type.is_file() {
                if let Ok(metadata) = entry.metadata() {
                    bytes = bytes.saturating_add(metadata.len());
                }
            }
        }
    }
    bytes
}

fn drive_name(root: &Path) -> String {
    let value = root.to_string_lossy();
    if value.len() >= 2 && value.as_bytes().get(1) == Some(&b':') {
        value[..2].to_string()
    } else {
        "Portable".to_string()
    }
}

fn drive_overview(root: &Path) -> Result<PublicDriveOverview, String> {
    let total_bytes =
        total_space(root).map_err(|error| format!("Unable to read disk size: {error}"))?;
    let free_bytes = available_space(root)
        .map_err(|error| format!("Unable to read disk free space: {error}"))?;
    let mut warnings = Vec::new();

    let rom_root = if root.join("roms").is_dir() {
        root.join("roms")
    } else {
        root.join("RetroBat").join("roms")
    };
    let mut platforms = Vec::new();
    if rom_root.is_dir() {
        let entries = fs::read_dir(&rom_root)
            .map_err(|error| format!("Unable to scan ROM root {}: {error}", rom_root.display()))?;
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                Err(error) => {
                    push_scan_warning(&mut warnings, error.to_string());
                    continue;
                }
            };
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if !file_type.is_dir() || file_type.is_symlink() {
                continue;
            }
            let mut game_files = 0;
            let mut game_bytes = 0;
            scan_platform(&path, &mut game_files, &mut game_bytes, &mut warnings);
            if game_files > 0 {
                platforms.push(PublicPlatform {
                    name: entry.file_name().to_string_lossy().into_owned(),
                    game_files,
                    bytes: game_bytes,
                });
            }
        }
    } else {
        push_scan_warning(
            &mut warnings,
            format!("ROM directory is unavailable: {}", rom_root.display()),
        );
    }

    let platform_count = fs::read_dir(&rom_root)
        .ok()
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|entry| entry.file_type().ok())
                .filter(|file_type| file_type.is_dir() && !file_type.is_symlink())
                .count() as u64
        })
        .unwrap_or_default();
    let game_files = platforms.iter().map(|platform| platform.game_files).sum();
    let game_bytes = platforms.iter().map(|platform| platform.bytes).sum();
    platforms.sort_by(|left, right| {
        right
            .game_files
            .cmp(&left.game_files)
            .then_with(|| left.name.cmp(&right.name))
    });

    let emulator_root = root.join("RetroBat").join("emulators");
    let mut emulators = Vec::new();
    if let Ok(entries) = fs::read_dir(&emulator_root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if entry
                .file_type()
                .map(|file_type| file_type.is_dir() && !file_type.is_symlink())
                .unwrap_or(false)
            {
                emulators.push(PublicEmulator {
                    name: entry.file_name().to_string_lossy().into_owned(),
                    path: path
                        .strip_prefix(root)
                        .unwrap_or(&path)
                        .to_string_lossy()
                        .replace('\\', "/"),
                });
            }
        }
    } else {
        push_scan_warning(
            &mut warnings,
            format!(
                "Emulator directory is unavailable: {}",
                emulator_root.display()
            ),
        );
    }
    emulators.sort_by(|left, right| left.name.cmp(&right.name));

    let retro_arch_core_count = fs::read_dir(root.join("RetroArch").join("cores"))
        .ok()
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|entry| entry.file_type().ok())
                .filter(|file_type| file_type.is_file())
                .count() as u64
        })
        .unwrap_or_default();
    let save_bytes = scan_directory_bytes(&root.join("RetroBat").join("saves"), &mut warnings)
        .saturating_add(scan_directory_bytes(
            &root.join("RetroArch").join("saves"),
            &mut warnings,
        ))
        .saturating_add(scan_directory_bytes(
            &root.join("RetroArch").join("states"),
            &mut warnings,
        ));

    Ok(PublicDriveOverview {
        root: root.to_string_lossy().into_owned(),
        drive: drive_name(root),
        total_bytes,
        free_bytes,
        used_bytes: total_bytes.saturating_sub(free_bytes),
        rom_root: rom_root.to_string_lossy().into_owned(),
        platform_count,
        systems_with_games: platforms.len() as u64,
        game_files,
        game_bytes,
        save_bytes,
        emulator_count: emulators.len() as u64,
        retro_arch_core_count,
        emulators,
        platforms,
        warnings,
        scanned_at_unix: unix_now(),
    })
}

fn update_directory(root: &Path) -> PathBuf {
    root.join("Launcher").join("data").join("updates")
}

fn write_update_state(root: &Path, status: &str, version: &str, detail: &str) {
    let state = serde_json::json!({
        "schemaVersion": 1,
        "checkedAtUnix": unix_now(),
        "status": status,
        "version": version,
        "detail": detail,
    });
    let data = root.join("Launcher").join("data");
    let _ = fs::create_dir_all(&data);
    let _ = fs::write(data.join("update-state.json"), state.to_string());
}

fn public_update(
    manifest: Option<&UpdateManifest>,
    status: &str,
    checked_at_unix: Option<u64>,
    detail: impl Into<String>,
) -> PublicUpdate {
    PublicUpdate {
        current_version: env!("CARGO_PKG_VERSION"),
        available_version: manifest.map(|manifest| manifest.version.clone()),
        published_at: manifest.map(|manifest| manifest.published_at.clone()),
        notes: manifest.map(|manifest| manifest.notes.clone()),
        checked_at_unix,
        status: status.to_string(),
        detail: detail.into(),
    }
}

fn cached_update(root: &Path) -> PublicUpdate {
    let state_path = root.join("Launcher").join("data").join("update-state.json");
    let state = fs::read(&state_path)
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok());
    let status = state
        .as_ref()
        .and_then(|value| value.get("status"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("not-checked");
    let checked_at = state
        .as_ref()
        .and_then(|value| value.get("checkedAtUnix"))
        .and_then(serde_json::Value::as_u64);
    let detail = state
        .as_ref()
        .and_then(|value| value.get("detail"))
        .and_then(serde_json::Value::as_str)
        .map(|detail| match detail {
            "还没有检查更新" => "No update check has run",
            "发现可用更新" => "Update available",
            "当前已经是最新版本" => "RetroDrive is up to date",
            "已验证更新，启动器即将重启完成安装" => {
                "Update verified. RetroDrive will restart to finish installation."
            }
            other => other,
        })
        .unwrap_or("No update check has run")
        .to_string();
    let manifest = fs::read(update_directory(root).join("latest.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<UpdateManifest>(&bytes).ok())
        .filter(|manifest| manifest.verify().is_ok())
        .filter(|manifest| {
            manifest
                .is_newer_than(env!("CARGO_PKG_VERSION"))
                .unwrap_or(false)
        });
    let status = match status {
        "available" if manifest.is_some() => "available",
        "current" => "current",
        "installing" => "installing",
        "failed" | "check-failed" => "failed",
        _ => "notChecked",
    };
    public_update(manifest.as_ref(), status, checked_at, detail)
}

fn curl_path() -> PathBuf {
    std::env::var_os("WINDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
        .join("System32")
        .join("curl.exe")
}

fn download_update(url: &str, destination: &Path, timeout_seconds: u32) -> Result<(), String> {
    let curl = curl_path();
    if !curl.is_file() {
        return Err("Windows curl.exe is unavailable".to_string());
    }
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("Unable to create update directory: {error}"))?;
    }
    let _ = fs::remove_file(destination);
    let status = Command::new(curl)
        .args([
            "--fail",
            "--location",
            "--silent",
            "--show-error",
            "--connect-timeout",
            "2",
            "--max-time",
        ])
        .arg(timeout_seconds.to_string())
        .args(["--proto", "=https", "--tlsv1.2", "--output"])
        .arg(destination)
        .arg(url)
        .creation_flags(CREATE_NO_WINDOW)
        .status()
        .map_err(|error| format!("Unable to run the Windows download client: {error}"))?;
    if !status.success() {
        let _ = fs::remove_file(destination);
        return Err(format!("Download failed with status {status}"));
    }
    Ok(())
}

fn fetch_update_manifest(root: &Path) -> Result<(UpdateManifest, PathBuf), String> {
    let updates = update_directory(root);
    fs::create_dir_all(&updates)
        .map_err(|error| format!("Unable to create update directory: {error}"))?;
    let manifest_path = updates.join("latest.json");
    download_update(UPDATE_MANIFEST_URL, &manifest_path, 5)?;
    let manifest: UpdateManifest = serde_json::from_slice(
        &fs::read(&manifest_path)
            .map_err(|error| format!("Unable to read the update manifest: {error}"))?,
    )
    .map_err(|error| format!("Unable to parse the update manifest: {error}"))?;
    manifest.verify()?;
    Ok((manifest, manifest_path))
}

fn install_verified_update(
    root: &Path,
    manifest: &UpdateManifest,
    manifest_path: &Path,
    app: &tauri::AppHandle,
) -> Result<(), String> {
    let updates = update_directory(root);
    let archive = updates.join("package.zip");
    download_update(&manifest.package.url, &archive, 120)?;
    verify_package(&archive, manifest)?;
    let updater = root.join("Launcher").join("Updater.exe");
    if !updater.is_file() {
        return Err("Launcher\\Updater.exe is missing".to_string());
    }
    let temporary_updater =
        std::env::temp_dir().join(format!("RetroDriveUpdater-{}.exe", std::process::id()));
    let _ = fs::remove_file(&temporary_updater);
    fs::copy(&updater, &temporary_updater)
        .map_err(|error| format!("Unable to prepare the update helper: {error}"))?;
    Command::new(&temporary_updater)
        .arg("--root")
        .arg(root)
        .arg("--manifest")
        .arg(manifest_path)
        .arg("--archive")
        .arg(&archive)
        .current_dir(root)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|error| format!("Unable to start the update helper: {error}"))?;
    write_update_state(
        root,
        "installing",
        &manifest.version,
        "Update verified. RetroDrive will restart to finish installation.",
    );
    app.exit(0);
    Ok(())
}

#[tauri::command]
fn get_launcher_state(state: tauri::State<'_, LauncherState>) -> PublicState {
    let active_entry = state
        .active
        .lock()
        .ok()
        .and_then(|active| active.as_ref().map(|(id, _)| id.clone()));
    PublicState {
        version: env!("CARGO_PKG_VERSION"),
        default_entry: state.manifest.default_entry.clone(),
        active_entry,
    }
}

#[tauri::command]
fn list_entries(state: tauri::State<'_, LauncherState>) -> Vec<PublicEntry> {
    state
        .manifest
        .entries
        .iter()
        .map(|entry| PublicEntry {
            id: entry.id.clone(),
            title: entry.title.clone(),
            primary: entry.primary,
        })
        .collect()
}

#[tauri::command]
fn get_drive_overview(
    state: tauri::State<'_, LauncherState>,
) -> Result<PublicDriveOverview, String> {
    let result = drive_overview(&state.root);
    if let Err(error) = &result {
        write_log(&state, &format!("drive-scan-failed detail={error}"));
    }
    result
}

#[tauri::command]
fn get_update_status(state: tauri::State<'_, LauncherState>) -> PublicUpdate {
    cached_update(&state.root)
}

#[tauri::command]
fn check_for_update(state: tauri::State<'_, LauncherState>) -> Result<PublicUpdate, String> {
    match fetch_update_manifest(&state.root) {
        Ok((manifest, _manifest_path)) => {
            let is_newer = manifest.is_newer_than(env!("CARGO_PKG_VERSION"))?;
            let checked_at = unix_now();
            if is_newer {
                write_update_state(
                    &state.root,
                    "available",
                    &manifest.version,
                    "Update available",
                );
                write_log(
                    &state,
                    &format!("update-available version={}", manifest.version),
                );
                Ok(public_update(
                    Some(&manifest),
                    "available",
                    Some(checked_at),
                    "Update available",
                ))
            } else {
                write_update_state(
                    &state.root,
                    "current",
                    env!("CARGO_PKG_VERSION"),
                    "RetroDrive is up to date",
                );
                write_log(&state, "update-current");
                Ok(public_update(
                    None,
                    "current",
                    Some(checked_at),
                    "RetroDrive is up to date",
                ))
            }
        }
        Err(error) => {
            write_update_state(
                &state.root,
                "check-failed",
                env!("CARGO_PKG_VERSION"),
                &error,
            );
            write_log(&state, &format!("update-check-failed detail={error}"));
            Err(error)
        }
    }
}

#[tauri::command]
fn install_update(
    app: tauri::AppHandle,
    state: tauri::State<'_, LauncherState>,
) -> Result<(), String> {
    let manifest_path = update_directory(&state.root).join("latest.json");
    let manifest: UpdateManifest = serde_json::from_slice(
        &fs::read(&manifest_path).map_err(|_| "Check for updates in Settings first".to_string())?,
    )
    .map_err(|error| format!("Unable to parse the cached update manifest: {error}"))?;
    manifest.verify()?;
    if !manifest.is_newer_than(env!("CARGO_PKG_VERSION"))? {
        return Err("RetroDrive is already up to date".to_string());
    }
    write_log(
        &state,
        &format!("update-install-start version={}", manifest.version),
    );
    if let Err(error) = install_verified_update(&state.root, &manifest, &manifest_path, &app) {
        write_update_state(&state.root, "failed", &manifest.version, &error);
        write_log(&state, &format!("update-install-failed detail={error}"));
        return Err(error);
    }
    Ok(())
}

#[tauri::command]
fn list_skills(
    state: tauri::State<'_, LauncherState>,
) -> Result<Vec<skill_sync::PublicSkill>, String> {
    skill_sync::list(&state.root)
}

#[tauri::command]
fn sync_skill(
    skill_id: String,
    state: tauri::State<'_, LauncherState>,
) -> Result<skill_sync::PublicSkill, String> {
    let result = skill_sync::sync(&state.root, &skill_id);
    match &result {
        Ok(skill) => write_log(
            &state,
            &format!("skill-sync id={} status={}", skill.id, skill.status),
        ),
        Err(error) => write_log(
            &state,
            &format!("skill-sync-failed id={skill_id} detail={error}"),
        ),
    }
    result
}

#[tauri::command]
fn sync_all_skills(
    state: tauri::State<'_, LauncherState>,
) -> Result<Vec<skill_sync::PublicSkill>, String> {
    let result = skill_sync::sync_all(&state.root);
    match &result {
        Ok(skills) => write_log(&state, &format!("skill-sync-all count={}", skills.len())),
        Err(error) => write_log(&state, &format!("skill-sync-all-failed detail={error}")),
    }
    result
}

#[tauri::command]
async fn launch_entry(
    entry_id: String,
    app: tauri::AppHandle,
    state: tauri::State<'_, LauncherState>,
) -> Result<(), String> {
    {
        let active = state
            .active
            .lock()
            .map_err(|_| "Launcher state is unavailable")?;
        if active.is_some() {
            return Err("A child session is already active".into());
        }
    }
    let entry = state
        .manifest
        .entries
        .iter()
        .find(|entry| entry.id == entry_id)
        .cloned()
        .ok_or_else(|| "Unknown launcher entry".to_string())?;
    let executable = contained_path(&state.root, &entry.executable, true)?;
    let working_directory = contained_path(&state.root, &entry.working_directory, false)?;
    if let Some(process_name) = entry.wait.process_name.as_deref() {
        if entry.wait.mode == "processName" && process_running(process_name)? {
            return Err(format!("{} is already running", entry.title));
        }
    }
    let mut child = Command::new(&executable)
        .current_dir(working_directory)
        .args(&entry.arguments)
        .spawn()
        .map_err(|error| {
            write_log(
                &state,
                &format!("launch-failed id={} detail={error}", entry.id),
            );
            format!("Unable to start {}", entry.title)
        })?;
    let pid = child.id();
    *state
        .active
        .lock()
        .map_err(|_| "Launcher state is unavailable")? = Some((entry.id.clone(), pid));
    write_log(
        &state,
        &format!(
            "process-start id={} executable={} pid={pid}",
            entry.id, entry.executable
        ),
    );
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
    let app_for_wait = app.clone();
    let entry_for_wait = entry.clone();
    thread::spawn(move || {
        let direct_result = child.wait();
        let exit_code = direct_result.as_ref().ok().and_then(|status| status.code());
        let tracked_result = if entry_for_wait.wait.mode == "processName" && direct_result.is_ok() {
            wait_for_process_session(
                entry_for_wait
                    .wait
                    .process_name
                    .as_deref()
                    .expect("validated processName policy"),
            )
        } else {
            Ok(())
        };
        if let Some(state) = app_for_wait.try_state::<LauncherState>() {
            if let Ok(mut active) = state.active.lock() {
                *active = None;
            }
            match &direct_result {
                Ok(status) => write_log(
                    &state,
                    &format!(
                        "process-exit id={} code={}",
                        entry_for_wait.id,
                        status
                            .code()
                            .map(|code| code.to_string())
                            .unwrap_or_else(|| "none".into())
                    ),
                ),
                Err(error) => write_log(
                    &state,
                    &format!(
                        "process-wait-failed id={} detail={error}",
                        entry_for_wait.id
                    ),
                ),
            }
            if let Err(error) = &tracked_result {
                write_log(
                    &state,
                    &format!(
                        "tracked-process-failed id={} detail={error}",
                        entry_for_wait.id
                    ),
                );
            }
        }
        if let Some(window) = app_for_wait.get_webview_window("main") {
            let _ = window.show();
            let _ = window.set_fullscreen(true);
            let _ = window.set_focus();
        }
        let _ = app_for_wait.emit(
            "child-session-ended",
            ChildEnded {
                entry_id: entry_for_wait.id,
                exit_code,
            },
        );
    });
    Ok(())
}

#[tauri::command]
fn close_launcher(app: tauri::AppHandle, state: tauri::State<'_, LauncherState>) {
    write_log(&state, "launcher-exit");
    app.exit(0);
}

pub fn run() {
    let state = load_state().unwrap_or_else(|error| panic!("{error}"));
    let webview_data = state.root.join("Launcher").join("data").join("webview2");
    let _ = fs::create_dir_all(&webview_data);
    std::env::set_var("WEBVIEW2_USER_DATA_FOLDER", &webview_data);
    write_log(
        &state,
        &format!("launcher-start version={}", env!("CARGO_PKG_VERSION")),
    );

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            get_launcher_state,
            list_entries,
            get_drive_overview,
            get_update_status,
            check_for_update,
            install_update,
            list_skills,
            sync_skill,
            sync_all_skills,
            launch_entry,
            close_launcher
        ])
        .run(tauri::generate_context!())
        .expect("RetroDrive launcher failed");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_path_rules_reject_escape_and_absolute_paths() {
        for value in [
            "",
            "../RetroBat.exe",
            "C:/RetroBat.exe",
            "\\\\server\\share\\app.exe",
        ] {
            assert!(safe_relative(value).is_err(), "{value} should be rejected");
        }
        assert!(safe_relative("RetroBat/RetroBat.exe").is_ok());
    }

    #[test]
    fn manifest_rejects_none_wait_and_duplicate_ids() {
        let entry = ManifestEntry {
            id: "retrobat".into(),
            entry_type: "application".into(),
            group: "home".into(),
            title: "RetroBat".into(),
            icon: "Launcher/assets/icons/retrobat.png".into(),
            executable: "RetroBat/RetroBat.exe".into(),
            working_directory: "RetroBat".into(),
            arguments: vec![],
            primary: true,
            wait: WaitPolicy {
                mode: "none".into(),
                process_name: None,
            },
        };
        let manifest = Manifest {
            schema_version: 1,
            default_entry: "retrobat".into(),
            ui: ManifestUi {
                title: "RetroDrive".into(),
                start_launches_default: true,
            },
            entries: vec![entry],
        };
        assert!(validate_manifest(&manifest).is_err());
    }

    #[test]
    fn drive_scan_counts_games_and_emulators_without_counting_metadata() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "retrodrive-overview-test-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(root.join("roms").join("nes")).expect("ROM directory should exist");
        fs::create_dir_all(root.join("RetroBat").join("emulators").join("mame"))
            .expect("emulator directory should exist");
        fs::create_dir_all(root.join("RetroArch").join("cores"))
            .expect("core directory should exist");
        fs::create_dir_all(root.join("RetroArch").join("saves"))
            .expect("save directory should exist");
        fs::write(root.join("roms").join("nes").join("game.nes"), [0_u8; 12])
            .expect("game should be written");
        fs::write(
            root.join("roms").join("nes").join("gamelist.xml"),
            "metadata",
        )
        .expect("metadata should be written");
        fs::write(
            root.join("RetroArch").join("cores").join("nes.dll"),
            [0_u8; 3],
        )
        .expect("core should be written");
        fs::write(
            root.join("RetroArch").join("saves").join("slot.sav"),
            [0_u8; 5],
        )
        .expect("save should be written");

        let overview = drive_overview(&root).expect("drive overview should be readable");

        assert_eq!(overview.platform_count, 1);
        assert_eq!(overview.systems_with_games, 1);
        assert_eq!(overview.game_files, 1);
        assert_eq!(overview.game_bytes, 12);
        assert_eq!(overview.emulator_count, 1);
        assert_eq!(overview.retro_arch_core_count, 1);
        assert_eq!(overview.save_bytes, 5);
        let _ = fs::remove_dir_all(root);
    }
}
