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

The GitHub repository stores the 32-byte Base64 release seed as the Actions secret `RETRODRIVE_SIGNING_PRIVATE_KEY`. Never commit or distribute it.

Launcher code merged or pushed to `main` automatically receives the next patch version and is published after all checks and the production build pass. Pull requests run CI automatically but never publish. Documentation-only changes run CI on `main` and are excluded from automatic releases.

For an explicit release, use one of these commands from a clean `main` branch that exactly matches `origin/main`:

```powershell
pnpm release:patch
pnpm release:minor
pnpm release:major
```

The local command asks GitHub to perform the release, follows the run until completion, and then fast-forwards local `main`. No local release build or signing key is needed.

The same operation is available in the GitHub web interface:

1. Open **Actions**.
2. Select **Release portable launcher**.
3. Choose **Run workflow**.
4. Select `patch`, `minor`, or `major`; optionally add a short update summary.

For automatic patch releases and explicit releases, GitHub calculates the next version, updates all four version sources, commits the version change, creates the annotated tag, tests the source, builds the production binaries, creates the allowlisted portable ZIP, signs `latest.json`, writes `SHA256SUMS.txt`, and publishes the Release. A concurrency lock prevents two stable releases from running at the same time.

Pushing a correctly prepared `vX.Y.Z` tag remains supported as an emergency path. The workflow rejects the tag unless all four source versions already match it.
