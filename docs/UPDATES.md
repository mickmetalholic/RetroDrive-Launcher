# Portable Update System

RetroDrive Launcher performs a background check against the stable GitHub Release channel after startup. The **Settings → Updates** page also provides a manual check. Network failures are recorded locally and never prevent RetroDrive from starting; a newer release is marked with a red dot beside the version.

## User Flow

1. The launcher starts and checks the stable release channel in the background.
2. The user can open **Settings → Updates** and choose **Check for updates** at any time.
3. The manifest is verified with the Ed25519 public key compiled into `START.exe`.
4. If the signed semantic version is newer, the user can choose **Install update**.
5. The selected ZIP is downloaded and verified against the signed size and SHA-256 digest.
6. A temporary copy of `Updater.exe` verifies everything again, extracts only the allowlisted launcher files, and replaces them after `START.exe` exits.
7. The updater launches the new `START.exe`.

Offline startup and an unavailable GitHub service are both normal supported states. The home screen never blocks on the background request.

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
Launcher/skills/**
```

The archive extractor rejects absolute paths, parent traversal, duplicate paths, and every file outside this allowlist. `Launcher/skills/**` is the only extensible directory: it contains the validated skill tree and manifest shipped by the repository. The updater does not touch the local Codex skills directory. Releases never contain or modify RetroBat, RetroArch, emulators, ROMs, BIOS or firmware, encryption keys, saves, states, `launcher.json`, or runtime data.

## Drive-local skill installation

The signed update package keeps its release-managed skill source under `Launcher/skills` so existing updaters can install new releases without widening their file allowlist. When the updated launcher starts, it validates that source and automatically installs each bundled skill into `.agents/skills/<skill-id>` at the portable drive root. The same installation runs on the first launch of a freshly packaged drive. Open the drive root as the Codex working folder to use these project-scoped skills.

Installation validates the manifest and deterministic tree hash, stages the new directory under `Launcher/data/skill-staging`, refuses links and untracked drive changes, and keeps the previous managed version under `Launcher/data/skill-backups`. It only manages the bundled skill directories and preserves unrelated skills already in `.agents/skills`. `Launcher/data/drive-skills-state.json` records the last successfully installed source hashes; unchanged installations are left alone. If deployment fails, previous drive skills are restored and the error is logged without preventing the Launcher from opening.

The launcher installs skills on the portable drive. It leaves legacy host-synchronized copies and their state files untouched.

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
