import type { DriveOverview } from "./types";

export function sampleDriveOverview(): DriveOverview {
  const gib = 1024 ** 3;
  const root = "R:\\";
  const platforms = [
    { name: "snes", gameFiles: 42, bytes: 220 * 1024 ** 2 },
    { name: "gba", gameFiles: 31, bytes: 310 * 1024 ** 2 },
    { name: "ps2", gameFiles: 24, bytes: 84 * gib },
    { name: "psx", gameFiles: 18, bytes: 10 * gib },
    { name: "psp", gameFiles: 12, bytes: 10 * gib },
    { name: "gamecube", gameFiles: 9, bytes: 12 * gib },
    { name: "ps3", gameFiles: 6, bytes: 78 * gib },
    { name: "wiiu", gameFiles: 4, bytes: 32 * gib },
  ];
  const emulators = ["retroarch", "pcsx2", "duckstation", "ppsspp", "dolphin", "rpcs3", "cemu", "azahar", "vita3k", "xenia-canary"]
    .map((name) => ({ name, path: `${root}RetroBat\\emulators\\${name}` }));

  return {
    root,
    drive: "R:",
    totalBytes: 465 * gib,
    freeBytes: 173 * gib,
    usedBytes: 292 * gib,
    romRoot: `${root}roms`,
    platformCount: 64,
    systemsWithGames: platforms.length,
    gameFiles: platforms.reduce((total, platform) => total + platform.gameFiles, 0),
    gameBytes: platforms.reduce((total, platform) => total + platform.bytes, 0),
    saveBytes: 768 * 1024 ** 2,
    emulatorCount: emulators.length,
    retroArchCoreCount: 38,
    emulators,
    platforms,
    warnings: [],
    scannedAtUnix: Math.floor(Date.now() / 1000),
  };
}
