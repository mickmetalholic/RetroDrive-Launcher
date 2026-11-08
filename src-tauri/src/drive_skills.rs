use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Component, Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_FILE_BYTES: u64 = 32 * 1024 * 1024;
const MAX_SKILL_BYTES: u64 = 128 * 1024 * 1024;

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicSkill {
    pub id: String,
    pub display_name: String,
    pub description: String,
    pub status: String,
    pub source_hash: String,
    pub destination: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SkillsManifest {
    schema_version: u32,
    skills: Vec<SkillSpec>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SkillSpec {
    id: String,
    display_name: String,
    description: String,
    sha256: String,
    files: Vec<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct InstallState {
    schema_version: u32,
    #[serde(default)]
    skills: HashMap<String, String>,
}

struct DrivePaths {
    skills: PathBuf,
    backups: PathBuf,
    staging: PathBuf,
}

struct InstallChange {
    destination: PathBuf,
    backup: Option<PathBuf>,
    installed: bool,
}

fn is_link(metadata: &fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    // Reparse points include directory junctions as well as symbolic links.
    metadata.file_attributes() & 0x400 != 0
}

fn check_drive_path(root: &Path, path: &Path) -> Result<(), String> {
    let relative = path
        .strip_prefix(root)
        .map_err(|_| format!("Skill path is outside the drive: {}", path.display()))?;
    relative_string(relative)?;
    let mut current = root.to_path_buf();
    for component in relative.components() {
        current.push(component);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if is_link(&metadata) => {
                return Err(format!(
                    "Skill paths cannot use links: {}",
                    current.display()
                ));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
            Err(error) => {
                return Err(format!("Unable to inspect {}: {error}", current.display()));
            }
        }
    }
    Ok(())
}

fn safe_skill_id(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.starts_with('-')
        || value.ends_with('-')
        || value.contains("--")
        || value.chars().any(|character| {
            !character.is_ascii_lowercase() && !character.is_ascii_digit() && character != '-'
        })
    {
        return Err(format!("Invalid skill id: {value}"));
    }
    Ok(())
}

fn relative_string(path: &Path) -> Result<String, String> {
    if path
        .components()
        .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(format!("Unsafe skill path: {}", path.display()));
    }
    Ok(path
        .components()
        .map(|component| component.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/"))
}

fn collect_files(root: &Path, current: &Path, output: &mut Vec<String>) -> Result<(), String> {
    let mut entries = fs::read_dir(current)
        .map_err(|error| {
            format!(
                "Unable to enumerate skill directory {}: {error}",
                current.display()
            )
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| {
            format!(
                "Unable to read skill directory {}: {error}",
                current.display()
            )
        })?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| format!("Unable to inspect {}: {error}", path.display()))?;
        let file_type = metadata.file_type();
        if is_link(&metadata) {
            return Err(format!(
                "Symlinks are not allowed in skills: {}",
                path.display()
            ));
        }
        if file_type.is_dir() {
            collect_files(root, &path, output)?;
        } else if file_type.is_file() {
            let relative = path.strip_prefix(root).map_err(|error| error.to_string())?;
            output.push(relative_string(relative)?);
        } else {
            return Err(format!("Unsupported skill entry: {}", path.display()));
        }
    }
    Ok(())
}

fn tree_hash(root: &Path) -> Result<(String, u64), String> {
    if !root.is_dir() {
        return Err(format!("Skill directory is missing: {}", root.display()));
    }
    let mut files = Vec::new();
    collect_files(root, root, &mut files)?;
    files.sort();
    let mut hash = Sha256::new();
    let mut bytes = 0u64;
    for relative in files {
        let path = root.join(relative.replace('/', "\\"));
        let metadata = fs::metadata(&path)
            .map_err(|error| format!("Unable to inspect {}: {error}", path.display()))?;
        if metadata.len() > MAX_FILE_BYTES {
            return Err(format!("Skill file is too large: {}", path.display()));
        }
        bytes = bytes.saturating_add(metadata.len());
        if bytes > MAX_SKILL_BYTES {
            return Err(format!("Skill is too large: {}", root.display()));
        }
        let content = fs::read(&path)
            .map_err(|error| format!("Unable to read {}: {error}", path.display()))?;
        hash.update(relative.as_bytes());
        hash.update([0]);
        hash.update(content);
    }
    let digest = hash.finalize();
    Ok((
        digest.iter().map(|byte| format!("{byte:02x}")).collect(),
        bytes,
    ))
}

fn copy_tree(source: &Path, destination: &Path) -> Result<(), String> {
    fs::create_dir_all(destination)
        .map_err(|error| format!("Unable to create skill staging directory: {error}"))?;
    let mut entries = fs::read_dir(source)
        .map_err(|error| format!("Unable to enumerate {}: {error}", source.display()))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        let metadata = fs::symlink_metadata(&source_path).map_err(|error| error.to_string())?;
        let file_type = metadata.file_type();
        if is_link(&metadata) {
            return Err(format!(
                "Symlinks are not allowed in skills: {}",
                source_path.display()
            ));
        }
        if file_type.is_dir() {
            copy_tree(&source_path, &destination_path)?;
        } else if file_type.is_file() {
            fs::copy(&source_path, &destination_path).map_err(|error| {
                format!(
                    "Unable to copy {} to {}: {error}",
                    source_path.display(),
                    destination_path.display()
                )
            })?;
        } else {
            return Err(format!(
                "Unsupported skill entry: {}",
                source_path.display()
            ));
        }
    }
    Ok(())
}

fn source_root(root: &Path) -> PathBuf {
    root.join("Launcher").join("skills")
}

fn load_manifest(root: &Path) -> Result<Option<SkillsManifest>, String> {
    let directory = source_root(root);
    check_drive_path(root, &directory)?;
    if !directory.is_dir() {
        return Ok(None);
    }
    let manifest_path = directory.join("skills-manifest.json");
    check_drive_path(root, &manifest_path)?;
    let manifest: SkillsManifest = serde_json::from_slice(
        &fs::read(&manifest_path)
            .map_err(|error| format!("Unable to read {}: {error}", manifest_path.display()))?,
    )
    .map_err(|error| format!("Invalid skills manifest: {error}"))?;
    if manifest.schema_version != 1 {
        return Err(format!(
            "Unsupported skills manifest schema: {}",
            manifest.schema_version
        ));
    }
    let mut ids = HashSet::new();
    for spec in &manifest.skills {
        safe_skill_id(&spec.id)?;
        if !ids.insert(spec.id.as_str()) {
            return Err(format!("Duplicate skill id: {}", spec.id));
        }
        if spec.files.is_empty() || spec.sha256.len() != 64 {
            return Err(format!("Invalid skill manifest entry: {}", spec.id));
        }
        let skill_path = directory.join(&spec.id);
        check_drive_path(root, &skill_path)?;
        if !skill_path.is_dir() {
            return Err(format!(
                "Skill directory is missing: {}",
                skill_path.display()
            ));
        }
        let mut expected = spec.files.clone();
        expected.sort();
        expected.dedup();
        if expected.len() != spec.files.len() {
            return Err(format!("Duplicate files in skill manifest: {}", spec.id));
        }
        for file in &spec.files {
            let relative = PathBuf::from(file.replace('/', "\\"));
            let normalized = relative_string(&relative)?;
            if normalized != *file {
                return Err(format!("Non-normalized skill file path: {file}"));
            }
            let path = skill_path.join(relative);
            if !path.is_file() {
                return Err(format!("Manifest file is missing: {}", path.display()));
            }
        }
        let mut actual = Vec::new();
        collect_files(&skill_path, &skill_path, &mut actual)?;
        actual.sort();
        if actual != expected {
            return Err(format!(
                "Skill manifest file list does not match: {}",
                spec.id
            ));
        }
        let (hash, _) = tree_hash(&skill_path)?;
        if hash != spec.sha256 {
            return Err(format!("Skill hash does not match manifest: {}", spec.id));
        }
    }
    Ok(Some(manifest))
}

fn drive_paths(root: &Path) -> Result<DrivePaths, String> {
    let paths = DrivePaths {
        skills: root.join(".agents").join("skills"),
        backups: root.join("Launcher").join("data").join("skill-backups"),
        staging: root.join("Launcher").join("data").join("skill-staging"),
    };
    for path in [&paths.skills, &paths.backups, &paths.staging] {
        check_drive_path(root, path)?;
    }
    Ok(paths)
}

fn state_path(root: &Path) -> PathBuf {
    root.join("Launcher")
        .join("data")
        .join("drive-skills-state.json")
}

fn load_install_state(root: &Path) -> Result<InstallState, String> {
    let path = state_path(root);
    check_drive_path(root, &path)?;
    if !path.is_file() {
        return Ok(InstallState {
            schema_version: 1,
            skills: HashMap::new(),
        });
    }
    let state: InstallState =
        serde_json::from_slice(&fs::read(&path).map_err(|error| error.to_string())?)
            .map_err(|error| format!("Invalid drive skill state: {error}"))?;
    if state.schema_version != 1 {
        return Err(format!(
            "Unsupported drive skill state schema: {}",
            state.schema_version
        ));
    }
    Ok(state)
}

fn save_install_state(root: &Path, state: &InstallState) -> Result<(), String> {
    let path = state_path(root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let temporary = path.with_extension("json.tmp");
    check_drive_path(root, &path)?;
    check_drive_path(root, &temporary)?;
    fs::write(
        &temporary,
        serde_json::to_vec_pretty(state).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    fs::rename(&temporary, &path).map_err(|error| error.to_string())
}

fn skill_status(paths: &DrivePaths, state: &InstallState, spec: &SkillSpec) -> PublicSkill {
    let destination = paths.skills.join(&spec.id);
    let status = match fs::symlink_metadata(&destination) {
        Ok(metadata) if is_link(&metadata) => "error".to_string(),
        Ok(metadata) if metadata.is_dir() => match tree_hash(&destination) {
            Ok((hash, _)) if hash == spec.sha256 => "upToDate".to_string(),
            Ok((hash, _)) if state.skills.get(&spec.id) == Some(&hash) => {
                "updateAvailable".to_string()
            }
            Ok(_) => "localChanges".to_string(),
            Err(_) => "error".to_string(),
        },
        Ok(_) => "error".to_string(),
        Err(_) => "notInstalled".to_string(),
    };
    PublicSkill {
        id: spec.id.clone(),
        display_name: spec.display_name.clone(),
        description: spec.description.clone(),
        status,
        source_hash: spec.sha256.clone(),
        destination: destination.to_string_lossy().into_owned(),
    }
}

pub fn list(root: &Path) -> Result<Vec<PublicSkill>, String> {
    let Some(manifest) = load_manifest(root)? else {
        return Ok(Vec::new());
    };
    let paths = drive_paths(root)?;
    let state = load_install_state(root)?;
    Ok(manifest
        .skills
        .iter()
        .map(|spec| skill_status(&paths, &state, spec))
        .collect())
}

fn install_one(
    root: &Path,
    paths: &DrivePaths,
    state: &mut InstallState,
    spec: &SkillSpec,
) -> Result<InstallChange, String> {
    let source = source_root(root).join(&spec.id);
    let destination = paths.skills.join(&spec.id);
    let current_hash = match fs::symlink_metadata(&destination) {
        Ok(metadata) if is_link(&metadata) => {
            return Err(format!(
                "Refusing to replace a symlink: {}",
                destination.display()
            ))
        }
        Ok(metadata) if metadata.is_dir() => Some(tree_hash(&destination)?.0),
        Ok(_) => {
            return Err(format!(
                "Destination is not a directory: {}",
                destination.display()
            ))
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => {
            return Err(format!(
                "Unable to inspect {}: {error}",
                destination.display()
            ))
        }
    };
    if let Some(hash) = &current_hash {
        if hash != &spec.sha256 && state.skills.get(&spec.id) != Some(hash) {
            return Err(format!(
                "Local changes detected; refusing to overwrite {}",
                destination.display()
            ));
        }
        if hash == &spec.sha256 {
            state.skills.insert(spec.id.clone(), spec.sha256.clone());
            return Ok(InstallChange {
                destination,
                backup: None,
                installed: false,
            });
        }
    }
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let stage = paths.staging.join(format!("{}-{timestamp}", spec.id));
    check_drive_path(root, &destination)?;
    check_drive_path(root, &stage)?;
    fs::create_dir_all(&paths.skills).map_err(|error| error.to_string())?;
    copy_tree(&source, &stage)?;
    let (staged_hash, _) = tree_hash(&stage)?;
    if staged_hash != spec.sha256 {
        let _ = fs::remove_dir_all(&stage);
        return Err(format!("Staged skill hash does not match: {}", spec.id));
    }
    let backup = paths.backups.join(format!("{}-{timestamp}", spec.id));
    check_drive_path(root, &backup)?;
    let had_destination = destination.exists();
    if had_destination {
        if let Some(parent) = backup.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        fs::rename(&destination, &backup)
            .map_err(|error| format!("Unable to back up skill: {error}"))?;
    }
    if let Err(error) = fs::rename(&stage, &destination) {
        if had_destination {
            let _ = fs::rename(&backup, &destination);
        }
        let _ = fs::remove_dir_all(&stage);
        return Err(format!("Unable to install skill: {error}"));
    }
    state.skills.insert(spec.id.clone(), spec.sha256.clone());
    Ok(InstallChange {
        destination,
        backup: if had_destination { Some(backup) } else { None },
        installed: true,
    })
}

fn rollback_change(change: &InstallChange) -> Result<(), String> {
    if !change.installed {
        return Ok(());
    }
    if change.destination.exists() {
        fs::remove_dir_all(&change.destination).map_err(|error| {
            format!(
                "Unable to remove partially installed skill {}: {error}",
                change.destination.display()
            )
        })?;
    }
    if let Some(backup) = &change.backup {
        fs::rename(backup, &change.destination).map_err(|error| {
            format!(
                "Unable to restore previous skill {}: {error}",
                change.destination.display()
            )
        })?;
    }
    Ok(())
}

pub fn install_bundled(root: &Path) -> Result<Vec<PublicSkill>, String> {
    let Some(manifest) = load_manifest(root)? else {
        return Ok(Vec::new());
    };
    let paths = drive_paths(root)?;
    let mut state = load_install_state(root)?;
    let previous_state = state.clone();
    let mut changes = Vec::new();
    for spec in &manifest.skills {
        match install_one(root, &paths, &mut state, spec) {
            Ok(change) => changes.push(change),
            Err(error) => {
                let rollback_errors = changes
                    .iter()
                    .rev()
                    .filter_map(|change| rollback_change(change).err())
                    .collect::<Vec<_>>();
                let suffix = if rollback_errors.is_empty() {
                    String::new()
                } else {
                    format!(" Rollback errors: {}", rollback_errors.join("; "))
                };
                return Err(format!("{error}.{suffix}"));
            }
        }
    }
    if let Err(error) = if state == previous_state {
        Ok(())
    } else {
        save_install_state(root, &state)
    } {
        let rollback_errors = changes
            .iter()
            .rev()
            .filter_map(|change| rollback_change(change).err())
            .collect::<Vec<_>>();
        let suffix = if rollback_errors.is_empty() {
            String::new()
        } else {
            format!(" Rollback errors: {}", rollback_errors.join("; "))
        };
        return Err(format!(
            "Unable to save drive skill state: {error}.{suffix}"
        ));
    }
    Ok(manifest
        .skills
        .iter()
        .map(|spec| skill_status(&paths, &state, spec))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        env,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn temp_root() -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = env::temp_dir().join(format!("retrodrive-skill-test-{suffix}"));
        fs::create_dir_all(&root).unwrap();
        root
    }

    fn bundled_root() -> PathBuf {
        let repository_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
        let root = temp_root();
        copy_tree(&repository_root.join("skills"), &source_root(&root)).unwrap();
        root
    }

    fn refresh_manifest(root: &Path) {
        let path = source_root(root).join("skills-manifest.json");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        for spec in manifest["skills"].as_array_mut().unwrap() {
            let skill_path = source_root(root).join(spec["id"].as_str().unwrap());
            let (hash, bytes) = tree_hash(&skill_path).unwrap();
            let mut files = Vec::new();
            collect_files(&skill_path, &skill_path, &mut files).unwrap();
            files.sort();
            spec["sha256"] = hash.into();
            spec["bytes"] = bytes.into();
            spec["files"] = serde_json::to_value(files).unwrap();
        }
        fs::write(path, serde_json::to_vec(&manifest).unwrap()).unwrap();
    }

    #[test]
    fn skill_ids_are_strictly_normalized() {
        assert!(safe_skill_id("retrodrive-import-game").is_ok());
        assert!(safe_skill_id("../escape").is_err());
        assert!(safe_skill_id("RetroDrive").is_err());
        assert!(safe_skill_id("a--b").is_err());
    }

    #[test]
    fn tree_hash_includes_relative_paths() {
        let root = temp_root();
        fs::create_dir_all(root.join("nested")).unwrap();
        fs::write(root.join("a.txt"), b"same").unwrap();
        fs::write(root.join("nested/a.txt"), b"same").unwrap();
        let (hash, bytes) = tree_hash(&root).unwrap();
        assert_eq!(bytes, 8);
        assert_eq!(hash.len(), 64);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn bundled_manifest_matches_rust_tree_hash() {
        let repository_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
        let portable_root = temp_root();
        copy_tree(
            &repository_root.join("skills"),
            &portable_root.join("Launcher/skills"),
        )
        .expect("bundled skills should be copied into the test portable root");
        let manifest = load_manifest(&portable_root)
            .expect("bundled skill manifest should validate")
            .expect("bundled skill manifest should be present");
        assert_eq!(manifest.skills.len(), 1);
        assert_eq!(manifest.skills[0].id, "retrodrive-import-game");
        let _ = fs::remove_dir_all(portable_root);
    }

    #[test]
    fn installs_to_drive_root_and_leaves_unchanged_installations_alone() {
        let root = bundled_root();
        let foreign = root.join(".agents/skills/personal/SKILL.md");
        fs::create_dir_all(foreign.parent().unwrap()).unwrap();
        fs::write(&foreign, b"personal skill").unwrap();
        let skills = install_bundled(&root).unwrap();
        let destination = root.join(".agents/skills/retrodrive-import-game");
        assert_eq!(PathBuf::from(&skills[0].destination), destination);
        assert_eq!(skills[0].status, "upToDate");
        assert!(destination.join("SKILL.md").is_file());
        let modified = fs::metadata(state_path(&root)).unwrap().modified().unwrap();
        assert_eq!(install_bundled(&root).unwrap()[0].status, "upToDate");
        assert_eq!(
            fs::metadata(state_path(&root)).unwrap().modified().unwrap(),
            modified
        );
        assert!(!drive_paths(&root).unwrap().backups.exists());
        assert_eq!(fs::read(foreign).unwrap(), b"personal skill");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn upgrades_managed_skills_and_backs_up_removed_files() {
        let root = bundled_root();
        install_bundled(&root).unwrap();
        let source = source_root(&root).join("retrodrive-import-game");
        fs::remove_file(source.join("references/xbox.md")).unwrap();
        fs::write(source.join("new.txt"), b"new version").unwrap();
        refresh_manifest(&root);
        assert_eq!(list(&root).unwrap()[0].status, "updateAvailable");
        let skill = install_bundled(&root).unwrap().remove(0);
        assert_eq!(skill.status, "upToDate");
        let destination = PathBuf::from(&skill.destination);
        assert!(!destination.join("references/xbox.md").exists());
        assert_eq!(
            fs::read(destination.join("new.txt")).unwrap(),
            b"new version"
        );
        let backups = drive_paths(&root).unwrap().backups;
        let backup = fs::read_dir(backups)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        assert!(backup.join("references/xbox.md").is_file());
        assert_eq!(
            tree_hash(&destination).unwrap().0,
            load_manifest(&root).unwrap().unwrap().skills[0].sha256
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn refuses_modified_drive_skills_without_changing_them() {
        let root = bundled_root();
        let destination = root.join(".agents/skills/retrodrive-import-game");
        fs::create_dir_all(&destination).unwrap();
        fs::write(destination.join("SKILL.md"), b"user skill").unwrap();
        assert!(install_bundled(&root)
            .unwrap_err()
            .contains("Local changes"));
        assert_eq!(
            fs::read(destination.join("SKILL.md")).unwrap(),
            b"user skill"
        );
        assert!(!state_path(&root).exists());
        // The same protection applies to a previously managed copy edited by its user.
        fs::remove_dir_all(&destination).unwrap();
        install_bundled(&root).unwrap();
        fs::write(destination.join("local.txt"), b"local change").unwrap();
        let before = tree_hash(&destination).unwrap().0;
        assert!(install_bundled(&root)
            .unwrap_err()
            .contains("Local changes"));
        assert_eq!(tree_hash(&destination).unwrap().0, before);
        assert_eq!(list(&root).unwrap()[0].status, "localChanges");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn restores_previous_drive_skill_if_saving_state_fails() {
        let root = bundled_root();
        install_bundled(&root).unwrap();
        let destination = root.join(".agents/skills/retrodrive-import-game");
        let before = tree_hash(&destination).unwrap().0;
        let state_before = fs::read(state_path(&root)).unwrap();
        fs::write(
            source_root(&root).join("retrodrive-import-game/new.txt"),
            b"new",
        )
        .unwrap();
        refresh_manifest(&root);
        fs::create_dir_all(state_path(&root).with_extension("json.tmp")).unwrap();
        assert!(install_bundled(&root)
            .unwrap_err()
            .contains("Unable to save"));
        assert_eq!(tree_hash(&destination).unwrap().0, before);
        assert_eq!(fs::read(state_path(&root)).unwrap(), state_before);
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn missing_bundle_does_not_create_drive_skill_directories() {
        let root = temp_root();
        assert!(install_bundled(&root).unwrap().is_empty());
        assert!(!root.join(".agents").exists());
        assert!(!state_path(&root).exists());
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn refuses_junctions_that_redirect_drive_skills_outside_the_root() {
        let root = bundled_root();
        let outside = temp_root();
        let link = root.join(".agents");
        let output = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(&outside)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(install_bundled(&root)
            .unwrap_err()
            .contains("cannot use links"));
        assert_eq!(fs::read_dir(&outside).unwrap().count(), 0);
        fs::remove_dir(link).unwrap();
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(outside);
    }
}
