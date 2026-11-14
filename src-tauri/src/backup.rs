use crate::backup_policy::{self as policy, Plan};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Arc, Mutex,
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

pub const REPOSITORY: &str = r"\\KAELTHAS\personal_folder\Backup\RetroDrive\restic";
const WORK: &str = "Launcher/data/backup";
const ENGINE_HASH: &str = "40576f77c1d40245a9f4af92a0b37b0d2514e6be0dffbf16ca8855820c13693e";
const ENGINE: &[u8] = include_bytes!("../tools/restic.exe");
const LICENSE: &str = include_str!("../resources/restic-LICENSE.txt");

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub id: String,
    pub time: String,
    pub complete: bool,
    pub files: u64,
    pub bytes: u64,
    pub added_bytes: u64,
    pub tags: Vec<String>,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub repository: String,
    pub busy: bool,
    pub phase: String,
    pub detail: String,
    pub snapshots: Vec<Snapshot>,
    pub files_done: u64,
    pub bytes_done: u64,
    pub total_files: u64,
    pub total_bytes: u64,
    pub snapshot_id: Option<String>,
    pub restore_path: Option<String>,
    pub last_success: Option<Value>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Layout {
    version: u32,
    drive_id: String,
    plan: Plan,
}

pub struct Manager {
    root: PathBuf,
    repository: PathBuf,
    drive_id: String,
    state: Mutex<Status>,
    busy: AtomicBool,
    cancel: AtomicBool,
    engine_started: AtomicBool,
}
#[derive(Clone)]
enum Job {
    Backup,
    History,
    Restore(String),
}

fn now() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
}
fn read_json(path: &Path) -> Option<Value> {
    fs::read(path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
}

impl Manager {
    pub fn new(root: &Path) -> Result<Arc<Self>, String> {
        Self::with_repository(root, Path::new(REPOSITORY))
    }
    fn with_repository(root: &Path, repository: &Path) -> Result<Arc<Self>, String> {
        let root = root.canonicalize().map_err(|e| e.to_string())?;
        let work = policy::safe_path(&root, WORK)?;
        fs::create_dir_all(&work).map_err(|e| e.to_string())?;
        let identity_path = policy::safe_path(&root, &format!("{WORK}/drive-id.txt"))?;
        let drive_id = if identity_path.is_file() {
            fs::read_to_string(&identity_path).map_err(|e| e.to_string())?
        } else {
            let id = policy::hash(
                format!("{}-{}-{}", root.display(), now(), std::process::id()).as_bytes(),
            )[..32]
                .to_owned();
            fs::write(&identity_path, &id).map_err(|e| e.to_string())?;
            id
        };
        if drive_id.len() != 32 || !drive_id.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("Invalid backup drive identity".into());
        }
        let last_success = read_json(&policy::safe_path(
            &root,
            &format!("{WORK}/last-success.json"),
        )?);
        let previous = read_json(&policy::safe_path(
            &root,
            &format!("{WORK}/last-operation.json"),
        )?);
        Ok(Arc::new(Self {
            state: Mutex::new(Status {
                repository: repository.to_string_lossy().into_owned(),
                busy: false,
                phase: previous
                    .as_ref()
                    .and_then(|p| p["phase"].as_str())
                    .filter(|p| ["failed", "partial", "cancelled"].contains(p))
                    .unwrap_or("idle")
                    .into(),
                detail: previous
                    .as_ref()
                    .and_then(|p| p["detail"].as_str())
                    .map(|d| format!("Previous operation · {d}"))
                    .unwrap_or_default(),
                snapshots: vec![],
                files_done: 0,
                bytes_done: 0,
                total_files: 0,
                total_bytes: 0,
                snapshot_id: None,
                restore_path: None,
                last_success,
            }),
            root,
            repository: repository.to_path_buf(),
            drive_id,
            busy: AtomicBool::new(false),
            cancel: AtomicBool::new(false),
            engine_started: AtomicBool::new(false),
        }))
    }
    pub fn status(&self) -> Status {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner()).clone();
        state.busy = self.is_busy();
        state
    }
    pub fn is_busy(&self) -> bool {
        self.busy.load(Ordering::Acquire)
    }
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Release);
    }
    pub fn backup(self: &Arc<Self>) -> Result<(), String> {
        self.start(Job::Backup)
    }
    pub fn history(self: &Arc<Self>) -> Result<(), String> {
        self.start(Job::History)
    }
    pub fn restore(self: &Arc<Self>, id: String) -> Result<(), String> {
        if id.len() != 64 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("Invalid snapshot ID".into());
        }
        self.start(Job::Restore(id))
    }
    pub fn open_restored_files(&self) -> Result<(), String> {
        let state = self.status();
        let path = PathBuf::from(state.restore_path.ok_or("No restored files available")?);
        let relative = path
            .strip_prefix(&self.root)
            .map_err(|_| "Invalid restore folder")?
            .to_string_lossy()
            .replace('\\', "/");
        let path = policy::safe_path(&self.root, &relative)?;
        if !path.is_dir() {
            return Err("The restore folder is unavailable".into());
        }
        Command::new("explorer.exe")
            .arg(path)
            .spawn()
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    fn update(&self, f: impl FnOnce(&mut Status)) {
        f(&mut self.state.lock().unwrap_or_else(|p| p.into_inner()));
    }
    fn start(self: &Arc<Self>, job: Job) -> Result<(), String> {
        if self
            .busy
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err("A backup operation is already running".into());
        }
        self.cancel.store(false, Ordering::Release);
        self.engine_started.store(false, Ordering::Release);
        self.update(|s| {
            s.phase = match job {
                Job::Backup => "preparing",
                Job::History => "loadingHistory",
                Job::Restore(_) => "restoring",
            }
            .into();
            s.detail.clear();
            s.files_done = 0;
            s.bytes_done = 0;
            s.total_files = 0;
            s.total_bytes = 0;
            s.snapshot_id = None;
        });
        let manager = Arc::clone(self);
        thread::spawn(move || {
            let result = manager.run(job);
            if manager.cancel.load(Ordering::Acquire)
                && manager.engine_started.load(Ordering::Acquire)
            {
                // Remove only stale locks after our child exits; never force-unlock another writer.
                manager.cancel.store(false, Ordering::Release);
                let _ = manager.command(&["unlock"], false);
                manager.cancel.store(true, Ordering::Release);
            }
            if let Err(error) = result {
                manager.update(|s| {
                    s.phase = if manager.cancel.load(Ordering::Acquire) {
                        "cancelled"
                    } else {
                        "failed"
                    }
                    .into();
                    s.detail = error;
                });
            }
            let state = manager.status();
            if let Ok(path) =
                policy::safe_path(&manager.root, &format!("{WORK}/last-operation.json"))
            {
                let record = json!({"phase":state.phase,"detail":state.detail,"snapshotId":state.snapshot_id,"finishedAtUnix":now()/1_000_000_000});
                if let Ok(bytes) = serde_json::to_vec_pretty(&record) {
                    let _ = fs::write(path, bytes);
                }
            }
            manager.busy.store(false, Ordering::Release);
        });
        Ok(())
    }
    fn run(self: &Arc<Self>, job: Job) -> Result<(), String> {
        match job {
            Job::Backup => self.run_backup()?,
            Job::History => {
                self.load_history()?;
                self.update(|s| {
                    s.phase = "completed".into();
                    s.detail = "History updated.".into();
                });
            }
            Job::Restore(id) => self.run_restore(&id)?,
        }
        Ok(())
    }
    fn engine(&self) -> Result<PathBuf, String> {
        let directory = policy::safe_path(&self.root, &format!("{WORK}/tools"))?;
        fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
        let path = policy::safe_path(&self.root, &format!("{WORK}/tools/restic.exe"))?;
        if !path.is_file()
            || policy::hash(&fs::read(&path).map_err(|e| e.to_string())?) != ENGINE_HASH
        {
            if policy::hash(ENGINE) != ENGINE_HASH {
                return Err("Embedded backup engine checksum mismatch".into());
            }
            fs::write(&path, ENGINE).map_err(|e| e.to_string())?;
        }
        fs::write(
            policy::safe_path(&self.root, &format!("{WORK}/tools/LICENSE-restic.txt"))?,
            LICENSE,
        )
        .map_err(|e| e.to_string())?;
        Ok(path)
    }
    fn command(
        self: &Arc<Self>,
        args: &[&str],
        allow_partial: bool,
    ) -> Result<(i32, String), String> {
        if self.cancel.load(Ordering::Acquire) {
            return Err("Cancelled".into());
        }
        let cache = policy::safe_path(&self.root, &format!("{WORK}/cache"))?;
        fs::create_dir_all(&cache).map_err(|e| e.to_string())?;
        let temp = policy::safe_path(&self.root, &format!("{WORK}/temp"))?;
        fs::create_dir_all(&temp).map_err(|e| e.to_string())?;
        let mut command = Command::new(self.engine()?);
        for (name, _) in std::env::vars_os() {
            if name
                .to_string_lossy()
                .to_ascii_uppercase()
                .starts_with("RESTIC_")
            {
                command.env_remove(name);
            }
        }
        command
            .current_dir(&self.root)
            .args(["--json", "--insecure-no-password", "--repo"])
            .arg(&self.repository)
            .arg("--cache-dir")
            .arg(cache)
            .args(["--stuck-request-timeout", "20s"])
            .args(args)
            .env("TMP", &temp)
            .env("TEMP", temp)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .creation_flags(0x0800_0000);
        let mut child = command
            .spawn()
            .map_err(|e| format!("Cannot start backup engine: {e}"))?;
        self.engine_started.store(true, Ordering::Release);
        let (tx, rx) = mpsc::channel();
        let stdout = child.stdout.take().ok_or("Missing backup output")?;
        let stderr = child.stderr.take().ok_or("Missing backup error output")?;
        let tx2 = tx.clone();
        let out = thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                if tx.send((false, line.unwrap_or_default())).is_err() {
                    break;
                }
            }
        });
        let err = thread::spawn(move || {
            for line in BufReader::new(stderr).lines() {
                if tx2.send((true, line.unwrap_or_default())).is_err() {
                    break;
                }
            }
        });
        let mut output = String::new();
        let mut errors = String::new();
        let mut consume = |is_error: bool, line: String| {
            if !is_error {
                if let Ok(value) = serde_json::from_str::<Value>(&line) {
                    if value["message_type"] == "status" {
                        self.update(|s| {
                            s.files_done = value["files_done"].as_u64().unwrap_or(s.files_done);
                            s.bytes_done = value["bytes_done"].as_u64().unwrap_or(s.bytes_done);
                            s.total_files = value["total_files"].as_u64().unwrap_or(s.total_files);
                            s.total_bytes = value["total_bytes"].as_u64().unwrap_or(s.total_bytes);
                        });
                    }
                }
            }
            let target = if is_error { &mut errors } else { &mut output };
            if target.len() < 64 * 1024 * 1024 {
                target.push_str(&line);
                target.push('\n');
            }
        };
        let exit = loop {
            if self.cancel.load(Ordering::Acquire) {
                let _ = child.kill();
            }
            if let Ok((e, l)) = rx.recv_timeout(Duration::from_millis(100)) {
                consume(e, l);
            }
            if let Some(exit) = child.try_wait().map_err(|e| e.to_string())? {
                break exit;
            }
        };
        let _ = out.join();
        let _ = err.join();
        while let Ok((e, l)) = rx.try_recv() {
            consume(e, l);
        }
        if self.cancel.load(Ordering::Acquire) {
            return Err("Cancelled. Previously completed snapshots are unchanged.".into());
        }
        let code = exit.code().unwrap_or(-1);
        if code == 12 {
            return Err("This backup repository requires its original password.".into());
        }
        if code != 0 && !(allow_partial && code == 3) {
            return Err(format!(
                "Backup engine exited with code {code}: {}",
                errors.chars().take(3000).collect::<String>()
            ));
        }
        Ok((code, output))
    }
    fn load_history(self: &Arc<Self>) -> Result<Vec<Snapshot>, String> {
        self.check_storage()?;
        if !self.repository.join("config").is_file() {
            self.update(|s| s.snapshots.clear());
            return Ok(vec![]);
        }
        let (_, output) = self.command(&["snapshots", "--tag", "retrodrive"], false)?;
        let values: Vec<Value> =
            serde_json::from_str(&output).map_err(|e| format!("Invalid snapshot list: {e}"))?;
        let mut snapshots = values
            .into_iter()
            .map(|v| {
                let tags: Vec<String> =
                    serde_json::from_value(v["tags"].clone()).unwrap_or_default();
                Snapshot {
                    id: v["id"].as_str().unwrap_or("").into(),
                    time: v["time"].as_str().unwrap_or("").into(),
                    complete: tags.contains(&"retrodrive-complete".into()),
                    files: v["summary"]["total_files_processed"].as_u64().unwrap_or(0),
                    bytes: v["summary"]["total_bytes_processed"].as_u64().unwrap_or(0),
                    added_bytes: v["summary"]["data_added"].as_u64().unwrap_or(0),
                    tags,
                }
            })
            .collect::<Vec<_>>();
        snapshots.sort_by(|a, b| b.time.cmp(&a.time));
        self.update(|s| s.snapshots = snapshots.clone());
        Ok(snapshots)
    }
    fn run_backup(self: &Arc<Self>) -> Result<(), String> {
        self.check_storage()?;
        let plan = policy::scan(&self.root, &self.cancel)?;
        let layout = Layout {
            version: 1,
            drive_id: self.drive_id.clone(),
            plan: plan.clone(),
        };
        let manifest_relative = format!("{WORK}/layout.json");
        let manifest_path = policy::safe_path(&self.root, &manifest_relative)?;
        fs::write(
            manifest_path,
            serde_json::to_vec_pretty(&layout).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        let list = policy::safe_path(&self.root, &format!("{WORK}/files.bin"))?;
        let mut file_list = fs::File::create(&list).map_err(|e| e.to_string())?;
        for file in &plan.files {
            policy::safe_path(&self.root, &file.path)?;
            file_list
                .write_all(file.path.as_bytes())
                .and_then(|_| file_list.write_all(&[0]))
                .map_err(|e| e.to_string())?;
        }
        file_list
            .write_all(manifest_relative.as_bytes())
            .and_then(|_| file_list.write_all(&[0]))
            .map_err(|e| e.to_string())?;
        drop(file_list);
        // Only the explicit Start Backup action can initialize the fixed NAS repository.
        self.update(|s| {
            s.total_files = plan.files.len() as u64;
            s.total_bytes = plan.bytes;
            s.detail.clear();
        });
        if !self.repository.join("config").is_file() {
            self.update(|s| {
                s.phase = "initializing".into();
                s.detail = "Creating the backup repository.".into();
            });
            self.command(&["init", "--repository-version", "2"], false)?;
        }
        self.update(|s| {
            s.phase = "backingUp".into();
            s.detail.clear();
        });
        let run_tag = format!("run-{}-{}", self.drive_id, now());
        let host = format!("retrodrive-{}", self.drive_id);
        let (code, _) = self.command(
            &[
                "backup",
                "--files-from-raw",
                &list.to_string_lossy(),
                "--host",
                &host,
                "--tag",
                "retrodrive",
                "--tag",
                &run_tag,
                "--group-by",
                "host",
            ],
            true,
        )?;
        if code == 3 {
            self.load_history()?;
            self.update(|s| {
                s.phase = "partial".into();
                s.detail = "Some files could not be read. This snapshot is incomplete.".into();
            });
            return Ok(());
        }
        self.command(
            &["tag", "--tag", &run_tag, "--add", "retrodrive-complete"],
            false,
        )?;
        let snapshots = self.load_history()?;
        let snapshot = snapshots
            .iter()
            .find(|s| s.tags.contains(&run_tag) && s.complete)
            .ok_or("Backup finished without a completed snapshot")?;
        let last = json!({"snapshotId": snapshot.id, "time": snapshot.time, "files": plan.files.len(), "bytes": plan.bytes, "addedBytes": snapshot.added_bytes});
        fs::write(
            policy::safe_path(&self.root, &format!("{WORK}/last-success.json"))?,
            serde_json::to_vec_pretty(&last).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        self.update(|s| {
            s.phase = "completed".into();
            s.detail = "Snapshot saved.".into();
            s.snapshot_id = Some(snapshot.id.clone());
            s.last_success = Some(last);
            s.files_done = plan.files.len() as u64;
            s.bytes_done = plan.bytes;
            s.total_files = s.files_done;
            s.total_bytes = s.bytes_done;
        });
        Ok(())
    }
    fn check_storage(&self) -> Result<(), String> {
        let parent = self
            .repository
            .ancestors()
            .nth(2)
            .ok_or("Invalid backup repository path")?;
        fs::read_dir(parent)
            .map_err(|e| format!("Backup destination unavailable: {}: {e}", parent.display()))?;
        Ok(())
    }
    fn run_restore(self: &Arc<Self>, id: &str) -> Result<(), String> {
        let snapshots = self.load_history()?;
        if !snapshots.iter().any(|s| s.id == id && s.complete) {
            return Err("Select a completed RetroDrive snapshot".into());
        }
        let relative = format!("{WORK}/restores/{}-{}", &id[..8], now());
        let target = policy::safe_path(&self.root, &relative)?;
        fs::create_dir_all(&target).map_err(|e| e.to_string())?;
        self.command(
            &[
                "restore",
                id,
                "--target",
                &target.to_string_lossy(),
                "--verify",
            ],
            false,
        )?;
        let mut stack = vec![target.clone()];
        let mut layouts = Vec::new();
        while let Some(path) = stack.pop() {
            for entry in fs::read_dir(path).map_err(|e| e.to_string())? {
                let entry = entry.map_err(|e| e.to_string())?;
                let metadata = fs::symlink_metadata(entry.path()).map_err(|e| e.to_string())?;
                if policy::is_link(&metadata) {
                    return Err("Unexpected link in restored files".into());
                }
                if metadata.is_dir() {
                    stack.push(entry.path());
                } else if entry.file_name() == "layout.json"
                    && entry
                        .path()
                        .to_string_lossy()
                        .replace('\\', "/")
                        .ends_with("/Launcher/data/backup/layout.json")
                {
                    layouts.push(entry.path());
                }
            }
        }
        if layouts.len() != 1 {
            return Err("Restored snapshot has no unique RetroDrive layout manifest".into());
        }
        let layout: Layout =
            serde_json::from_slice(&fs::read(&layouts[0]).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        if layout.version != 1 {
            return Err("Unsupported backup layout version".into());
        }
        let payload = layouts[0]
            .ancestors()
            .nth(4)
            .ok_or("Invalid restored layout path")?
            .to_path_buf();
        for file in &layout.plan.files {
            if !policy::safe_path(&payload, &file.path)?.is_file() {
                return Err(format!("Restored file missing: {}", file.path));
            }
        }
        for directory in &layout.plan.directories {
            fs::create_dir_all(policy::safe_path(&payload, directory)?)
                .map_err(|e| e.to_string())?;
        }
        // Recreate only known directory junctions within the isolated restored tree.
        for link in &layout.plan.links {
            let path = policy::safe_path(&payload, &link.path)?;
            let destination = policy::safe_path(&payload, &link.target)?;
            fs::create_dir_all(&destination).map_err(|e| e.to_string())?;
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            let output = Command::new("powershell.exe")
                .args(["-NoProfile", "-NonInteractive", "-Command", "New-Item -ItemType Junction -Path $env:RETRODRIVE_BACKUP_LINK -Target $env:RETRODRIVE_BACKUP_TARGET -ErrorAction Stop | Out-Null"])
                .env_remove("PSModulePath")
                .env("RETRODRIVE_BACKUP_LINK", &path)
                .env("RETRODRIVE_BACKUP_TARGET", &destination)
                .creation_flags(0x0800_0000)
                .output()
                .map_err(|e| e.to_string())?;
            if !output.status.success() {
                return Err(format!("Could not recreate restored link: {}", link.path));
            }
        }
        self.update(|s| {
            s.phase = "completed".into();
            s.detail = "Restored files verified.".into();
            s.restore_path = Some(payload.to_string_lossy().into_owned());
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    struct Fixture {
        directory: PathBuf,
        root: PathBuf,
        manager: Arc<Manager>,
    }
    impl Fixture {
        fn new() -> Self {
            let directory = std::env::temp_dir().join(format!(
                "retrodrive-backup-test-{}-{}",
                std::process::id(),
                now()
            ));
            let root = directory.join("portable drive");
            fs::create_dir_all(&root).unwrap();
            for (path, bytes) in [
                ("START.exe", "launcher"),
                ("launcher.json", "{}"),
                ("RetroArch/saves/game.srm", "save version one"),
                ("RetroArch/states/game.state", "state"),
                ("RetroBat/bios/firmware.bin", "bios"),
                (
                    "RetroBat/saves/psvita/vita3k/ux0/app/PCSE00120/eboot.bin",
                    "game content",
                ),
                (
                    "RetroBat/saves/psvita/vita3k/ux0/user/00/savedata/PCSE00120/save.bin",
                    "vita save",
                ),
                (
                    "RetroBat/saves/psvita/vita3k/ux0/license/PCSE00120/game.rif",
                    "license",
                ),
                (
                    "RetroBat/saves/3ds/azahar/sdmc/title/00040000/id/content/game.app",
                    "3ds game",
                ),
                (
                    "RetroBat/saves/3ds/azahar/sdmc/title/00040000/id/data/save.bin",
                    "3ds save",
                ),
                (
                    "RetroBat/saves/ps3/rpcs3/dev_hdd0/game/ID/USRDIR/EBOOT.BIN",
                    "ps3 game",
                ),
                (
                    "RetroBat/saves/ps3/rpcs3/dev_hdd0/home/user/savedata/ID/save.dat",
                    "ps3 save",
                ),
                (
                    "RetroBat/saves/switch/ryujinx/bis/user/Contents/registered/game.nca",
                    "switch game",
                ),
                (
                    "RetroBat/saves/switch/ryujinx/bis/user/save/progress.dat",
                    "switch save",
                ),
                ("roms/nes/game.nes", "ROM"),
                ("roms/nes/#收藏 [1].zip", "ROM archive"),
                ("roms/nes/gamelist.xml", "<games/>"),
                ("roms/nes/images/cover.png", "artwork"),
                ("RetroArch/config/#配置 [1].cfg", "config"),
                ("Launcher/data/updates/download.zip", "temporary update"),
            ] {
                let path = root.join(path);
                fs::create_dir_all(path.parent().unwrap()).unwrap();
                fs::write(path, bytes).unwrap();
            }
            let link = root.join("RetroBat/roms");
            let output = Command::new("cmd.exe")
                .args(["/D", "/C", "mklink", "/J"])
                .arg(&link)
                .arg(root.join("roms"))
                .creation_flags(0x0800_0000)
                .output()
                .unwrap();
            assert!(output.status.success());
            let manager =
                Manager::with_repository(&root, &directory.join("nas/RetroDrive/restic")).unwrap();
            fs::create_dir_all(directory.join("nas")).unwrap();
            Self {
                directory,
                root,
                manager,
            }
        }
        fn wait(&self) -> Status {
            let deadline = Instant::now() + Duration::from_secs(90);
            while self.manager.is_busy() {
                assert!(
                    Instant::now() < deadline,
                    "Backup test timed out: {:?}",
                    self.manager.status()
                );
                thread::sleep(Duration::from_millis(50));
            }
            self.manager.status()
        }
        fn backup(&self) -> Snapshot {
            self.manager.backup().unwrap();
            let state = self.wait();
            assert_eq!(state.phase, "completed", "{}", state.detail);
            state.snapshots.into_iter().find(|s| s.complete).unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            // All paths were generated directly beneath the OS temporary directory.
            assert_eq!(
                self.directory.parent(),
                Some(std::env::temp_dir().as_path())
            );
            assert!(self
                .directory
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("retrodrive-backup-test-"));
            // Remove junction entries themselves before removing the isolated test tree.
            let mut stack = vec![self.directory.clone()];
            while let Some(path) = stack.pop() {
                if let Ok(entries) = fs::read_dir(path) {
                    for entry in entries.flatten() {
                        if let Ok(meta) = fs::symlink_metadata(entry.path()) {
                            if policy::is_link(&meta) {
                                let _ = fs::remove_dir(entry.path());
                            } else if meta.is_dir() {
                                stack.push(entry.path());
                            }
                        }
                    }
                }
            }
            let _ = fs::remove_dir_all(&self.directory);
        }
    }

    #[test]
    fn scan_excludes_all_known_game_locations_but_keeps_saves_metadata_and_bios() {
        let f = Fixture::new();
        let plan = policy::scan(&f.manager.root, &AtomicBool::new(false)).unwrap();
        let paths: Vec<_> = plan.files.iter().map(|p| p.path.as_str()).collect();
        for wanted in [
            "RetroBat/bios/firmware.bin",
            "RetroBat/saves/3ds/azahar/sdmc/title/00040000/id/data/save.bin",
            "roms/nes/images/cover.png",
            "RetroArch/config/#配置 [1].cfg",
        ] {
            assert!(paths.contains(&wanted), "Missing {wanted}");
        }
        for forbidden in [
            "roms/nes/game.nes",
            "roms/nes/#收藏 [1].zip",
            "RetroBat/saves/psvita/vita3k/ux0/app/PCSE00120/eboot.bin",
            "RetroBat/saves/3ds/azahar/sdmc/title/00040000/id/content/game.app",
            "RetroBat/saves/ps3/rpcs3/dev_hdd0/game/ID/USRDIR/EBOOT.BIN",
            "RetroBat/saves/switch/ryujinx/bis/user/Contents/registered/game.nca",
            "Launcher/data/updates/download.zip",
        ] {
            assert!(!paths.contains(&forbidden), "Included {forbidden}");
        }
        assert_eq!(plan.links.len(), 1);
        assert!(
            !f.manager.repository.exists(),
            "Read-only scan created a NAS repository"
        );
    }

    #[test]
    fn real_engine_creates_incremental_history_and_restores_without_touching_current_files() {
        let f = Fixture::new();
        f.manager.history().unwrap();
        assert!(f.wait().snapshots.is_empty());
        assert!(!f.manager.repository.exists());
        let first = f.backup();
        assert!(first.files > 0);
        assert!(first.bytes > 0);
        fs::write(f.root.join("RetroArch/saves/game.srm"), "save version two").unwrap();
        let second = f.backup();
        assert_ne!(first.id, second.id);
        assert_eq!(
            f.manager
                .status()
                .snapshots
                .iter()
                .filter(|s| s.complete)
                .count(),
            2
        );
        f.manager.restore(first.id).unwrap();
        let status = f.wait();
        assert_eq!(status.phase, "completed", "{}", status.detail);
        let restored = PathBuf::from(status.restore_path.unwrap());
        assert_eq!(
            fs::read_to_string(restored.join("RetroArch/saves/game.srm")).unwrap(),
            "save version one"
        );
        assert_eq!(
            fs::read_to_string(f.root.join("RetroArch/saves/game.srm")).unwrap(),
            "save version two"
        );
        assert!(restored.join("roms/nes/gamelist.xml").is_file());
        assert!(restored.join("RetroArch/config/#配置 [1].cfg").is_file());
        assert!(!restored.join("roms/nes/game.nes").exists());
        assert!(!restored
            .join("RetroBat/saves/psvita/vita3k/ux0/app")
            .exists());
        assert!(policy::is_link(
            &fs::symlink_metadata(restored.join("RetroBat/roms")).unwrap()
        ));
        let last = f.manager.status().last_success;
        f.manager.backup().unwrap();
        f.manager.cancel();
        assert_eq!(f.wait().phase, "cancelled");
        assert_eq!(f.manager.status().last_success, last);
    }

    #[test]
    fn backup_without_a_valid_source_does_not_initialize_the_repository() {
        let f = Fixture::new();
        fs::remove_file(f.root.join("launcher.json")).unwrap();
        f.manager.backup().unwrap();
        let state = f.wait();
        assert_eq!(state.phase, "failed");
        assert!(state.detail.contains("Missing RetroDrive marker: launcher.json"));
        assert!(!f.manager.repository.join("config").exists());
    }

    #[test]
    fn password_protected_repository_is_not_reinitialized() {
        let f = Fixture::new();
        let init = Command::new(f.manager.engine().unwrap())
            .arg("--repo")
            .arg(&f.manager.repository)
            .arg("init")
            .env_clear()
            .env("RESTIC_PASSWORD", "fixture-password")
            .stdin(Stdio::null())
            .creation_flags(0x0800_0000)
            .output()
            .unwrap();
        assert!(init.status.success(), "{}", String::from_utf8_lossy(&init.stderr));
        let config = f.manager.repository.join("config");
        let existing_config = fs::read(&config).unwrap();
        f.manager.history().unwrap();
        let state = f.wait();
        assert_eq!(state.phase, "failed");
        assert!(state.detail.contains("requires its original password"), "{}", state.detail);
        f.manager.backup().unwrap();
        assert_eq!(f.wait().phase, "failed");
        assert_eq!(fs::read(config).unwrap(), existing_config);
    }
}
