# RetroDrive Launcher

Source repository for the RetroDrive fullscreen Windows launcher.

[Releases](https://github.com/mickmetalholic/RetroDrive-Launcher/releases) provide signed, portable launcher-only updates. The launcher remains fully usable when the update service or internet connection is unavailable.

This repository contains source code, tests, build configuration, dependency lockfiles, and developer documentation only. It is a nested Git repository inside the RetroDrive planning workspace, while remaining separate from the future portable release tree.

## Product Boundary

- `START.exe` is a small native Rust Windows-subsystem bootstrapper.
- `RetroDriveLauncher.exe` is a Tauri 2 application with a Rust backend and vanilla TypeScript/HTML/CSS frontend.
- `Updater.exe` is a native transactional updater that runs from a temporary copy so every launcher-owned executable can be replaced safely.
- The launcher exposes only RetroBat and independent RetroArch.
- `skills/` contains the versioned, validated Codex skills shipped with the launcher. The Skills screen can copy an approved skill into the local Codex skills directory after an explicit user action.
- Runtime executables are resolved from manifest-approved paths below the attached RetroDrive root.
- ROMs, BIOS files, firmware, encryption keys, saves, credentials, and emulator binaries must never be committed to this repository.

## Repository Contract

- Default branch: `main`
- Normal work: short-lived topic branches
- Rust and JavaScript dependencies: committed lockfiles
- Releases: annotated SemVer tags such as `v0.2.0`, built and signed by GitHub Actions
- Build output: copied into a staging RetroDrive tree only after tests pass
- Production replacement: performed from a validated staging build, never directly from a development build directory

The detailed implementation, security, packaging, and acceptance requirements are maintained in the adjacent planning workspace:

```text
..\plans\02-launcher.md
..\plans\07-validation-release.md
```

## Development

Requirements: Node.js 24, pnpm 10.33.2, Rust 1.97.1, and the Visual Studio C++
desktop workload.

```powershell
pnpm install --frozen-lockfile
pnpm check
pnpm skills:manifest   # only after intentionally changing a skill
pnpm build:release
powershell -NoProfile -ExecutionPolicy Bypass -File scripts/package-portable.ps1 -Root D:\
```

`build:release` is the only supported release-binary build path. It creates the
Tauri production launcher first, builds only `START.exe` afterward, and records
both hashes. Do not run `cargo build --release --bins` between this command and
packaging: a plain Cargo build does not establish the Tauri production frontend
contract.

The portable packaging script validates the external RetroDrive root and the
recorded production hashes before it copies release binaries. It never copies
ROMs, firmware, keys, saves, or emulator binaries into this repository.

See [docs/UPDATES.md](docs/UPDATES.md) for the signed update protocol, strict file boundary, rollback behavior, and publishing procedure.

See [skills/retrodrive-import-game/SKILL.md](skills/retrodrive-import-game/SKILL.md) for the first supported game-import workflow. Skills are validated with `pnpm skills:validate`; the generated `skills/skills-manifest.json` is part of the signed release package.

## Release

Every Launcher code change merged or pushed to `main` automatically runs CI and publishes the next patch release. Documentation-only changes run CI but do not create a Release.

For an explicit release, run one command from a clean, synchronized `main` branch:

```powershell
pnpm release:patch
```

`release:minor` and `release:major` are available for larger version changes. The command starts the GitHub workflow, follows it through completion, and fast-forwards the local checkout to the automated version commit. The same operation is available from **Actions → Release portable launcher → Run workflow** without a local development environment. Pull requests run CI automatically and never publish a Release before merge.
