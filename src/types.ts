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

export type UiMode = "home" | "launching" | "error" | "exit";
