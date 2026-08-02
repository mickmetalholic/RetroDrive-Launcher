# RetroDrive Launcher

Source repository for the RetroDrive fullscreen Windows launcher.

This repository contains source code, tests, build configuration, dependency lockfiles, and developer documentation only. It is a nested Git repository inside the RetroDrive planning workspace, while remaining separate from the future portable release tree.

## Product Boundary

- `START.exe` is a small native Rust Windows-subsystem bootstrapper.
- `RetroDriveLauncher.exe` is a Tauri 2 application with a Rust backend and vanilla TypeScript/HTML/CSS frontend.
- The launcher exposes only RetroBat and independent RetroArch.
- Runtime executables are resolved from manifest-approved paths below the attached RetroDrive root.
- ROMs, BIOS files, firmware, encryption keys, saves, credentials, and emulator binaries must never be committed to this repository.

## Repository Contract

- Default branch: `main`
- Normal work: short-lived topic branches
- Rust and JavaScript dependencies: committed lockfiles
- Releases: annotated Git tags matching the RetroDrive release identifier
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
