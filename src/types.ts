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

export interface ChildEndedEvent {
  entryId: EntryId;
  exitCode: number | null;
}

export type SkillStatus =
  | "notInstalled"
  | "upToDate"
  | "updateAvailable"
  | "localChanges"
  | "error";

export interface SkillSummary {
  id: string;
  displayName: string;
  description: string;
  status: SkillStatus;
  sourceHash: string;
  destination: string;
}

export type UiMode = "home" | "launching" | "error" | "exit" | "skills";
