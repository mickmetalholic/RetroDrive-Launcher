use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    os::windows::fs::MetadataExt,
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::UNIX_EPOCH,
};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupFile {
    pub path: String,
    pub bytes: u64,
    pub modified: u128,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct DriveLink {
    pub path: String,
    pub target: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub token: String,
    pub files: Vec<BackupFile>,
    pub directories: Vec<String>,
    pub links: Vec<DriveLink>,
    pub excluded: BTreeMap<String, u64>,
    pub warnings: Vec<String>,
    pub bytes: u64,
}

pub fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
pub fn is_link(metadata: &fs::Metadata) -> bool {
    metadata.file_attributes() & 0x400 != 0
}
pub fn safe_path(root: &Path, relative: &str) -> Result<PathBuf, String> {
    if relative.is_empty() || relative.contains(':') || relative.starts_with(['/', '\\']) {
        return Err("Expected a drive-relative path".into());
    }
    let path = PathBuf::from(relative.replace('/', "\\"));
    if path
        .components()
        .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err("Unsafe backup path".into());
    }
    let mut full = root.to_path_buf();
    for part in path.components() {
        full.push(part);
        match fs::symlink_metadata(&full) {
            Ok(meta) if is_link(&meta) => {
                return Err(format!(
                    "Backup paths cannot traverse links: {}",
                    full.display()
                ))
            }
            Ok(_) => (),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => break,
            Err(e) => return Err(e.to_string()),
        }
    }
    Ok(root.join(path))
}

fn excluded_directory(path: &str) -> Option<&'static str> {
    let lower = path.to_ascii_lowercase();
    let parts: Vec<_> = lower.split('/').collect();
    if parts.iter().any(|p| {
        [
            "$recycle.bin",
            "system volume information",
            ".git",
            "node_modules",
        ]
        .contains(p)
    }) {
        return Some("System and development files");
    }
    if [
        "launcher/data/backup",
        "launcher/data/updates",
        "launcher/data/imports",
        "launcher/data/webview2",
        "launcher/data/skill-staging",
        "launcher/data/skill-backups",
        "retroarch/downloads",
    ]
    .iter()
    .any(|p| lower == *p || lower.starts_with(&format!("{p}/")))
    {
        return Some("Temporary and backup working files");
    }
    if ["retroarch/cache", "retroarch/logs"].contains(&lower.as_str())
        || (parts.len() == 4
            && parts[0] == "retrobat"
            && parts[1] == "emulators"
            && ["cache", "caches", "shadercache", "shader_cache", "logs"].contains(&parts[3]))
    {
        return Some("Rebuildable caches and logs");
    }
    if parts
        .windows(3)
        .any(|w| w[0] == "vita3k" && w[1] == "ux0" && ["app", "patch", "addcont"].contains(&w[2]))
    {
        return Some("Installed Vita games, patches and DLC");
    }
    if parts
        .windows(3)
        .any(|w| w[0] == "rpcs3" && w[1] == "dev_hdd0" && ["game", "disc", "tmp"].contains(&w[2]))
    {
        return Some("Installed PS3 game content");
    }
    if parts.iter().any(|p| ["azahar", "citra"].contains(p))
        && parts.windows(4).any(|w| {
            w[0] == "title"
                && ["00040000", "0004000e", "0004008c"].contains(&w[1])
                && w[3] == "content"
        })
    {
        return Some("Installed 3DS content (save data retained)");
    }
    if parts
        .iter()
        .any(|p| ["switch", "yuzu", "suyu", "sudachi", "eden", "ryujinx"].contains(p))
        && parts
            .windows(2)
            .any(|w| ["user", "nintendo"].contains(&w[0]) && w[1] == "contents")
    {
        return Some("Installed Switch content (saves retained)");
    }
    if parts.contains(&"xenia")
        && parts.contains(&"content")
        && parts
            .iter()
            .any(|p| ["00007000", "00004000", "00000002", "000b0000"].contains(p))
    {
        return Some("Installed Xbox 360 game content");
    }
    None
}
fn rom_metadata(path: &str, bytes: u64) -> bool {
    let lower = path.to_ascii_lowercase();
    let parts: Vec<_> = lower.split('/').collect();
    let Some(index) = parts.iter().position(|p| *p == "roms") else {
        return false;
    };
    // Metadata lives directly under a platform or its known frontend asset folders.
    // Images buried inside unpacked games are game content, even with a media extension.
    let top_level = parts.len() == index + 3;
    let asset_folder = parts.get(index + 2).is_some_and(|p| {
        [
            "images",
            "videos",
            "media",
            "manuals",
            "screenshots",
            "downloaded_images",
            "downloaded_videos",
        ]
        .contains(p)
    });
    if !top_level && !asset_folder {
        return false;
    }
    let name = lower.rsplit('/').next().unwrap_or("");
    let extension = name.rsplit('.').next().unwrap_or("");
    if top_level && extension == "psvita" {
        return bytes == 0;
    }
    if top_level && extension == "m3u" {
        return bytes <= 128 * 1024;
    }
    [
        "xml", "json", "cfg", "ini", "txt", "png", "jpg", "jpeg", "webp", "svg", "mp4", "pdf",
    ]
    .contains(&extension)
}
fn is_game_file(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    let ext = lower.rsplit('.').next().unwrap_or("");
    [
        "iso", "nsp", "xci", "vpk", "xiso", "xex", "nes", "sfc", "smc", "gba", "gb", "gbc", "nds",
        "3ds", "cia", "chd", "cso", "pbp", "rvz", "wbfs", "n64", "z64", "v64", "gcm", "gen", "32x",
        "sms", "gg", "a26", "a52", "a78", "pce", "ngp", "ngc", "wsc", "rom", "pkg",
    ]
    .contains(&ext)
}

pub fn scan(root: &Path, cancel: &AtomicBool) -> Result<Plan, String> {
    for marker in ["START.exe", "launcher.json"] {
        if !safe_path(root, marker)?.is_file() {
            return Err(format!("Missing RetroDrive marker: {marker}"));
        }
    }
    let mut plan = Plan {
        token: String::new(),
        files: Vec::new(),
        directories: Vec::new(),
        links: Vec::new(),
        excluded: BTreeMap::new(),
        warnings: Vec::new(),
        bytes: 0,
    };
    let mut stack = vec![(root.to_path_buf(), String::new())];
    while let Some((directory, prefix)) = stack.pop() {
        if cancel.load(Ordering::Relaxed) {
            return Err("Cancelled".into());
        }
        for entry in fs::read_dir(&directory)
            .map_err(|e| format!("Cannot scan {}: {e}", directory.display()))?
        {
            if cancel.load(Ordering::Relaxed) {
                return Err("Cancelled".into());
            }
            let entry = entry.map_err(|e| e.to_string())?;
            let name = entry
                .file_name()
                .to_str()
                .ok_or("Non-UTF-8 filename")?
                .to_owned();
            let relative = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            // Do not even enumerate excluded directory contents.
            if let Some(reason) = excluded_directory(&relative) {
                *plan.excluded.entry(reason.into()).or_default() += 1;
                continue;
            }
            let metadata = fs::symlink_metadata(entry.path()).map_err(|e| e.to_string())?;
            if is_link(&metadata) {
                if metadata.file_attributes() & 0x10 == 0 {
                    plan.warnings
                        .push(format!("File link excluded: {relative}"));
                    continue;
                }
                if let Ok(target) = fs::read_link(entry.path()) {
                    let absolute = if target.is_absolute() {
                        target
                    } else {
                        directory.join(target)
                    };
                    if let Ok(canonical) = absolute.canonicalize() {
                        if let Ok(target) = canonical.strip_prefix(root) {
                            plan.links.push(DriveLink {
                                path: relative,
                                target: target.to_string_lossy().replace('\\', "/"),
                            });
                            continue;
                        }
                    }
                }
                plan.warnings.push(format!("Link not followed: {relative}"));
                continue;
            }
            let in_roms = relative.split('/').any(|p| p.eq_ignore_ascii_case("roms"));
            if metadata.is_dir() {
                if prefix.is_empty()
                    && ![
                        "Launcher",
                        "RetroBat",
                        "RetroArch",
                        "saves",
                        ".agents",
                        "roms",
                    ]
                    .iter()
                    .any(|p| p.eq_ignore_ascii_case(&name))
                {
                    plan.warnings.push(format!(
                        "Unrecognized top-level directory excluded: {relative}"
                    ));
                    continue;
                }
                if !in_roms {
                    plan.directories.push(relative.clone());
                }
                stack.push((entry.path(), relative));
            } else if metadata.is_file() {
                if (in_roms && !rom_metadata(&relative, metadata.len()))
                    || (!in_roms && is_game_file(&relative))
                {
                    *plan
                        .excluded
                        .entry("ROMs and game images".into())
                        .or_default() += 1;
                    continue;
                }
                let modified = metadata
                    .modified()
                    .map_err(|e| e.to_string())?
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_nanos();
                plan.bytes += metadata.len();
                plan.files.push(BackupFile {
                    path: relative,
                    bytes: metadata.len(),
                    modified,
                });
            }
        }
    }
    plan.files.sort_by(|a, b| a.path.cmp(&b.path));
    plan.directories.sort();
    plan.links.sort_by(|a, b| a.path.cmp(&b.path));
    plan.token = hash(
        &serde_json::to_vec(&(&plan.files, &plan.directories, &plan.links))
            .map_err(|e| e.to_string())?,
    );
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn distinguishes_games_from_saves_and_bios() {
        assert!(excluded_directory("RetroBat/saves/psvita/vita3k/ux0/app").is_some());
        assert!(excluded_directory("RetroBat/saves/psvita/vita3k/ux0/user/00/savedata").is_none());
        assert!(excluded_directory(
            "RetroBat/saves/3ds/azahar/sdmc/Nintendo 3DS/id/id/title/00040000/id/content"
        )
        .is_some());
        assert!(excluded_directory(
            "RetroBat/saves/3ds/azahar/sdmc/Nintendo 3DS/id/id/title/00040000/id/data"
        )
        .is_none());
        assert!(excluded_directory("RetroBat/saves/switch/ryujinx/bis/user/Contents").is_some());
        assert!(excluded_directory("RetroBat/saves/switch/ryujinx/bis/user/save").is_none());
        assert!(!is_game_file("RetroBat/bios/firmware.bin"));
        assert!(rom_metadata("roms/nes/images/game.png", 100));
        assert!(!rom_metadata("roms/nes/game.zip", 100));
        assert!(!rom_metadata(
            "roms/ps3/Game/PS3_GAME/USRDIR/textures/image.png",
            100
        ));
        assert!(rom_metadata("roms/psvita/Title [PCSE00120].psvita", 0));
        assert!(rom_metadata("roms/3ds/Title.m3u", 100));
        for path in [
            "RetroBat/saves/psvita/vita3k/ux0/user/00/savedata/app",
            "RetroBat/saves/ps3/rpcs3/dev_hdd0/home/user/savedata/game",
            "RetroBat/saves/3ds/azahar/sdmc/title/00040000/id/data/content",
            "RetroBat/saves/3ds/azahar/nand/title/00040138/id/content",
            "RetroArch/saves/game/cache",
        ] {
            assert!(
                excluded_directory(path).is_none(),
                "Excluded user or firmware data: {path}"
            );
        }
    }
}
