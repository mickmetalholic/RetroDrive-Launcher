import type { BackupSnapshot, BackupStatus } from "./types";

export function sampleBackupStatus(): BackupStatus {
  const mib = 1024 ** 2;
  const newestComplete: BackupSnapshot = {
    id: "c692afb4",
    time: "2026-11-13T21:30:00+08:00",
    complete: true,
    files: 12484,
    bytes: 6912 * mib,
    addedBytes: 32 * mib,
  };
  const snapshots: BackupSnapshot[] = [
    {
      id: "43fe80b1",
      time: "2026-11-14T09:12:00+08:00",
      complete: false,
      files: 12410,
      bytes: 6880 * mib,
      addedBytes: 4 * mib,
    },
    newestComplete,
    {
      id: "7da136ef",
      time: "2026-11-10T20:45:00+08:00",
      complete: true,
      files: 12420,
      bytes: 6880 * mib,
      addedBytes: 18 * mib,
    },
    {
      id: "a9db1c25",
      time: "2026-11-06T20:05:00+08:00",
      complete: true,
      files: 12374,
      bytes: 6862 * mib,
      addedBytes: 4930 * mib,
    },
  ];

  return {
    repository: "\\\\KAELTHAS\\personal_folder\\Backup\\RetroDrive\\restic",
    busy: false,
    phase: "idle",
    detail: "",
    snapshots,
    filesDone: 0,
    bytesDone: 0,
    totalFiles: 0,
    totalBytes: 0,
    snapshotId: null,
    restorePath: null,
    lastSuccess: {
      snapshotId: newestComplete.id,
      time: newestComplete.time,
      files: newestComplete.files,
      bytes: newestComplete.bytes,
      addedBytes: newestComplete.addedBytes,
    },
  };
}

export function sampleBackupAction(state: BackupStatus, action: string, snapshotId: string): BackupStatus {
  switch (action) {
    case "backup": {
      const snapshot = {
        id: crypto.randomUUID().replaceAll("-", ""),
        time: new Date().toISOString(),
        complete: true,
        files: 12490,
        bytes: 6920 * 1024 ** 2,
        addedBytes: 8 * 1024 ** 2,
      };
      return {
        ...state, phase: "completed", detail: "Sample snapshot saved.",
        snapshots: [snapshot, ...state.snapshots], snapshotId: snapshot.id,
        lastSuccess: { ...snapshot, snapshotId: snapshot.id },
      };
    }
    case "history":
      return { ...state, phase: "completed", detail: "Sample history refreshed." };
    case "restore":
      if (!state.snapshots.some((snapshot) => snapshot.id === snapshotId && snapshot.complete)) return state;
      return {
        ...state, phase: "completed", detail: "Sample restore completed.",
        restorePath: `R:\\Launcher\\data\\backup\\restores\\${snapshotId.slice(0, 8)}`,
      };
    default:
      return state;
  }
}
