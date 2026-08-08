use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    env, fs,
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

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct SyncState {
    schema_version: u32,
    #[serde(default)]
    skills: HashMap<String, String>,
}

struct CodexPaths {
    home: PathBuf,
    skills: PathBuf,
}

struct SyncChange {
    destination: PathBuf,
    backup: Option<PathBuf>,
    installed: bool,
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
        let file_type = fs::symlink_metadata(&path)
            .map_err(|error| format!("Unable to inspect {}: {error}", path.display()))?
            .file_type();
        if file_type.is_symlink() {
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
        let file_type = fs::symlink_metadata(&source_path)
            .map_err(|error| error.to_string())?
            .file_type();
        if file_type.is_symlink() {
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
    if !directory.is_dir() {
        return Ok(None);
    }
    let manifest_path = directory.join("skills-manifest.json");
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

fn codex_paths() -> Result<CodexPaths, String> {
    let home = if let Some(value) = env::var_os("CODEX_HOME") {
        PathBuf::from(value)
    } else if let Some(value) = env::var_os("USERPROFILE") {
        PathBuf::from(value).join(".codex")
    } else {
        return Err("Unable to resolve Codex home; set CODEX_HOME or USERPROFILE".into());
    };
    Ok(CodexPaths {
        skills: home.join("skills"),
        home,
    })
}

fn state_path(root: &Path) -> PathBuf {
    root.join("Launcher")
        .join("data")
        .join("skill-sync-state.json")
}

fn load_sync_state(root: &Path) -> Result<SyncState, String> {
    let path = state_path(root);
    if !path.is_file() {
        return Ok(SyncState {
            schema_version: 1,
            skills: HashMap::new(),
        });
    }
    let state: SyncState =
        serde_json::from_slice(&fs::read(&path).map_err(|error| error.to_string())?)
            .map_err(|error| format!("Invalid skill sync state: {error}"))?;
    if state.schema_version != 1 {
        return Err(format!(
            "Unsupported skill sync state schema: {}",
            state.schema_version
        ));
    }
    Ok(state)
}

fn save_sync_state(root: &Path, state: &SyncState) -> Result<(), String> {
    let path = state_path(root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let temporary = path.with_extension("json.tmp");
    fs::write(
        &temporary,
        serde_json::to_vec_pretty(state).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;
    if path.exists() {
        fs::remove_file(&path).map_err(|error| error.to_string())?;
    }
    fs::rename(&temporary, &path).map_err(|error| error.to_string())
}

fn skill_status(paths: &CodexPaths, state: &SyncState, spec: &SkillSpec) -> PublicSkill {
    let destination = paths.skills.join(&spec.id);
    let status = match fs::symlink_metadata(&destination) {
        Ok(metadata) if metadata.file_type().is_symlink() => "error".to_string(),
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
    let paths = codex_paths()?;
    let state = load_sync_state(root)?;
    Ok(manifest
        .skills
        .iter()
        .map(|spec| skill_status(&paths, &state, spec))
        .collect())
}

fn sync_one(
    root: &Path,
    paths: &CodexPaths,
    state: &mut SyncState,
    spec: &SkillSpec,
) -> Result<SyncChange, String> {
    let source = source_root(root).join(&spec.id);
    let destination = paths.skills.join(&spec.id);
    let current_hash = match fs::symlink_metadata(&destination) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
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
        Err(_) => None,
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
            return Ok(SyncChange {
                destination,
                backup: None,
                installed: false,
            });
        }
    }
    fs::create_dir_all(&paths.skills).map_err(|error| error.to_string())?;
    let stage = paths.skills.join(format!(
        ".{}.retrodrive-stage-{}",
        spec.id,
        std::process::id()
    ));
    if stage.exists() {
        fs::remove_dir_all(&stage).map_err(|error| error.to_string())?;
    }
    copy_tree(&source, &stage)?;
    let (staged_hash, _) = tree_hash(&stage)?;
    if staged_hash != spec.sha256 {
        let _ = fs::remove_dir_all(&stage);
        return Err(format!("Staged skill hash does not match: {}", spec.id));
    }
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let backup = paths
        .home
        .join("retrodrive-skill-backups")
        .join(format!("{}-{timestamp}", spec.id));
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
    Ok(SyncChange {
        destination,
        backup: if had_destination { Some(backup) } else { None },
        installed: true,
    })
}

fn rollback_change(change: &SyncChange) -> Result<(), String> {
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

pub fn sync(root: &Path, id: &str) -> Result<PublicSkill, String> {
    let Some(manifest) = load_manifest(root)? else {
        return Err("The portable Launcher does not contain a skills manifest".into());
    };
    let paths = codex_paths()?;
    let mut state = load_sync_state(root)?;
    let spec = manifest
        .skills
        .iter()
        .find(|spec| spec.id == id)
        .ok_or_else(|| format!("Unknown skill: {id}"))?;
    let change = sync_one(root, &paths, &mut state, spec)?;
    if let Err(error) = save_sync_state(root, &state) {
        let rollback_error = rollback_change(&change).err();
        let suffix = rollback_error
            .map(|rollback| format!(" Rollback failed: {rollback}"))
            .unwrap_or_default();
        return Err(format!("Unable to save skill sync state: {error}.{suffix}"));
    }
    Ok(skill_status(&paths, &state, spec))
}

pub fn sync_all(root: &Path) -> Result<Vec<PublicSkill>, String> {
    let Some(manifest) = load_manifest(root)? else {
        return Err("The portable Launcher does not contain a skills manifest".into());
    };
    let paths = codex_paths()?;
    let mut state = load_sync_state(root)?;
    let mut changes = Vec::new();
    for spec in &manifest.skills {
        match sync_one(root, &paths, &mut state, spec) {
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
    if let Err(error) = save_sync_state(root, &state) {
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
        return Err(format!("Unable to save skill sync state: {error}.{suffix}"));
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
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_root() -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = env::temp_dir().join(format!("retrodrive-skill-test-{suffix}"));
        fs::create_dir_all(&root).unwrap();
        root
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
    fn sync_stages_and_refuses_untracked_destination_changes() {
        let repository_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
        let portable_root = temp_root();
        copy_tree(
            &repository_root.join("skills"),
            &portable_root.join("Launcher/skills"),
        )
        .unwrap();
        let codex_home = temp_root();
        let paths = CodexPaths {
            skills: codex_home.join("skills"),
            home: codex_home.clone(),
        };
        let manifest = load_manifest(&portable_root).unwrap().unwrap();
        let spec = &manifest.skills[0];
        let mut state = SyncState {
            schema_version: 1,
            skills: HashMap::new(),
        };
        sync_one(&portable_root, &paths, &mut state, spec).unwrap();
        assert_eq!(
            tree_hash(&paths.skills.join(&spec.id)).unwrap().0,
            spec.sha256
        );
        fs::write(
            paths.skills.join(&spec.id).join("local.txt"),
            b"local change",
        )
        .unwrap();
        assert!(sync_one(&portable_root, &paths, &mut state, spec).is_err());
        let _ = fs::remove_dir_all(portable_root);
        let _ = fs::remove_dir_all(codex_home);
    }
}
