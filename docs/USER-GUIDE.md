# RetroDrive Quick Start

1. Connect the drive to a 64-bit Windows 10 22H2 or Windows 11 PC.
2. Run `START.exe` from the drive root.
3. Use the D-pad or arrow keys to choose RetroBat or RetroArch.
4. Press A or Enter to open the selected application. Press Start to open RetroBat immediately.
5. Close the emulator frontend to return to RetroDrive.
6. Close RetroDrive, then use Windows **Safely Remove Hardware** before unplugging the drive.

## Controls

- D-pad / Arrow keys: move focus
- A / Enter: open
- Start: open RetroBat
- B / Backspace / Escape: exit or go back

## Runtime recovery

`START.exe` checks Windows version, free space, write access, WebView2, Visual C++ runtime files, and legacy DirectX files. Offline installers are stored in `Launcher\runtime`:

- `MicrosoftEdgeWebView2RuntimeInstallerX64.exe`
- `VC_redist.x64.exe`
- `VC_redist.x86.exe`
- `directx_Jun2010_redist.exe`

WebView2 recovery can be started directly by `START.exe`. Run the other installers manually only when the preflight message identifies a missing runtime file.

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
