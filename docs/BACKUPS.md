# Portable NAS backups

Open **Settings → Backup**. The destination is fixed at:

```text
\\KAELTHAS\personal_folder\Backup\RetroDrive\restic
```

1. Close RetroBat, RetroArch, and any emulators running from the portable installation.
2. Choose **Start backup**. The launcher scans and filters the source as part of the backup job. Only this action can initialize the NAS repository. **Cancel operation** stops the background job.
3. Use **Load history**, open a completed snapshot, and confirm **Restore copy**. **Open restored files** opens the verified copy for review.

The launcher uses Windows' existing SMB access. The repository uses an empty password, so anyone with read access to the NAS repository files can read the backups. NAS failures appear on the Backup page. Backup jobs block application launching, updates and closing the launcher until the job finishes or is cancelled. A process-image check also rejects backup operations while programs below the portable `RetroBat` or `RetroArch` directory are running.

## Included and excluded content

The versioned scanner in `src-tauri/src/backup_policy.rs` includes the portable launcher, `.agents`, RetroBat, RetroArch, emulator binaries, BIOS/firmware, licenses, configuration, save files, save states, and ROM-library metadata/artwork. Source contents are never modified by scanning or backup.

It excludes:

- Game files in any `roms` subtree, retaining platform-level metadata/configuration, small `.m3u` playlists, empty `.psvita` launch entries, and known frontend artwork/video/manual directories. Media inside unpacked game directories is excluded. Unknown ROM formats are excluded rather than guessed from extensions.
- Known standalone game-image extensions outside ROM directories, including packages and cartridge/disc images.
- Vita3K `ux0/app`, `patch` and `addcont`; RPCS3 `dev_hdd0/game`, `disc` and `tmp`.
- Azahar/Citra game/update/DLC `title/<category>/<title>/content` directories for categories `00040000`, `0004000e`, and `0004008c`. The neighboring `data` directories and system/firmware title content are retained.
- Switch emulator `user/Contents` and `Nintendo/Contents`, retaining save storage. Known Xenia game/DLC containers are excluded while save containers are retained.
- Windows recycle/system directories, development trees, launcher update/import/WebView working files, the backup workspace itself, and known program cache/log directories.

Directory junctions are recorded without traversing their targets. Only internal directory targets can be reconstructed inside an isolated restore. External, broken and file links and unrecognized top-level directories are excluded. This policy covers the documented portable layout. Adding another emulator or a nonstandard installed-game layout requires a policy update.

## Snapshots and integrity

restic 0.19.0 provides compression, content deduplication and integrity checks. A stable drive identifier groups backups across PCs. The launcher retains all snapshots.

Each attempt has a unique run tag. Only a successful restic exit followed by a completed-snapshot tag updates **Last successful backup**. Read errors produce an incomplete snapshot, never a successful result. Incomplete or interrupted attempts are not offered as restorable completed snapshots. Cancelling retains prior snapshots and removes only stale restic locks, never an active writer's lock.

Restore uses `--verify`, validates the embedded layout manifest, reconstructs empty directories and internal junctions, and leaves the existing drive installation untouched. Restored ROM links point to an empty/metadata-only ROM tree: game files must be supplied separately.

## Implementation and packaging

`backup.rs` owns the cancellable job, progress, history and verified restore. `backup_policy.rs` owns inclusion rules and path validation. The launcher uses fixed repository and restore locations.

Local tools, caches, the engine's file list and restored copies are under `Launcher/data/backup`. The previous successful backup record is stored there as `last-success.json`. This workspace is excluded from the payload, apart from the deliberately added per-snapshot layout manifest. The engine uses a NUL-delimited verbatim file list, preserving spaces, Unicode, brackets and leading `#` characters.

`pnpm prepare:backup` downloads the official Windows x64 ZIP, checks its pinned SHA-256, extracts only the executable, and verifies its executable hash. `pnpm check` and the production build prepare it automatically. Rust embeds that executable and its [BSD license](../src-tauri/resources/restic-LICENSE.txt) into the launcher. On demand it extracts the verified engine on the portable drive. Existing updaters can deliver this feature through the unchanged launcher executable allowlist; no global installation or runtime engine download is required.

Tests use a separate local repository with the real restic engine. They verify exclusions, metadata and save preservation, multiple snapshots, historical restoration, junction reconstruction, source validation and cancellation.
