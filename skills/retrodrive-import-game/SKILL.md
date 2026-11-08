---
name: retrodrive-import-game
description: Import user-supplied games into RetroDrive with normalized names and content checks. Use for adding ROMs, installing unpacked PSVita games, or verifying an import; follow platform instructions for packages, disc conversions, and linked disc sets.
---

# RetroDrive Game Import

Prepare games on the portable drive, preserve their sources, and report separately what was copied, installed, and tested in an emulator. The helper requires Windows and PowerShell 7 (`pwsh`).

## Workflow

1. **Locate the drive automatically.** Prefer an explicit root, then this installed skill's location, then the working directory. Silently check for `START.exe` and `launcher.json`. Search mounted volumes only as a fallback; ask for a target only when none is found or the result is ambiguous.
2. **Inspect and name.** Read the source without modifying it. Identify the platform, content type, and title ID where available. Resolve the import name using [common.md](references/common.md); clarify uncertain identities instead of guessing.
3. **Choose a supported route.** Use the table below and read the relevant platform reference before changing files. Identify ambiguous formats such as `.iso` before invoking the helper.
4. **Preview and apply.** Show the source, proposed name, target, backup locations, commands, and checks. Run the helper without `-Apply` first. Apply the reviewed plan when existing authorization covers it; otherwise ask for confirmation. For manual routes, prepare the equivalent plan before using emulator tools.
5. **Verify and report.** Check copied content or the emulator's completed installation before creating launch entries. Report final paths, backups, and verification evidence. Test a RetroBat launch when requested. On failure, preserve diagnostics and recover backups once the emulator has stopped; report any unresolved recovery.

## Import routes

| Source | Handling |
| --- | --- |
| One unpacked PSVita game with `app/<TITLE_ID>/sce_sys/param.sfo` | Scripted Vita3K installation; read [psvita.md](references/psvita.md). |
| Standalone Switch `.nsp` or `.xci` | Copy and hash-check under `roms/switch`; emulator installation and runtime compatibility are separate checks. |
| PS3 disc image, package, or update | Read [ps3.md](references/ps3.md). The helper can copy an emulator-readable standalone image with `-Platform ps3`; package installation is manual. |
| Xbox or Xbox 360 image | Follow [xbox.md](references/xbox.md); conversion and emulator setup are manual. |
| Multi-disc set or linked image (`.m3u`, `.cue`, `.gdi`, etc.) | Follow [multidisc.md](references/multidisc.md); the single-file helper rejects these inputs. |
| Other standalone ROM | The helper's `generic` route copies to `roms/inbox` only. Arrange the final platform directory separately. |

Vita packages, patch-only trees, and installed Switch content need emulator-specific handling. The helper does not install firmware or keys, convert images, or generate multi-disc playlists. Operate on user-supplied game assets and keep saves in emulator-owned storage.

## Script usage

From the portable drive root, preview a Vita import:

```powershell
pwsh -NoProfile -File .\.agents\skills\retrodrive-import-game\scripts\Import-RetroDriveGame.ps1 `
  -SourcePath "C:\Games\Persona 4 Golden" `
  -DisplayName "Persona 4 Golden" `
  -Platform psvita
```

Repeat the reviewed command with `-Apply` to execute it. `-DisplayName` is the filename stem without its extension; retain known edition tags. Use `-RetroDriveRoot "D:\"` to explicitly select a different drive. When invoking the repository copy, resolve the script relative to this `SKILL.md` instead of assuming it is installed at the drive root.
