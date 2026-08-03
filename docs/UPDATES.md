# Portable Update System

RetroDrive Launcher checks the stable GitHub Release channel whenever `START.exe` runs. The check has a three-second hard timeout. Network failures are recorded locally and never prevent RetroDrive from starting.

## User Flow

1. `START.exe` downloads `latest.json` from the latest GitHub Release.
2. The manifest is verified with the Ed25519 public key compiled into `START.exe`.
3. If the signed semantic version is newer, the user can choose **Yes** to update or **No** to continue immediately.
4. The selected ZIP is downloaded and verified against the signed size and SHA-256 digest.
5. A temporary copy of `Updater.exe` verifies everything again, extracts only the allowlisted launcher files, and replaces them after `START.exe` exits.
6. The updater launches the new `START.exe`.

Offline startup and an unavailable GitHub service are both normal supported states.

## Protected Boundary

An update may replace only:

```text
START.exe
Launcher/RetroDriveLauncher.exe
Launcher/Updater.exe
Launcher/assets/icons/retrobat.png
Launcher/assets/icons/retroarch.png
Launcher/release/USER-GUIDE.md
Launcher/release/VERSION.json
```

The archive extractor rejects absolute paths, parent traversal, duplicate paths, and every file outside this allowlist. Releases never contain or modify RetroBat, RetroArch, emulators, ROMs, BIOS or firmware, encryption keys, saves, states, `launcher.json`, or runtime data.

## Recovery

Before replacing any existing file, the updater moves it to:

```text
Launcher/data/updates/previous/
```

If any replacement fails, all files changed in that transaction are rolled back before the installed `START.exe` is relaunched. The last result is recorded in `Launcher/data/update-state.json`.

## Publishing

All three version declarations must match:

- `package.json`
- `src-tauri/Cargo.toml`
- `src-tauri/tauri.conf.json`

The GitHub repository stores the 32-byte Base64 release seed as the Actions secret `RETRODRIVE_SIGNING_PRIVATE_KEY`. Never commit or distribute it. To publish a release:

```powershell
git tag -a v0.3.0 -m "RetroDrive Launcher v0.3.0"
git push origin v0.3.0
```

The release workflow tests the source, builds all production binaries, creates the allowlisted portable ZIP, signs `latest.json`, writes `SHA256SUMS.txt`, and publishes all three files to GitHub Releases.
