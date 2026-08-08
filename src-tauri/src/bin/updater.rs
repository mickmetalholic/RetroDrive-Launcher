#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[path = "../update_common.rs"]
mod update_common;

use std::{
    env,
    fs::{self, File},
    io,
    path::{Component, Path, PathBuf},
    process::Command,
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use update_common::{verify_package, UpdateManifest};
use windows::{
    core::PCWSTR,
    Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK},
};
use zip::ZipArchive;

const REQUIRED_FILES: [&str; 3] = [
    "START.exe",
    "Launcher/RetroDriveLauncher.exe",
    "Launcher/Updater.exe",
];

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn message(text: &str) {
    let text = wide(text);
    let title = wide("RetroDrive Update");
    unsafe {
        MessageBoxW(
            None,
            PCWSTR(text.as_ptr()),
            PCWSTR(title.as_ptr()),
            MB_OK | MB_ICONERROR,
        );
    }
}

fn argument(name: &str) -> Result<PathBuf, String> {
    let args: Vec<String> = env::args().collect();
    let index = args
        .iter()
        .position(|argument| argument == name)
        .ok_or_else(|| format!("Missing argument {name}"))?;
    args.get(index + 1)
        .map(PathBuf::from)
        .ok_or_else(|| format!("Missing value for {name}"))
}

fn canonical_file_within(path: &Path, directory: &Path) -> Result<PathBuf, String> {
    let canonical = path
        .canonicalize()
        .map_err(|error| format!("Unable to resolve {}: {error}", path.display()))?;
    if !canonical.is_file() || !canonical.starts_with(directory) {
        return Err(format!(
            "Update input is outside the protected update directory: {}",
            path.display()
        ));
    }
    Ok(canonical)
}

fn normalized_relative(path: &Path) -> Option<String> {
    if path
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return None;
    }
    Some(
        path.components()
            .map(|component| component.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/"),
    )
}

fn allowed_file(relative: &str) -> bool {
    REQUIRED_FILES.contains(&relative)
        || matches!(
            relative,
            "Launcher/assets/icons/retrobat.png"
                | "Launcher/assets/icons/retroarch.png"
                | "Launcher/release/USER-GUIDE.md"
                | "Launcher/release/VERSION.json"
        )
        || relative
            .strip_prefix("Launcher/skills/")
            .is_some_and(|skill_relative| !skill_relative.is_empty())
}

fn allowed_directory(relative: &str) -> bool {
    let normalized = relative.trim_end_matches('/');
    matches!(
        normalized,
        "Launcher" | "Launcher/assets" | "Launcher/assets/icons" | "Launcher/release"
    ) || normalized == "Launcher/skills"
        || normalized
            .strip_prefix("Launcher/skills/")
            .is_some_and(|path| !path.is_empty())
}

fn extract_archive(archive: &Path, staging: &Path) -> Result<Vec<PathBuf>, String> {
    let file = File::open(archive)
        .map_err(|error| format!("Unable to open {}: {error}", archive.display()))?;
    let mut zip =
        ZipArchive::new(file).map_err(|error| format!("Unable to read update archive: {error}"))?;
    let mut extracted = Vec::new();
    for index in 0..zip.len() {
        let mut entry = zip
            .by_index(index)
            .map_err(|error| format!("Unable to read archive entry {index}: {error}"))?;
        let relative_path = entry
            .enclosed_name()
            .ok_or_else(|| "Update archive contains an unsafe path".to_string())?;
        let relative = normalized_relative(&relative_path)
            .ok_or_else(|| "Update archive contains an unsafe path".to_string())?;
        let destination = staging.join(&relative_path);
        if entry.is_dir() {
            if !allowed_directory(&relative) {
                return Err(format!(
                    "Update archive contains an unexpected directory: {relative}"
                ));
            }
            fs::create_dir_all(&destination)
                .map_err(|error| format!("Unable to create {}: {error}", destination.display()))?;
            continue;
        }
        if !allowed_file(&relative) || extracted.iter().any(|path| path == &relative_path) {
            return Err(format!(
                "Update archive contains an unexpected file: {relative}"
            ));
        }
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("Unable to create {}: {error}", parent.display()))?;
        }
        let mut output = File::create(&destination)
            .map_err(|error| format!("Unable to create {}: {error}", destination.display()))?;
        io::copy(&mut entry, &mut output)
            .map_err(|error| format!("Unable to extract {relative}: {error}"))?;
        extracted.push(relative_path);
    }
    for required in REQUIRED_FILES {
        if !extracted
            .iter()
            .any(|path| normalized_relative(path).as_deref() == Some(required))
        {
            return Err(format!("Update archive is missing {required}"));
        }
    }
    Ok(extracted)
}

#[derive(Debug)]
struct AppliedFile {
    target: PathBuf,
    backup: Option<PathBuf>,
}

fn rollback(applied: &mut [AppliedFile]) -> Vec<String> {
    let mut errors = Vec::new();
    for change in applied.iter().rev() {
        if change.target.exists() {
            if let Err(error) = fs::remove_file(&change.target) {
                errors.push(format!(
                    "Unable to remove {}: {error}",
                    change.target.display()
                ));
                continue;
            }
        }
        if let Some(backup) = &change.backup {
            if let Err(error) = fs::rename(backup, &change.target) {
                errors.push(format!(
                    "Unable to restore {}: {error}",
                    change.target.display()
                ));
            }
        }
    }
    errors
}

fn install_one(
    root: &Path,
    staging: &Path,
    backup_root: &Path,
    relative: &Path,
) -> Result<AppliedFile, String> {
    let source = staging.join(relative);
    let target = root.join(relative);
    let backup = backup_root.join(relative);
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("Unable to create {}: {error}", parent.display()))?;
    }
    let existing_backup = if target.exists() {
        if let Some(parent) = backup.parent() {
            fs::create_dir_all(parent).map_err(|error| {
                format!(
                    "Unable to create backup directory {}: {error}",
                    parent.display()
                )
            })?;
        }
        fs::rename(&target, &backup)
            .map_err(|error| format!("Unable to back up {}: {error}", target.display()))?;
        Some(backup)
    } else {
        None
    };
    if let Err(error) = fs::rename(&source, &target) {
        if let Some(backup) = &existing_backup {
            let _ = fs::rename(backup, &target);
        }
        return Err(format!("Unable to install {}: {error}", target.display()));
    }
    Ok(AppliedFile {
        target,
        backup: existing_backup,
    })
}

fn install_files(
    root: &Path,
    staging: &Path,
    backup_root: &Path,
    files: &[PathBuf],
) -> Result<(), String> {
    if backup_root.exists() {
        fs::remove_dir_all(backup_root)
            .map_err(|error| format!("Unable to clear previous update backup: {error}"))?;
    }
    fs::create_dir_all(backup_root)
        .map_err(|error| format!("Unable to create update backup: {error}"))?;
    let mut ordered = files.to_vec();
    ordered.sort_by_key(|path| {
        let relative = normalized_relative(path).unwrap_or_default();
        match relative.as_str() {
            "Launcher/RetroDriveLauncher.exe" => 0,
            "START.exe" => 1,
            "Launcher/Updater.exe" => 2,
            _ => 3,
        }
    });
    let mut applied = Vec::new();
    for relative in ordered {
        match install_one(root, staging, backup_root, &relative) {
            Ok(change) => applied.push(change),
            Err(error) => {
                let rollback_errors = rollback(&mut applied);
                let suffix = if rollback_errors.is_empty() {
                    String::new()
                } else {
                    format!(" Rollback errors: {}", rollback_errors.join("; "))
                };
                return Err(format!("{error}.{suffix}"));
            }
        }
    }
    Ok(())
}

fn state_json(status: &str, version: &str, detail: &str) -> String {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    serde_json::json!({
        "schemaVersion": 1,
        "checkedAtUnix": timestamp,
        "status": status,
        "version": version,
        "detail": detail,
    })
    .to_string()
}

fn launch_start(root: &Path) {
    if env::var("RETRODRIVE_UPDATER_NO_RELAUNCH").as_deref() == Ok("1") {
        return;
    }
    let _ = Command::new(root.join("START.exe"))
        .current_dir(root)
        .spawn();
}

fn run() -> Result<(PathBuf, String), String> {
    thread::sleep(Duration::from_millis(900));
    let root = argument("--root")?
        .canonicalize()
        .map_err(|error| format!("Unable to resolve RetroDrive root: {error}"))?;
    let updates = root.join("Launcher").join("data").join("updates");
    fs::create_dir_all(&updates)
        .map_err(|error| format!("Unable to create update workspace: {error}"))?;
    let canonical_updates = updates
        .canonicalize()
        .map_err(|error| format!("Unable to resolve update workspace: {error}"))?;
    let manifest_path = canonical_file_within(&argument("--manifest")?, &canonical_updates)?;
    let archive_path = canonical_file_within(&argument("--archive")?, &canonical_updates)?;
    let manifest: UpdateManifest = serde_json::from_slice(
        &fs::read(&manifest_path)
            .map_err(|error| format!("Unable to read signed update manifest: {error}"))?,
    )
    .map_err(|error| format!("Unable to parse signed update manifest: {error}"))?;
    manifest.verify()?;
    verify_package(&archive_path, &manifest)?;

    let staging = canonical_updates.join("staging");
    if staging.exists() {
        fs::remove_dir_all(&staging)
            .map_err(|error| format!("Unable to clear update staging directory: {error}"))?;
    }
    fs::create_dir_all(&staging)
        .map_err(|error| format!("Unable to create update staging directory: {error}"))?;
    let files = extract_archive(&archive_path, &staging)?;
    install_files(&root, &staging, &canonical_updates.join("previous"), &files)?;
    let state = root.join("Launcher").join("data").join("update-state.json");
    let _ = fs::write(
        state,
        state_json(
            "installed",
            &manifest.version,
            "Update installed successfully",
        ),
    );
    let _ = fs::remove_file(&archive_path);
    let _ = fs::remove_file(&manifest_path);
    let _ = fs::remove_dir_all(&staging);
    Ok((root, manifest.version))
}

fn main() {
    let root_hint = argument("--root").ok();
    match run() {
        Ok((root, _version)) => launch_start(&root),
        Err(error) => {
            if let Some(root) = &root_hint {
                let state = root.join("Launcher").join("data").join("update-state.json");
                let _ = fs::write(
                    state,
                    state_json("failed", env!("CARGO_PKG_VERSION"), &error),
                );
                launch_start(root);
            }
            message(&format!(
                "The update could not be installed. RetroDrive will continue with the previous version.\n\n{error}"
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_root() -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos();
        let root = env::temp_dir().join(format!(
            "retrodrive-updater-test-{}-{unique}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("test root should be created");
        root
    }

    #[test]
    fn package_paths_are_strictly_limited() {
        assert!(allowed_file("START.exe"));
        assert!(allowed_file("Launcher/Updater.exe"));
        assert!(!allowed_file("launcher.json"));
        assert!(!allowed_file("RetroBat/RetroBat.exe"));
        assert!(!allowed_file("Launcher/data/update-state.json"));
        assert!(allowed_file(
            "Launcher/skills/retrodrive-import-game/SKILL.md"
        ));
        assert!(!allowed_file("Launcher/skills"));
        assert!(allowed_directory(
            "Launcher/skills/retrodrive-import-game/agents"
        ));
    }

    #[test]
    fn relative_paths_reject_parent_and_root_components() {
        assert!(normalized_relative(Path::new("Launcher/Updater.exe")).is_some());
        assert!(normalized_relative(Path::new("../START.exe")).is_none());
        assert!(normalized_relative(Path::new("C:/START.exe")).is_none());
    }

    #[test]
    fn transaction_replaces_only_listed_files_and_keeps_backups() {
        let root = test_root();
        let staging = root.join("Launcher/data/updates/staging");
        let backup = root.join("Launcher/data/updates/previous");
        let files: Vec<PathBuf> = REQUIRED_FILES.iter().map(PathBuf::from).collect();
        for relative in &files {
            let installed = root.join(relative);
            let staged = staging.join(relative);
            fs::create_dir_all(installed.parent().unwrap()).unwrap();
            fs::create_dir_all(staged.parent().unwrap()).unwrap();
            fs::write(installed, b"old").unwrap();
            fs::write(staged, b"new").unwrap();
        }
        fs::write(root.join("launcher.json"), b"user config").unwrap();

        install_files(&root, &staging, &backup, &files).unwrap();

        for relative in &files {
            assert_eq!(fs::read(root.join(relative)).unwrap(), b"new");
            assert_eq!(fs::read(backup.join(relative)).unwrap(), b"old");
        }
        assert_eq!(
            fs::read(root.join("launcher.json")).unwrap(),
            b"user config"
        );
        let _ = fs::remove_dir_all(root);
    }
}
