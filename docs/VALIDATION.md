# Validation Record

## 2026-08-03 Host A smoke test

Environment: Windows 11 Pro x64 build 26200, Intel Core i5-14600KF,
NVIDIA GeForce RTX 4080, dual 3840 x 2160 displays at 150% scaling.

- `START.exe` completed preflight with exit code 0 and started the native launcher.
- The packaged Tauri window rendered the embedded RetroDrive home screen at
  3840 x 2160 without a localhost dependency or framework error overlay.
- The RetroArch entry started `RetroArch\retroarch.exe`, hid the main launcher
  window, and restored it after RetroArch exited with code 0.
- The RetroBat entry started `RetroBat\RetroBat.exe`, tracked the spawned
  `emulationstation.exe`, kept the main launcher window hidden for that session,
  and restored it after EmulationStation exited. The direct RetroBat process
  returned code 0.
- Independent RetroArch loaded the Chinese translation of Kirby's Adventure
  through `mesen_libretro.dll`. The game rendered and remained responsive.
- The ROM inventory remained 65 files and 90,686,197,177 bytes. The tested ROM
  retained SHA-256
  `B1FF57FC59EA6274413B6CE611A008E5157CE3E6539DDF7F4429EFB5BED4D497`.
- The generated SRAM file was isolated under `RetroArch\saves`; no file was
  created or changed under the shared `roms` tree.

Remaining release-matrix work: physical controller endurance, every platform's
representative game, changed drive letter, second Windows host, sleep/resume,
safe-eject, and unexpected-removal recovery.
