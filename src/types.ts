export type EntryId = "retrobat" | "retroarch";

export interface LauncherEntry {
  id: EntryId;
  title: "RetroBat" | "RetroArch";
  primary: boolean;
}

export interface LauncherState {
  version: string;
  defaultEntry: EntryId;
  activeEntry: EntryId | null;
}

export interface EmulatorSummary {
  name: string;
  path: string;
}

export interface PlatformSummary {
  name: string;
  gameFiles: number;
  bytes: number;
}

export interface DriveOverview {
  root: string;
  drive: string;
  totalBytes: number;
  freeBytes: number;
  usedBytes: number;
  romRoot: string;
  platformCount: number;
  systemsWithGames: number;
  gameFiles: number;
  gameBytes: number;
  saveBytes: number;
  emulatorCount: number;
  retroArchCoreCount: number;
  emulators: EmulatorSummary[];
  platforms: PlatformSummary[];
  warnings: string[];
  scannedAtUnix: number;
}

export type UpdateStatus =
  | "notChecked"
  | "current"
  | "available"
  | "failed"
  | "installing";

export interface UpdateInfo {
  currentVersion: string;
  availableVersion: string | null;
  publishedAt: string | null;
  notes: string | null;
  checkedAtUnix: number | null;
  status: UpdateStatus;
  detail: string;
}

export interface ChildEndedEvent {
  entryId: EntryId;
  exitCode: number | null;
}

export type UiMode = "home" | "launching" | "error" | "exit" | "settings";
export type SettingsSection = "overview" | "updates";
export type HomeSelection = EntryId | "settings";
