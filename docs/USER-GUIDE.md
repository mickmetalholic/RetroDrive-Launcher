# RetroDrive Quick Start

1. Connect the drive to a 64-bit Windows 10 22H2 or Windows 11 PC.
2. Run `START.exe` from the drive root.
   It refreshes installed Nintendo 3DS `.m3u` shortcuts to the drive's current absolute path before opening RetroDrive.
3. Use the D-pad or arrow keys to choose RetroBat, RetroArch, or **Settings**.
4. Press A or Enter to open the selected application, or open the Settings page.
5. Close the emulator frontend to return to RetroDrive.
6. Close RetroDrive, then use Windows **Safely Remove Hardware** before unplugging the drive.

## Controls

- D-pad / Arrow keys: move focus (up/down browses Skills)
- A / Enter: open or activate
- B / Backspace / Escape: exit or go back
- S: open Settings
- Settings keyboard ← / → or gamepad LB / RB: switch between Storage, Updates, and Skills
- Settings Storage R or gamepad A: refresh the read-only drive scan
- Settings Updates C or gamepad A: check for updates; I or gamepad X: install a verified update

## Runtime recovery

`START.exe` checks Windows version, free space, write access, WebView2, Visual C++ runtime files, and legacy DirectX files. Offline installers are stored in `Launcher\runtime`:

- `MicrosoftEdgeWebView2RuntimeInstallerX64.exe`
- `VC_redist.x64.exe`
- `VC_redist.x86.exe`
- `directx_Jun2010_redist.exe`

WebView2 recovery can be started directly by `START.exe`. Run the other installers manually only when the preflight message identifies a missing runtime file.

Installed Azahar games remain under `RetroBat\saves\3ds\azahar`. Keep their portable `.m3u` shortcuts under `RetroBat\roms\3ds`; `START.exe` rebases matching `.app` paths whenever the drive letter changes. The latest rewrite result is recorded in `Launcher\data\start.log`.

## Data locations

- Shared games: `roms`
- RetroBat saves and states: `RetroBat\saves`
- Independent RetroArch saves: `RetroArch\saves`
- Independent RetroArch states: `RetroArch\states`
- Launcher data and logs: `Launcher\data`
- Drive-local Codex skills: `.agents\skills`

## Skills

The launcher automatically installs bundled skills into `.agents\skills` at the drive root on first launch and after Launcher updates. For example, the game-import skill is stored at `.agents\skills\retrodrive-import-game\SKILL.md`. Open the drive root as your Codex working folder to use these project-scoped skills.

The Settings → Skills page is a read-only view of the drive installation and its status. No skills or configuration are written to the host PC's Codex directories. Existing modified drive skills are preserved instead of overwritten; installation failures are recorded in `Launcher\data\launcher.log`. Previous managed versions are backed up under `Launcher\data\skill-backups`.

## Drive overview

The Settings → Storage page reads the current portable drive rather than assuming a fixed drive letter. It reports free/used capacity, emulator directories under `RetroBat\emulators`, RetroArch core files, game files by platform, and the size of the main save/state directories. The scan is read-only and can be refreshed at any time.

RetroBat and independent RetroArch data are intentionally isolated. Do not merge their save or state directories.

## Verification files

- `Launcher\release\BINARY-HASHES.json`: launcher, skill, manifest, icon, and runtime hashes
- `Launcher\release\LIBRARY.csv`: game-library path, size, and SHA-256 inventory
