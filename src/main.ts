import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { platformIcon } from "./platform-icons";
import { sampleBackupAction, sampleBackupStatus } from "./preview-backup";
import { sampleDriveOverview } from "./preview-drive";
import "./styles.css";
import type {
  ChildEndedEvent,
  BackupStatus,
  BackupSnapshot,
  DriveOverview,
  EntryId,
  HomeSelection,
  LauncherEntry,
  LauncherState,
  SettingsSection,
  UiMode,
  UpdateInfo,
} from "./types";

function requireApplicationRoot(): HTMLDivElement {
  const root = document.querySelector<HTMLDivElement>("#app");
  if (!root) throw new Error("Application root is missing");
  return root;
}

const app = requireApplicationRoot();
const isTauriRuntime = "__TAURI_INTERNALS__" in window;
const samplePage = import.meta.env.DEV && !isTauriRuntime
  ? new URLSearchParams(window.location.search).get("mock")
  : null;
const showSampleData = samplePage === "storage" || samplePage === "backup";

let entries: LauncherEntry[] = [];
let selected: HomeSelection = "retrobat";
let mode: UiMode = "home";
let settingsSection: SettingsSection = "overview";
const sectionTitles: Record<SettingsSection, string> = {
  overview: "Storage",
  updates: "Updates",
  backup: "Backup",
};
const settingsSections = Object.keys(sectionTitles) as SettingsSection[];
let errorEntry: EntryId | null = null;
let settingsError: string | null = null;
let driveBusy = false;
let driveOverview: DriveOverview | null = null;
let updateBusy = false;
let updateInfo: UpdateInfo | null = null;
let backupStatus: BackupStatus | null = null;
let backupPoll: number | undefined;
let pendingRestoreId: string | null = null;
let inputLockedUntilNeutral = false;
let launcherVersion = "0.2.2";
type InputMethod = "keyboard" | "gamepad" | "mouse";
let inputMethod: InputMethod = "keyboard";
let animateHomeSelection = false;
let animateScreenEnter = true;
let animateSettingsPanel = false;
let animateSettingsTab = false;

const entryTitle = (id: EntryId) =>
  entries.find((entry) => entry.id === id)?.title ??
  (id === "retrobat" ? "RetroBat" : "RetroArch");

function iconPath(id: string): string {
  return `/assets/icons/${id}.png`;
}

function escapeHtml(value: string): string {
  return value.replace(/[&<>"']/g, (character) => {
    const entities: Record<string, string> = {
      "&": "&amp;",
      "<": "&lt;",
      ">": "&gt;",
      '"': "&quot;",
      "'": "&#39;",
    };
    return entities[character] ?? character;
  });
}

function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  const units = ["B", "KiB", "MiB", "GiB", "TiB"];
  const exponent = Math.min(
    units.length - 1,
    Math.floor(Math.log(bytes) / Math.log(1024)),
  );
  const value = bytes / 1024 ** exponent;
  const digits = exponent === 0 ? 0 : value >= 100 ? 0 : value >= 10 ? 1 : 2;
  return `${value.toFixed(digits)} ${units[exponent]}`;
}

function formatDate(unix: number | null | undefined): string {
  if (!unix) return "Not recorded";
  return new Date(unix * 1000).toLocaleString("en-US", {
    dateStyle: "medium",
    timeStyle: "short",
  });
}

const displayNames = new Map(Object.entries({
  gba: "GBA",
  snes: "SNES",
  psx: "PS1",
  ps2: "PS2",
  ps3: "PS3",
  psp: "PSP",
  gamecube: "GameCube",
  gc: "GameCube",
  wiiu: "Wii U",
}));

function formatName(value: string): string {
  return displayNames.get(value.toLowerCase()) ?? value
    .replace(/[-_]+/g, " ")
    .replace(/\b[a-z]/g, (character) => character.toUpperCase());
}

function updateStatusLabel(status: UpdateInfo["status"]): string {
  const labels: Record<UpdateInfo["status"], string> = {
    notChecked: "Not checked",
    current: "Up to date",
    available: "Update available",
    failed: "Check failed",
    installing: "Installing update",
  };
  return labels[status];
}

function homeItems(): HomeSelection[] {
  return [...entries.map((entry) => entry.id), "settings"];
}

function setInputMethod(method: InputMethod): void {
  if (inputMethod === method) return;
  inputMethod = method;
  const content = document.querySelector<HTMLElement>(".settings-content");
  if (content) content.dataset.input = method;
  const dialog = document.querySelector<HTMLDialogElement>(".restore-dialog");
  if (dialog) dialog.dataset.input = method;
  updateInputHints();
}

function hint(key: string, label: string): string {
  return `<span><kbd>${key}</kbd>${label}</span>`;
}

function homeHints(): string {
  if (inputMethod === "gamepad") {
    return `${hint("D-PAD", "Navigate")}${hint("A", "Launch")}${hint("B", "Exit")}`;
  }
  if (inputMethod === "mouse") {
    return hint("ESC", "Exit");
  }
  return `${hint("↑ ↓", "Navigate")}${hint("ENTER", "Launch")}${hint("ESC", "Exit")}`;
}

function settingsHints(snapshotFocused = false): string {
  if (inputMethod === "mouse") {
    return `${snapshotFocused ? hint("CLICK", "Open backup") : ""}${hint("ESC", "Back")}`;
  }
  const tabHint =
    inputMethod === "gamepad"
      ? hint("LB / RB", "Switch tabs")
      : hint("← →", "Switch tabs");
  const backHint = inputMethod === "gamepad" ? hint("B", "Back") : hint("ESC", "Back");
  let actionHint = "";
  if (settingsSection === "overview") {
    actionHint = inputMethod === "gamepad" ? hint("A", "Refresh") : hint("R", "Refresh");
  } else if (settingsSection === "updates") {
    actionHint = inputMethod === "gamepad" ? hint("A", "Check for updates") : hint("C", "Check for updates");
    if (updateInfo?.status === "available") {
      actionHint += inputMethod === "gamepad" ? hint("X", "Install") : hint("I", "Install");
    }
  } else if (settingsSection === "backup") {
    actionHint = inputMethod === "gamepad"
      ? `${hint("D-PAD", "Move")}${hint("A", snapshotFocused ? "Open backup" : "Select")}`
      : `${hint("TAB", "Move")}${snapshotFocused ? hint("ENTER", "Open backup") : ""}`;
  }
  return `${tabHint}${actionHint}${backHint}`;
}

function restoreHints(): string {
  if (inputMethod === "gamepad") return `${hint("D-PAD", "Move")}${hint("A", "Select")}${hint("B", "Back")}`;
  if (inputMethod === "mouse") return hint("ESC", "Back");
  return `${hint("TAB", "Move")}${hint("ENTER", "Select")}${hint("ESC", "Back")}`;
}

function updateIndicator(): string {
  if (updateInfo?.status === "available") {
    return `<span class="update-indicator is-available" title="Update available" aria-label="Update available"><span class="update-dot"></span></span>`;
  }
  if (updateBusy) {
    return `<span class="update-indicator is-checking" title="Checking for updates" aria-label="Checking for updates"><span class="update-ring"></span></span>`;
  }
  return "";
}

function updateInputHints(): void {
  const hints = document.querySelector<HTMLElement>(".hints");
  const restoreControls = document.querySelector<HTMLElement>(".restore-hints");
  if (restoreControls) restoreControls.innerHTML = restoreHints();
  if (hints) {
    const content =
      mode === "settings"
        ? settingsHints(Boolean(focusedBackupControl()?.matches("[data-backup-snapshot]:enabled")))
        : mode === "error"
          ? `${inputMethod === "gamepad" ? hint("A", "Retry") : hint("ENTER", "Retry")}${inputMethod === "gamepad" ? hint("B", "Back") : hint("ESC", "Back")}`
          : homeHints();
    hints.innerHTML = `${content}<span class="footer-version">${updateIndicator()}RetroDrive v${escapeHtml(launcherVersion)}</span>`;
  }
}

function renderHome(): void {
  const launchCopy = mode === "launching" ? `Launching ${entryTitle(selected as EntryId)}…` : "";
  const errorCopy =
    mode === "error" && errorEntry
      ? `Could not launch ${entryTitle(errorEntry)}`
      : "";
  app.innerHTML = `
    <main class="shell home-shell ${animateScreenEnter ? "is-screen-entering" : ""}" aria-label="RetroDrive launcher">
      <header class="topbar">
        <div class="brand">RetroDrive</div>
      </header>

      <section class="tiles" aria-label="Applications">
        ${entries
          .map((entry) => {
            const focused = selected === entry.id;
            const launching = focused && mode === "launching";
            const failed = focused && mode === "error";
            return `
              <button
                class="launch-tile ${focused ? "is-focused" : "is-idle"} ${focused && animateHomeSelection ? "is-selection-entering" : ""} ${launching ? "is-launching" : ""} ${failed ? "is-error" : ""}"
                data-entry="${escapeHtml(entry.id)}"
                type="button"
                aria-label="${escapeHtml(entry.title)}"
                aria-current="${focused ? "true" : "false"}"
                ${mode === "launching" || mode === "exit" ? "disabled" : ""}
              >
                <span class="icon-box"><img src="${iconPath(entry.id)}" alt="" draggable="false" /></span>
                <span class="tile-copy">
                  <span class="tile-title">${escapeHtml(entry.title)}</span>
                  ${launching ? `<span class="transient">${escapeHtml(launchCopy)}</span><span class="progress" aria-hidden="true"></span>` : ""}
                  ${failed ? `<span class="transient error-copy">${escapeHtml(errorCopy)}</span>` : ""}
                </span>
              </button>`;
          })
          .join("")}
        <button
          class="launch-tile settings-tile ${selected === "settings" ? "is-focused" : "is-idle"} ${selected === "settings" && animateHomeSelection ? "is-selection-entering" : ""}"
          data-home-settings
          type="button"
          aria-label="Settings"
          aria-current="${selected === "settings" ? "true" : "false"}"
          ${mode === "launching" || mode === "exit" ? "disabled" : ""}
        >
          <span class="settings-mark" aria-hidden="true"><img src="/assets/icons/settings-2.svg" alt="" draggable="false" /></span>
          <span class="tile-copy">
            <span class="tile-title">Settings</span>
          </span>
        </button>
      </section>

      <section class="home-summary" aria-label="Drive summary">
        <article class="summary-card">
          <div class="summary-label">Free space</div>
          <div class="summary-value">${driveOverview ? escapeHtml(formatBytes(driveOverview.freeBytes)) : "—"}</div>
          ${driveBusy ? `<div class="summary-note">Scanning drive…</div>` : ""}
        </article>
        <article class="summary-card">
          <div class="summary-label">Games</div>
          <div class="summary-value">${driveOverview ? driveOverview.gameFiles : "—"}</div>
        </article>
        <article class="summary-card">
          <div class="summary-label">Emulators</div>
          <div class="summary-value">${driveOverview ? driveOverview.emulatorCount : "—"}</div>
        </article>
      </section>

      ${
        mode === "exit"
          ? `<section class="dialog" role="dialog" aria-modal="true" aria-label="Exit RetroDrive">
               <p>Exit RetroDrive?</p>
               <div>${inputMethod === "gamepad" ? `${hint("A", "Confirm")}${hint("B", "Back")}` : `${hint("ENTER", "Confirm")}${hint("ESC", "Back")}`}</div>
             </section>`
          : ""
      }
      <footer class="hints ${mode === "launching" ? "is-muted" : ""}">
        ${
          mode === "error"
            ? `${inputMethod === "gamepad" ? hint("A", "Retry") : hint("ENTER", "Retry")}${inputMethod === "gamepad" ? hint("B", "Back") : hint("ESC", "Back")}`
            : `${homeHints()}<span class="footer-version">${updateIndicator()}RetroDrive v${escapeHtml(launcherVersion)}</span>`
        }
      </footer>
    </main>`;

  animateHomeSelection = false;
  animateScreenEnter = false;

  document.querySelectorAll<HTMLButtonElement>("[data-entry]").forEach((tile) => {
    tile.addEventListener("mouseenter", () => {
      setInputMethod("mouse");
      if (mode === "home" || mode === "error") {
        const nextEntry = tile.dataset.entry as EntryId;
        if (selected === nextEntry) return;
        selected = nextEntry;
        mode = "home";
        errorEntry = null;
        animateHomeSelection = true;
        renderHome();
      }
    });
    tile.addEventListener("click", () => void launch(tile.dataset.entry as EntryId));
  });
  document.querySelectorAll<HTMLButtonElement>("[data-home-settings]").forEach((button) => {
    button.addEventListener("mouseenter", () => {
      setInputMethod("mouse");
      if (mode === "home") {
        if (selected === "settings") return;
        selected = "settings";
        animateHomeSelection = true;
        renderHome();
      }
    });
    button.addEventListener("click", () => openSettings());
  });
}

function renderSettings(): void {
  const previousContent = document.querySelector<HTMLElement>(".settings-content");
  const scrollTop = previousContent?.dataset.section === settingsSection ? previousContent.scrollTop : 0;
  const backupFocus = focusedBackupControl()?.dataset.backupFocus;
  const tabIndex = settingsSections.indexOf(settingsSection);
  const showingSamples = showSampleData && settingsSection !== "updates";
  const pageError = settingsSection === "updates" && updateInfo?.detail === settingsError
    ? null
    : settingsError;
  const nav = settingsSections
    .map(
      (section) =>
        `<button class="settings-nav ${settingsSection === section ? "is-active" : ""}" data-settings-section="${section}" type="button" aria-current="${settingsSection === section ? "page" : "false"}"><span class="tab-label">${sectionTitles[section]}</span></button>`,
    )
    .join("");
  const content =
    settingsSection === "overview"
      ? renderOverviewContent()
      : settingsSection === "updates"
        ? renderUpdateContent()
        : renderBackupContent();

  app.innerHTML = `
    <main class="shell settings-shell ${animateScreenEnter ? "is-screen-entering" : ""}" aria-label="RetroDrive settings">
      <header class="topbar">
        <div class="brand">RetroDrive</div>
      </header>
      <nav class="settings-nav-bar ${animateSettingsTab ? "is-tab-changing" : ""}" data-active="${settingsSection}" style="--tab-index:${tabIndex}; --tab-count:${settingsSections.length}" aria-label="Settings sections">${nav}</nav>
      <section class="settings-content ${animateSettingsPanel ? "is-section-entering" : ""}" data-section="${settingsSection}" data-input="${inputMethod}">
        <h1 class="visually-hidden">${sectionTitles[settingsSection]}</h1>
        ${showingSamples ? `<p class="sample-label">Sample data</p>` : ""}
        ${pageError ? `<p class="settings-error">${escapeHtml(pageError)}</p>` : ""}
        ${content}
      </section>
      <footer class="hints ${driveBusy || updateBusy ? "is-muted" : ""}">
        ${settingsHints()}<span class="footer-version">${updateIndicator()}RetroDrive v${escapeHtml(launcherVersion)}</span>
      </footer>
    </main>`;

  animateSettingsPanel = false;
  animateSettingsTab = false;
  animateScreenEnter = false;

  document.querySelectorAll<HTMLButtonElement>("[data-backup-action]").forEach((button) => {
    button.addEventListener("click", () => {
      if (button.dataset.backupAction) void runBackupAction(button.dataset.backupAction);
    });
  });
  document.querySelectorAll<HTMLButtonElement>("[data-backup-snapshot]").forEach((button) => {
    button.addEventListener("click", () => {
      pendingRestoreId = button.dataset.backupSnapshot ?? null;
      renderSettings();
    });
  });
  document.querySelector("[data-restore-back]")?.addEventListener("click", closeRestoreDialog);
  document.querySelector("[data-backup-cancel]")?.addEventListener("click", () => void backupRequest("cancel_backup"));
  document.querySelector("[data-open-restored]")?.addEventListener("click", () => void backupRequest("open_restored_files"));

  document.querySelectorAll<HTMLButtonElement>("[data-settings-section]").forEach((button) => {
    button.addEventListener("click", () => {
      const section = button.dataset.settingsSection as SettingsSection;
      switchSettingsSection(section);
    });
  });
  document.querySelectorAll<HTMLButtonElement>("[data-refresh-drive]").forEach((button) => {
    button.addEventListener("click", () => void refreshDriveOverview());
  });
  document.querySelectorAll<HTMLButtonElement>("[data-check-update]").forEach((button) => {
    button.addEventListener("click", () => void checkForUpdate());
  });
  document.querySelectorAll<HTMLButtonElement>("[data-install-update]").forEach((button) => {
    button.addEventListener("click", () => void installUpdate());
  });
  const nextContent = document.querySelector<HTMLElement>(".settings-content");
  if (nextContent) nextContent.scrollTop = scrollTop;
  const restoreDialog = document.querySelector<HTMLDialogElement>(".restore-dialog");
  if (restoreDialog) {
    restoreDialog.addEventListener("cancel", (event) => {
      event.preventDefault();
      closeRestoreDialog();
    });
    restoreDialog.showModal();
    const previous = availableBackupControls().find((control) => control.dataset.backupFocus === backupFocus);
    previous?.focus();
  } else if (settingsSection === "backup" && (backupFocus || inputMethod === "gamepad")) {
    const controls = availableBackupControls();
    const previous = controls.find((control) => control.dataset.backupFocus === backupFocus);
    (previous ?? controls[0])?.focus({ preventScroll: true });
  }
}

function focusedBackupControl(): HTMLButtonElement | null {
  const control = document.activeElement;
  return control instanceof HTMLButtonElement && control.matches("[data-backup-focus]")
    ? control
    : null;
}

function availableBackupControls(): HTMLButtonElement[] {
  const scope = document.querySelector(".restore-dialog[open]") ?? app;
  return [...scope.querySelectorAll<HTMLButtonElement>("[data-backup-focus]:enabled")];
}

function closeRestoreDialog(): void {
  const snapshotId = pendingRestoreId;
  pendingRestoreId = null;
  renderSettings();
  const row = availableBackupControls().find((control) => control.dataset.backupSnapshot === snapshotId);
  row?.focus({ preventScroll: true });
  row?.scrollIntoView({ block: "nearest", inline: "nearest" });
}

function moveBackupFocus(horizontal: number, vertical: number): void {
  const controls = availableBackupControls();
  const current = focusedBackupControl();
  let next: HTMLButtonElement | undefined = controls[0];
  if (current && !current.disabled) {
    const bounds = current.getBoundingClientRect();
    const centerX = bounds.x + bounds.width / 2;
    const centerY = bounds.y + bounds.height / 2;
    let best = Infinity;
    next = undefined;
    for (const control of controls) {
      if (control === current) continue;
      const target = control.getBoundingClientRect();
      const dx = target.x + target.width / 2 - centerX;
      const dy = target.y + target.height / 2 - centerY;
      const forward = horizontal ? dx * horizontal : dy * vertical;
      const side = horizontal ? Math.abs(dy) : Math.max(0, target.left - centerX, centerX - target.right);
      if (forward <= 4 || (horizontal && side > (bounds.height + target.height) / 3)) continue;
      // Keep vertical moves on the nearest row even when it only has an action on one side.
      const distance = forward + (horizontal ? side * 3 : Math.min(side, 40) * .35);
      if (distance >= best) continue;
      best = distance;
      next = control;
    }
  }
  next?.focus({ preventScroll: true });
  next?.scrollIntoView({ block: "nearest", inline: "nearest" });
}

function renderOverviewContent(): string {
  const drive = driveOverview;
  if (driveBusy && !drive) {
    return `<div class="loading-panel" role="status"><span class="spinner" aria-hidden="true"></span><strong>Scanning drive…</strong></div>`;
  }
  if (!drive) {
    return isTauriRuntime
      ? `<div class="loading-panel"><strong>Drive data unavailable</strong><button class="primary-button" data-refresh-drive type="button">Refresh drive</button></div>`
      : `<div class="loading-panel"><strong>Drive data is available in the desktop app.</strong></div>`;
  }
  const usedPercent = drive.totalBytes
    ? Math.min(100, Math.max(0, (drive.usedBytes / drive.totalBytes) * 100))
    : 0;
  const maxGames = drive.platforms[0]?.gameFiles || 1;
  const platformRows = drive.platforms
    .map((platform) => {
      const width = (platform.gameFiles / maxGames) * 100;
      return `<li class="platform-row">
        <span class="platform-name">${platformIcon(platform.name)}<span>${escapeHtml(formatName(platform.name))}</span></span>
        <span class="platform-bar" aria-hidden="true"><span style="width:${width}%"></span></span>
        <span class="platform-count">${platform.gameFiles.toLocaleString()}</span>
      </li>`;
    })
    .join("");
  const warnings = drive.warnings.length
    ? `<details class="scan-warnings"><summary>Scan notes</summary><ul>${drive.warnings.map((warning) => `<li>${escapeHtml(warning)}</li>`).join("")}</ul></details>`
    : "";

  return `
    <section class="setting-card storage-capacity" aria-label="Drive space">
      <div class="storage-capacity-heading">
        <p class="storage-free">${formatBytes(drive.freeBytes)} <span>free</span></p>
        <button class="secondary-button" data-refresh-drive type="button" ${driveBusy ? "disabled" : ""}>${showSampleData ? "Reload sample" : driveBusy ? "Scanning…" : "Rescan drive"}</button>
      </div>
      <div class="storage-meter" aria-hidden="true"><span style="width:${usedPercent.toFixed(1)}%"></span></div>
      <div class="storage-meter-labels"><span>${formatBytes(drive.usedBytes)} used</span><span>${formatBytes(drive.totalBytes)} total</span></div>
    </section>
    <section class="storage-library">
      <div class="storage-list-heading"><h2>Games</h2><span>${drive.gameFiles.toLocaleString()}</span></div>
      <p class="storage-list-summary">${drive.systemsWithGames.toLocaleString()} platforms · ${formatBytes(drive.gameBytes)}</p>
      ${platformRows ? `<ul class="platform-list">${platformRows}</ul>` : ""}
    </section>
    ${warnings}`;
}

function renderUpdateContent(): string {
  const info = updateInfo;
  const available = info?.status === "available" && info.availableVersion;
  const status = info?.status ?? "notChecked";
  const statusText = !isTauriRuntime
    ? "Updates are available in the desktop app."
    : updateBusy && status !== "installing" ? "Checking…" : updateStatusLabel(status);
  const detail = status === "failed" || status === "installing" ? info?.detail : null;
  const notes = info?.notes ? `<div class="release-notes"><span class="card-kicker">Release notes</span><p>${escapeHtml(info.notes)}</p></div>` : "";
  return `
      <article class="setting-card update-card ${available ? "has-update" : ""}">
        <div class="card-kicker">Launcher version</div>
        <div class="version-line"><span>v${escapeHtml(info?.currentVersion ?? launcherVersion)}</span>${available ? `<b>→</b><strong>v${escapeHtml(info.availableVersion ?? "")}</strong>` : ""}</div>
        <div class="update-status status-${status}" role="status">${escapeHtml(statusText)}</div>
        ${detail ? `<p class="update-detail">${escapeHtml(detail)}</p>` : ""}
        <div class="update-actions"><button class="secondary-button" data-check-update type="button" ${updateBusy || !isTauriRuntime ? "disabled" : ""}>Check for updates</button>${available ? `<button class="primary-button" data-install-update type="button" ${updateBusy ? "disabled" : ""}>Install update</button>` : ""}</div>
        ${info?.checkedAtUnix || info?.publishedAt ? `<div class="update-meta">${info.checkedAtUnix ? `<span>Last checked ${escapeHtml(formatDate(info.checkedAtUnix))}</span>` : ""}${info.publishedAt ? `<span>Published ${escapeHtml(formatDate(Date.parse(info.publishedAt) / 1000))}</span>` : ""}</div>` : ""}
      </article>
    ${notes}`;
}

function formatSnapshotSizes(snapshot: Pick<BackupSnapshot, "bytes" | "addedBytes">): string {
  return `${formatBytes(snapshot.bytes)} · ${formatBytes(snapshot.addedBytes)} added`;
}

function renderBackupContent(): string {
  const state = backupStatus;
  const busy = state?.busy ?? false;
  const unavailable = !isTauriRuntime && !showSampleData;
  const disabled = (condition: boolean) => condition ? "disabled" : "";
  const snapshots = state?.snapshots ?? [];
  const restoreSnapshot = pendingRestoreId
    ? snapshots.find((snapshot) => snapshot.id === pendingRestoreId && snapshot.complete)
    : undefined;
  if (!restoreSnapshot) pendingRestoreId = null;
  const lastSuccess = state?.lastSuccess;
  const operation = state && state.phase !== "idle" ? state : null;
  const percent = state?.totalBytes ? Math.min(100, 100 * state.bytesDone / state.totalBytes) : 0;
  return `<div class="backup-layout">
    <section class="setting-card backup-main">
      <div class="backup-toolbar">
        <div><h2>Backup destination</h2>
          <code class="backup-destination">${escapeHtml(state?.repository ?? "\\\\KAELTHAS\\personal_folder\\Backup\\RetroDrive\\restic")}</code>
        </div>
        <div class="backup-actions">
          ${busy ? `<button class="secondary-button" data-backup-cancel data-backup-focus="cancel">Cancel operation</button>` : `
            <button class="primary-button" data-backup-action="backup" data-backup-focus="backup" ${disabled(unavailable)}>Start backup</button>`}
        </div>
      </div>
      ${operation ? `<div class="backup-state" role="status"><strong>${escapeHtml(operation.phase.replace(/([A-Z])/g, " $1"))}</strong>${operation.detail ? `<span>${escapeHtml(operation.detail)}</span>` : ""}</div>` : ""}
      ${busy ? `<progress aria-label="Backup progress" max="100" ${state?.totalBytes ? `value="${percent}"` : ""}></progress>${state?.totalBytes ? `<div class="backup-progress-note">${state.filesDone} files · ${formatBytes(state.bytesDone)} / ${formatBytes(state.totalBytes)}</div>` : ""}` : ""}
    </section>
    <section class="backup-history"><div class="backup-toolbar"><h2>Backup history</h2><button class="secondary-button" data-backup-action="history" data-backup-focus="history" ${disabled(busy || unavailable)}>${snapshots.length ? "Refresh history" : "Load history"}</button></div>
      ${snapshots.length ? `
          <ul class="backup-snapshots" aria-label="Snapshots">
            ${snapshots.map((snapshot) => `
              <li><button class="backup-snapshot" type="button" data-backup-snapshot="${escapeHtml(snapshot.id)}" data-backup-focus="snapshot-${escapeHtml(snapshot.id)}" ${snapshot.complete ? 'aria-haspopup="dialog"' : ""} ${disabled(busy || unavailable || !snapshot.complete)}>
                <span class="snapshot-info">
                  <strong>${escapeHtml(formatDate(Date.parse(snapshot.time) / 1000))}</strong>
                  <span class="snapshot-id">${escapeHtml(snapshot.id.slice(0, 8))}</span>
                </span>
                <span class="snapshot-stats">
                  ${snapshot.complete ? `<span>${snapshot.files.toLocaleString()} files</span>` : `<span class="snapshot-incomplete">Incomplete</span>`}
                  <span class="snapshot-sizes">${formatSnapshotSizes(snapshot)}</span>
                </span>
              </button></li>
            `).join("")}
          </ul>`
        : lastSuccess ? `<p>Last successful backup · ${escapeHtml(formatDate(Date.parse(lastSuccess.time) / 1000))}<br>${lastSuccess.files.toLocaleString()} files · ${formatSnapshotSizes(lastSuccess)}</p>`
          : `<p>No snapshots loaded.</p>`}
      ${state?.restorePath ? `<div class="backup-recovery">
        <code class="backup-destination">${escapeHtml(state.restorePath)}</code>
        ${isTauriRuntime ? `<button class="secondary-button" data-open-restored data-backup-focus="open-restore" ${disabled(busy)}>Open restored files</button>` : ""}
      </div>` : ""}
    </section>
    ${restoreSnapshot ? `<dialog class="restore-dialog setting-card" aria-labelledby="restore-title" aria-describedby="restore-description" data-input="${inputMethod}">
      <h2 id="restore-title">Restore this backup?</h2>
      <p class="restore-version"><strong>${escapeHtml(formatDate(Date.parse(restoreSnapshot.time) / 1000))}</strong> · ${escapeHtml(restoreSnapshot.id.slice(0, 8))}</p>
      <p id="restore-description">Restores to a separate folder.</p>
      <div class="backup-actions">
        <button class="secondary-button" type="button" data-restore-back data-backup-focus="restore-back" autofocus>Back</button>
        <button class="primary-button" type="button" data-backup-action="restore" data-backup-focus="restore-confirm" ${disabled(busy || unavailable)}>Restore copy</button>
      </div>
      <div class="restore-hints">${restoreHints()}</div>
    </dialog>` : ""}
  </div>`;
}

async function refreshBackupStatus(): Promise<void> {
  if (!isTauriRuntime) return;
  try {
    backupStatus = await invoke<BackupStatus>("get_backup_status");
    if (mode === "settings" && settingsSection === "backup") renderSettings();
  } catch (error) {
    settingsError = error instanceof Error ? error.message : String(error);
    if (mode === "settings" && settingsSection === "backup") renderSettings();
  }
  window.clearTimeout(backupPoll);
  if (backupStatus?.busy) backupPoll = window.setTimeout(() => void refreshBackupStatus(), 700);
}

async function backupRequest(command: string, args?: Record<string, unknown>): Promise<void> {
  if (!isTauriRuntime) return;
  settingsError = null;
  try { await invoke(command, args); }
  catch (error) { settingsError = error instanceof Error ? error.message : String(error); }
  await refreshBackupStatus();
}

async function runBackupAction(action: string): Promise<void> {
  const snapshotId = action === "restore" ? pendingRestoreId : null;
  if (action === "restore") {
    if (!snapshotId || backupStatus?.busy ||
        !backupStatus?.snapshots.some((snapshot) => snapshot.id === snapshotId && snapshot.complete)) return;
    closeRestoreDialog();
  }
  if (showSampleData && backupStatus) {
    backupStatus = sampleBackupAction(backupStatus, action, snapshotId ?? "");
    renderSettings();
    return;
  }
  await backupRequest("backup_action", { action, snapshotId });
}

async function launch(id: EntryId): Promise<void> {
  if (backupStatus?.busy) { openSettings("backup"); return; }
  if (mode === "launching") return;
  selected = id;
  mode = "launching";
  errorEntry = null;
  renderHome();
  try {
    await invoke("launch_entry", { entryId: id });
  } catch {
    mode = "error";
    errorEntry = id;
    renderHome();
  }
}

async function refreshDriveOverview(): Promise<void> {
  if (showSampleData) {
    driveOverview = sampleDriveOverview();
    render();
    return;
  }
  if (driveBusy || !isTauriRuntime) return;
  driveBusy = true;
  if (mode === "settings" || mode === "home") render();
  try {
    driveOverview = await invoke<DriveOverview>("get_drive_overview");
  } catch (error) {
    settingsError = error instanceof Error ? error.message : String(error);
  } finally {
    driveBusy = false;
    if (mode === "settings" || mode === "home") render();
  }
}

async function loadUpdateStatus(): Promise<void> {
  if (!isTauriRuntime) {
    updateInfo = {
      currentVersion: launcherVersion,
      availableVersion: null,
      publishedAt: null,
      notes: null,
      checkedAtUnix: null,
      status: "notChecked",
      detail: "Preview mode does not contact the update service.",
    };
    if (mode === "settings" && settingsSection === "updates") {
      animateSettingsPanel = true;
      animateSettingsTab = true;
    }
    if (mode === "settings" || mode === "home") render();
    return;
  }
  if (updateInfo) {
    return;
  }
  try {
    updateInfo = await invoke<UpdateInfo>("get_update_status");
  } catch (error) {
    settingsError = error instanceof Error ? error.message : String(error);
  }
  if (mode === "settings" && settingsSection === "updates") {
    animateSettingsPanel = true;
    animateSettingsTab = true;
  }
  if (mode === "settings" || mode === "home") render();
}

async function checkForUpdate(options: { automatic?: boolean } = {}): Promise<void> {
  if (updateBusy || !isTauriRuntime) return;
  updateBusy = true;
  if (!options.automatic) settingsError = null;
  updateInfo = updateInfo ?? {
    currentVersion: launcherVersion,
    availableVersion: null,
    publishedAt: null,
    notes: null,
    checkedAtUnix: null,
    status: "notChecked",
    detail: "Checking for updates…",
  };
  updateInfo = { ...updateInfo, detail: "Checking for updates…" };
  render();
  try {
    updateInfo = await invoke<UpdateInfo>("check_for_update");
  } catch (error) {
    const detail = error instanceof Error ? error.message : String(error);
    if (!options.automatic || mode === "settings") settingsError = detail;
    updateInfo = {
      ...updateInfo,
      status: "failed",
      detail,
    };
  } finally {
    updateBusy = false;
    if (mode === "settings" || mode === "home") render();
  }
}

async function installUpdate(): Promise<void> {
  if (updateBusy || !isTauriRuntime || updateInfo?.status !== "available") return;
  updateBusy = true;
  settingsError = null;
  updateInfo = { ...updateInfo, status: "installing", detail: "Update verified. RetroDrive will restart…" };
  renderSettings();
  try {
    await invoke("install_update");
  } catch (error) {
    settingsError = error instanceof Error ? error.message : String(error);
    updateInfo = { ...updateInfo, status: "failed", detail: settingsError };
    updateBusy = false;
    renderSettings();
  }
}

function openSettings(section: SettingsSection = "overview"): void {
  if (mode === "launching" || mode === "exit") return;
  mode = "settings";
  selected = "settings";
  pendingRestoreId = null;
  settingsSection = section;
  settingsError = null;
  animateScreenEnter = true;
  animateSettingsPanel = true;
  animateSettingsTab = true;
  renderSettings();
  if (section === "overview" && !driveOverview) void refreshDriveOverview();
  if (section === "updates") void loadUpdateStatus();
  if (section === "backup") void refreshBackupStatus();
}

function switchSettingsSection(section: SettingsSection): void {
  if (section === settingsSection) return;
  pendingRestoreId = null;
  settingsSection = section;
  settingsError = null;
  animateSettingsPanel = true;
  animateSettingsTab = true;
  renderSettings();
  if (section === "updates") void loadUpdateStatus();
  if (section === "backup") void refreshBackupStatus();
}

function moveHomeFocus(direction: number): void {
  if (mode !== "home" && mode !== "error") return;
  const items = homeItems();
  const index = Math.max(0, items.indexOf(selected));
  const next = Math.max(0, Math.min(items.length - 1, index + direction));
  const item = items[next];
  if (item && item !== selected) {
    selected = item;
    mode = "home";
    errorEntry = null;
    animateHomeSelection = true;
    renderHome();
  }
}

function moveSettingsSection(direction: number): void {
  if (mode !== "settings") return;
  const index = settingsSections.indexOf(settingsSection);
  const next = Math.max(0, Math.min(settingsSections.length - 1, index + direction));
  const section = settingsSections[next];
  if (section && section !== settingsSection) switchSettingsSection(section);
}

function confirmAction(): void {
  if (mode === "exit") {
    void exitLauncher();
  } else if (mode === "error" && errorEntry) {
    void launch(errorEntry);
  } else if (mode === "home") {
    if (selected === "settings") openSettings();
    else void launch(selected);
  } else if (mode === "settings") {
    if (settingsSection === "overview") void refreshDriveOverview();
    else if (settingsSection === "updates") void checkForUpdate();
    else if (settingsSection === "backup") {
      const control = focusedBackupControl();
      if (control) control.click();
      else moveBackupFocus(0, 1);
    }
  }
}

function secondaryAction(): void {
  if (mode !== "settings" || updateBusy || driveBusy) return;
  if (settingsSection === "updates") void installUpdate();
  else if (settingsSection === "overview") void refreshDriveOverview();
}

function backAction(): void {
  if (pendingRestoreId) {
    closeRestoreDialog();
  } else if (mode === "settings" || mode === "exit" || mode === "error") {
    const wasSettings = mode === "settings";
    mode = "home";
    selected = "retrobat";
    errorEntry = null;
    animateScreenEnter = true;
    settingsError = null;
    animateHomeSelection = wasSettings;
    renderHome();
  } else if (mode === "home") {
    mode = "exit";
    renderHome();
  }
}

async function exitLauncher(): Promise<void> {
  if (backupStatus?.busy) { openSettings("backup"); return; }
  if (isTauriRuntime) await invoke("close_launcher");
}

window.addEventListener("keydown", (event) => {
  if (event.repeat) return;
  setInputMethod("keyboard");
  if (event.target instanceof HTMLInputElement || event.target instanceof HTMLSelectElement || event.target instanceof HTMLTextAreaElement || (settingsSection === "backup" && event.target instanceof HTMLButtonElement && ["Enter", " "].includes(event.key))) return;
  if ((event.key === "s" || event.key === "S") && mode === "home") {
    openSettings();
    return;
  }
  if (mode === "launching") return;
  if (mode === "settings") {
    if (settingsSection === "backup" && focusedBackupControl()) {
      const horizontal = Number(event.key === "ArrowRight") - Number(event.key === "ArrowLeft");
      const vertical = Number(event.key === "ArrowDown") - Number(event.key === "ArrowUp");
      if (horizontal || vertical) {
        event.preventDefault();
        moveBackupFocus(horizontal, horizontal ? 0 : vertical);
        return;
      }
    }
    if (event.key === "ArrowLeft") moveSettingsSection(-1);
    if (event.key === "ArrowRight") moveSettingsSection(1);
    if (event.key === "Enter" || event.key === " ") confirmAction();
    if (settingsSection === "overview" && (event.key === "r" || event.key === "R")) void refreshDriveOverview();
    if (settingsSection === "updates" && (event.key === "c" || event.key === "C")) void checkForUpdate();
    if (settingsSection === "updates" && (event.key === "i" || event.key === "I")) void installUpdate();
    if (event.key === "Backspace" || event.key === "Escape") {
      if (pendingRestoreId) event.preventDefault();
      backAction();
    }
    return;
  }
  if (driveBusy || updateBusy) return;
  if (event.key === "ArrowLeft" || event.key === "ArrowUp") moveHomeFocus(-1);
  if (event.key === "ArrowRight" || event.key === "ArrowDown") moveHomeFocus(1);
  if (event.key === "Enter" || event.key === " ") confirmAction();
  if (event.key === "Backspace") backAction();
  if (event.key === "Escape") {
    if (mode === "home") {
      mode = "exit";
      renderHome();
    } else backAction();
  }
});

window.addEventListener("pointermove", () => setInputMethod("mouse"), { passive: true });
app.addEventListener("focusin", () => {
  if (mode === "settings" && settingsSection === "backup" && !pendingRestoreId) updateInputHints();
});
app.addEventListener("focusout", (event) => {
  if (!pendingRestoreId && event.target instanceof HTMLElement && event.target.matches("[data-backup-snapshot]") &&
      !(event.relatedTarget instanceof Node && app.contains(event.relatedTarget))) updateInputHints();
});

interface PadMemory {
  buttons: boolean[];
  horizontal: number;
  vertical: number;
  repeatAt: number;
}

const padMemory = new Map<number, PadMemory>();

function isPadNeutral(pad: Gamepad): boolean {
  return (
    pad.buttons.every((button) => !button.pressed) &&
    pad.axes.every((axis) => Math.abs(axis) < 0.35)
  );
}

function pollGamepads(now: number): void {
  const pads = navigator.getGamepads?.() ?? [];
  if (inputLockedUntilNeutral) {
    const connected = Array.from(pads).filter((pad): pad is Gamepad => Boolean(pad));
    if (connected.every(isPadNeutral)) inputLockedUntilNeutral = false;
    requestAnimationFrame(pollGamepads);
    return;
  }

  for (const pad of pads) {
    if (!pad) continue;
    const memory = padMemory.get(pad.index) ?? {
      buttons: [],
      horizontal: 0,
      vertical: 0,
      repeatAt: 0,
    };
    const horizontal =
      pad.buttons[14]?.pressed || (pad.axes[0] ?? 0) < -0.35
        ? -1
        : pad.buttons[15]?.pressed || (pad.axes[0] ?? 0) > 0.35
          ? 1
          : 0;
    const vertical =
      pad.buttons[12]?.pressed || (pad.axes[1] ?? 0) < -0.35
        ? -1
        : pad.buttons[13]?.pressed || (pad.axes[1] ?? 0) > 0.35
          ? 1
          : 0;
    if (horizontal || vertical || pad.buttons.some((button) => button.pressed)) {
      setInputMethod("gamepad");
    }
    const directionChanged = horizontal !== memory.horizontal || vertical !== memory.vertical;
    if (
      (mode === "home" || mode === "error") &&
      horizontal !== 0 &&
      (horizontal !== memory.horizontal || now >= memory.repeatAt)
    ) {
      moveHomeFocus(horizontal);
      memory.repeatAt = now + (horizontal === memory.horizontal ? 100 : 350);
    }
    if (mode === "settings" && settingsSection === "backup" && (horizontal || vertical) &&
        (directionChanged || now >= memory.repeatAt)) {
      moveBackupFocus(vertical ? 0 : horizontal, vertical);
      memory.repeatAt = now + (directionChanged ? 350 : 100);
    }
    const pressedEdge = (index: number) =>
      Boolean(pad.buttons[index]?.pressed) && !Boolean(memory.buttons[index]);
    const shoulderDirection = pressedEdge(4) ? -1 : pressedEdge(5) ? 1 : 0;
    if (mode === "settings" && !pendingRestoreId && shoulderDirection !== 0) {
      moveSettingsSection(shoulderDirection);
    }
    if (mode === "settings") {
      if (pressedEdge(0)) confirmAction();
      if (pressedEdge(2)) secondaryAction();
      if (pressedEdge(1)) backAction();
    } else {
      if (pressedEdge(0)) confirmAction();
      if (pressedEdge(1)) backAction();
    }
    memory.buttons = pad.buttons.map((button) => button.pressed);
    memory.horizontal = horizontal;
    memory.vertical = vertical;
    padMemory.set(pad.index, memory);
  }
  requestAnimationFrame(pollGamepads);
}

function render(): void {
  if (mode === "settings") renderSettings();
  else renderHome();
}

async function boot(): Promise<void> {
  if (!isTauriRuntime) {
    entries = [
      { id: "retrobat", title: "RetroBat", primary: true },
      { id: "retroarch", title: "RetroArch", primary: false },
    ];
    selected = "retrobat";
    if (showSampleData) {
      driveOverview = sampleDriveOverview();
      backupStatus = sampleBackupStatus();
      openSettings(samplePage === "backup" ? "backup" : "overview");
    } else {
      renderHome();
    }
    requestAnimationFrame(pollGamepads);
    return;
  }

  try {
    const [state, loadedEntries] = await Promise.all([
      invoke<LauncherState>("get_launcher_state"),
      invoke<LauncherEntry[]>("list_entries"),
    ]);
    launcherVersion = state.version;
    entries = loadedEntries;
    selected = state.defaultEntry;
    await listen<ChildEndedEvent>("child-session-ended", () => {
      mode = "home";
      errorEntry = null;
      inputLockedUntilNeutral = true;
      renderHome();
    });
    await listen("backup-close-blocked", () => openSettings("backup"));
    renderHome();
    requestAnimationFrame(pollGamepads);
    void refreshDriveOverview();
    void checkForUpdate({ automatic: true });
  } catch {
    entries = [
      { id: "retrobat", title: "RetroBat", primary: true },
      { id: "retroarch", title: "RetroArch", primary: false },
    ];
    selected = "retrobat";
    mode = "error";
    errorEntry = "retrobat";
    renderHome();
  }
}

void boot();
