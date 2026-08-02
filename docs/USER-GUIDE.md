# RetroDrive Quick Start

1. Connect the drive to a 64-bit Windows 10 22H2 or Windows 11 PC.
2. Run `START.exe` from the drive root.
   It refreshes installed Nintendo 3DS `.m3u` shortcuts to the drive's current absolute path before opening RetroDrive.
3. Use the D-pad or arrow keys to choose RetroBat or RetroArch.
4. Press A or Enter to open the selected application.
5. Close the emulator frontend to return to RetroDrive.
6. Close RetroDrive, then use Windows **Safely Remove Hardware** before unplugging the drive.

## Controls

- D-pad / Arrow keys: move focus
- A / Enter: open
- B / Backspace / Escape: exit or go back

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

RetroBat and independent RetroArch data are intentionally isolated. Do not merge their save or state directories.

## Verification files

- `Launcher\release\BINARY-HASHES.json`: launcher, manifest, icon, and runtime hashes
- `Launcher\release\LIBRARY.csv`: game-library path, size, and SHA-256 inventory
