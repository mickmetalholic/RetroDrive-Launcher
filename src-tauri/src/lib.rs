use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::{Component, Path, PathBuf},
    process::Command,
    sync::Mutex,
    thread,
    time::{Duration, Instant},
};
use tauri::{Emitter, Manager};
use windows::Win32::{
    Foundation::CloseHandle,
    System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    },
};

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
}
