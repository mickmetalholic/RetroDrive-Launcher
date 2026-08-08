---
name: retrodrive-import-game
description: Safely import a game file or content directory into a RetroDrive portable installation, with read-only detection, platform-specific installation, rollback-friendly staging, integrity checks, and RetroBat launch metadata. Use when the user asks to add, prepare, normalize, decrypt, install, or verify a game for RetroDrive.
---

# RetroDrive Game Import

Use this skill when a user provides a game file or game-content directory and wants it prepared for the RetroDrive hard drive. Treat every import as a controlled transaction: identify the platform first, show the planned target, preserve conflicting content, complete emulator-owned installation, validate the resulting files, and only then update the shared ROM library.

## Required workflow

1. Resolve the RetroDrive root. Prefer an explicit root supplied by the user. Otherwise discover a mounted volume containing both `START.exe` and `launcher.json`; stop if zero or multiple candidates exist.
2. Inspect the source read-only. Record source path, platform, title ID, content type, file count, byte count, and SHA-256 where practical. Do not rename, delete, overwrite, decrypt, or copy during inspection.
3. Select an adapter:
   - Use the Vita3K adapter for an unpacked PSVita content directory containing `app/<TITLE_ID>/sce_sys/param.sfo`. A `.vpk` or encrypted package is detected but must be unpacked or installed through the Vita3K GUI before this adapter can verify it.
   - Use the file-copy adapter for ordinary ROM files. Preserve the source extension and place the result under `roms/<system>`.
   - Treat PS3 PKG/updates, Xbox ISO/XISO, Switch installed content, and multi-disc sets as special cases; read the matching reference before applying changes. The bundled script refuses to copy PS3 package files, Xbox disc/executable images, or `.m3u` playlists through the ordinary ROM adapter.
4. Present the plan and require an explicit apply decision when the user has not already authorized the import. A plan must name every source, destination, backup path, and emulator command.
5. Run `scripts/Import-RetroDriveGame.ps1` first in plan mode. Use `-Apply` only after the plan is approved.
6. For an emulator-owned install, wait for the emulator's own completion signal and verify its final storage layout. A copied directory, a running emulator process, or an existing title entry is not proof of a successful import.
7. Validate the final invariant. For Vita3K, there must be no unfinished `<TITLE_ID>_dec` directory, the installed `eboot.bin` must begin with the Vita SELF header, and a valid 512-byte license must exist under `ux0/license/<TITLE_ID>`.
8. Generate or update launch metadata only after the content passes validation. The script creates zero-length PSVita `.psvita` stubs whose filenames contain the title IDs; do not put a trailing newline into a title-ID launcher file.
9. Launch the game through RetroBat when the user requests runtime verification. Capture the launcher/emulator log and close the test process cleanly so buffered logs are flushed.
10. On failure, stop the import, preserve the failed staging/partial content, and restore the previous installation when a backup exists. Never silently delete a user's source or saves.

## Safe command examples

Read-only plan:

```powershell
pwsh -NoProfile -File .\scripts\Import-RetroDriveGame.ps1 `
  -SourcePath "C:\Games\Persona 4 Golden" `
  -RetroDriveRoot "D:\" `
  -Platform psvita
```

Apply an approved plan:

```powershell
pwsh -NoProfile -File .\scripts\Import-RetroDriveGame.ps1 `
  -SourcePath "C:\Games\Persona 4 Golden" `
  -RetroDriveRoot "D:\" `
  -Platform psvita `
  -Apply
```

## Non-negotiable safeguards

- Never overwrite a conflicting ROM or emulator-owned installation without moving it to a dated, recoverable backup first.
- Never assume an `.m3u` file is safe to pass through unchanged. Trim and validate every referenced line; use `.psvita` for Vita3K installed content.
- Never kill Vita3K while it is still producing a `_dec` directory. Wait for the install completion signal, then close it normally.
- Keep ROMs, emulator-owned storage, saves, states, firmware, keys, and backups in their designated locations. Do not place emulator saves beside shared ROMs.
- Do not download games, firmware, keys, or copyrighted material. Operate only on files supplied by the user.

## References

- Read [common.md](references/common.md) for naming, staging, and verification rules.
- Read [psvita.md](references/psvita.md) before importing PSVita content.
- Read [ps3.md](references/ps3.md) before importing PS3 PKG or update content.
- Read [xbox.md](references/xbox.md) before handling Xbox ISO/XISO conversion.
- Read [multidisc.md](references/multidisc.md) before creating or editing multi-disc launch files.
