# RetroDrive Launcher

Source repository for the RetroDrive fullscreen Windows launcher.

This repository contains source code, tests, build configuration, dependency lockfiles, and developer documentation only. It is deliberately separate from the portable RetroDrive release tree at `C:\Users\yuans\Downloads\RetroDrive`.

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

The detailed implementation, security, packaging, and acceptance requirements are maintained in:

```text
C:\Users\yuans\Downloads\RetroDrive\plans\02-launcher.md
C:\Users\yuans\Downloads\RetroDrive\plans\07-validation-release.md
```

## Initial Status

Repository initialized. Application scaffolding begins during RetroDrive Step 02.

