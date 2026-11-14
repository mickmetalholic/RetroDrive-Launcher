import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./styles.css";
import type {
  ChildEndedEvent,
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

let entries: LauncherEntry[] = [];
let selected: HomeSelection = "retrobat";
let mode: UiMode = "home";
let settingsSection: SettingsSection = "overview";
const sectionTitles: Record<SettingsSection, string> = {
  overview: "Storage",
  updates: "Updates",
};
const settingsSections = Object.keys(sectionTitles) as SettingsSection[];
let errorEntry: EntryId | null = null;
let settingsError: string | null = null;
let driveBusy = false;
let driveOverview: DriveOverview | null = null;
let updateBusy = false;
let updateInfo: UpdateInfo | null = null;
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

function formatName(value: string): string {
  return value
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
    return `${hint("CLICK", "Launch")}${hint("ESC", "Exit")}`;
  }
  return `${hint("↑ ↓", "Navigate")}${hint("ENTER", "Launch")}${hint("ESC", "Exit")}`;
}

function settingsHints(): string {
  const tabHint =
    inputMethod === "gamepad"
      ? hint("LB / RB", "Switch tabs")
      : inputMethod === "mouse"
        ? hint("CLICK", "Switch tabs")
        : hint("← →", "Switch tabs");
  const backHint = inputMethod === "gamepad" ? hint("B", "Back") : hint("ESC", "Back");
  let actionHint = "";
  if (settingsSection === "overview") {
    actionHint = inputMethod === "gamepad" ? hint("A", "Refresh") : inputMethod === "mouse" ? hint("CLICK", "Refresh") : hint("R", "Refresh");
  } else if (settingsSection === "updates") {
    actionHint = inputMethod === "gamepad" ? hint("A", "Check") : inputMethod === "mouse" ? hint("CLICK", "Check") : hint("C", "Check");
    if (updateInfo?.status === "available") {
      actionHint += inputMethod === "gamepad" ? hint("X", "Install") : inputMethod === "mouse" ? hint("CLICK", "Install") : hint("I", "Install");
    }
  }
  return `${tabHint}${actionHint}${backHint}`;
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
  if (hints) {
    const content =
      mode === "settings"
        ? settingsHints()
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
  const diskSummary = driveOverview
    ? `${formatBytes(driveOverview.freeBytes)} free · ${driveOverview.gameFiles} games`
    : driveBusy
      ? "Scanning drive…"
      : "Open Storage for details";

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
          <div class="summary-label">${escapeHtml(driveOverview?.drive ?? "D:")} Drive</div>
          <div class="summary-value">${driveOverview ? escapeHtml(formatBytes(driveOverview.usedBytes)) : "—"}</div>
          <div class="summary-note">${escapeHtml(diskSummary)}</div>
        </article>
        <article class="summary-card">
          <div class="summary-label">Library</div>
          <div class="summary-value">${driveOverview ? driveOverview.gameFiles : "—"}</div>
          <div class="summary-note">${driveOverview ? `${driveOverview.systemsWithGames} platforms · ${formatBytes(driveOverview.gameBytes)}` : "Scanning in background"}</div>
        </article>
        <article class="summary-card">
          <div class="summary-label">Emulators</div>
          <div class="summary-value">${driveOverview ? driveOverview.emulatorCount : "—"}</div>
          <div class="summary-note">${driveOverview ? `RetroArch · ${driveOverview.retroArchCoreCount} cores` : "Open Storage for details"}</div>
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
  const tabIndex = settingsSections.indexOf(settingsSection);
  const sectionNumber = String(tabIndex + 1).padStart(2, "0");
  const nav = settingsSections
    .map(
      (section) =>
        `<button class="settings-nav ${settingsSection === section ? "is-active" : ""}" data-settings-section="${section}" type="button" aria-current="${settingsSection === section ? "page" : "false"}"><span class="tab-label">${sectionTitles[section]}</span></button>`,
    )
    .join("");
  const content = settingsSection === "overview" ? renderOverviewContent() : renderUpdateContent();

  app.innerHTML = `
    <main class="shell settings-shell ${animateScreenEnter ? "is-screen-entering" : ""}" aria-label="RetroDrive settings">
      <header class="topbar">
        <div class="brand">RetroDrive</div>
      </header>
      <nav class="settings-nav-bar ${animateSettingsTab ? "is-tab-changing" : ""}" data-active="${settingsSection}" style="--tab-index:${tabIndex}; --tab-count:${settingsSections.length}" aria-label="Settings sections">${nav}</nav>
      <section class="settings-content ${animateSettingsPanel ? "is-section-entering" : ""}" data-section="${settingsSection}">
        <div class="settings-heading">
          <h1>${sectionTitles[settingsSection]}</h1>
          <span class="section-index">${sectionNumber} / ${String(settingsSections.length).padStart(2, "0")}</span>
        </div>
        ${settingsError ? `<p class="settings-error">${escapeHtml(settingsError)}</p>` : ""}
        ${content}
      </section>
      <footer class="hints ${driveBusy || updateBusy ? "is-muted" : ""}">
        ${settingsHints()}<span class="footer-version">${updateIndicator()}RetroDrive v${escapeHtml(launcherVersion)}</span>
      </footer>
    </main>`;

  animateSettingsPanel = false;
  animateSettingsTab = false;
  animateScreenEnter = false;

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
}

function renderOverviewContent(): string {
  if (driveBusy && !driveOverview) {
    return `<div class="loading-panel"><span class="spinner"></span><strong>Scanning drive…</strong><p>Reading emulators, platforms and save data.</p></div>`;
  }
  if (!driveOverview) {
    return `<div class="loading-panel"><strong>No drive snapshot</strong><p>Refresh to read the current RetroDrive volume.</p><button class="primary-button" data-refresh-drive type="button">Refresh drive</button></div>`;
  }
  const usedPercent = driveOverview.totalBytes
    ? Math.min(100, Math.max(0, (driveOverview.usedBytes / driveOverview.totalBytes) * 100))
    : 0;
  const platformRows = driveOverview.platforms
    .slice(0, 12)
    .map((platform) => {
      const max = driveOverview?.platforms[0]?.gameFiles || 1;
      const width = Math.max(4, (platform.gameFiles / max) * 100);
      return `<div class="platform-row"><div><span>${escapeHtml(formatName(platform.name))}</span><span>${platform.gameFiles}</span></div><div class="platform-bar"><span style="width:${width}%"></span></div></div>`;
    })
    .join("");
  const emulatorChips = driveOverview.emulators
    .slice(0, 28)
    .map((emulator) => `<span class="emulator-chip" title="${escapeHtml(emulator.path)}">${escapeHtml(formatName(emulator.name))}</span>`)
    .join("");
  const warnings = driveOverview.warnings.length
    ? `<div class="scan-warnings"><strong>Scan notes</strong><span>${escapeHtml(driveOverview.warnings.slice(0, 3).join(" · "))}</span></div>`
    : "";

  return `
    <div class="overview-grid">
      <article class="setting-card storage-card">
        <div class="card-topline"><span class="card-kicker">${escapeHtml(driveOverview.drive)} / Capacity</span><span class="card-icon">◉</span></div>
        <div class="large-number">${escapeHtml(formatBytes(driveOverview.usedBytes))}<small> used</small></div>
        <div class="storage-meter"><span style="width:${usedPercent.toFixed(1)}%"></span></div>
        <div class="card-meta"><span>${escapeHtml(formatBytes(driveOverview.freeBytes))} free</span><span>${escapeHtml(formatBytes(driveOverview.totalBytes))} total</span></div>
        <div class="stat-pairs"><div><span>Root</span><strong>${escapeHtml(driveOverview.root)}</strong></div><div><span>Scanned</span><strong>${escapeHtml(formatDate(driveOverview.scannedAtUnix))}</strong></div></div>
      </article>
      <article class="setting-card library-card">
        <div class="card-topline"><span class="card-kicker">Library</span><span class="card-icon">▦</span></div>
        <div class="large-number">${driveOverview.gameFiles}<small> games</small></div>
        <div class="card-meta"><span>${driveOverview.systemsWithGames} platforms</span><span>${escapeHtml(formatBytes(driveOverview.gameBytes))}</span></div>
        <div class="stat-pairs"><div><span>System folders</span><strong>${driveOverview.platformCount}</strong></div><div><span>Saves / states</span><strong>${escapeHtml(formatBytes(driveOverview.saveBytes))}</strong></div></div>
      </article>
      <article class="setting-card emulator-card">
        <div class="card-topline"><span class="card-kicker">Emulators</span><span class="card-icon">✦</span></div>
        <div class="large-number">${driveOverview.emulatorCount}<small> found</small></div>
        <div class="card-meta"><span>RetroBat folders</span><span>${driveOverview.retroArchCoreCount} cores</span></div>
        <div class="stat-pairs"><div><span>RetroBat</span><strong>${driveOverview.emulators.some((emulator) => emulator.name.toLowerCase() === "retroarch") ? "Detected" : "Installed"}</strong></div><div><span>ROM root</span><strong>${escapeHtml(driveOverview.romRoot)}</strong></div></div>
      </article>
    </div>
    <div class="overview-lower">
      <article class="setting-card list-card"><div class="card-heading"><div><span class="card-kicker">Library mix</span><h2>Games by platform</h2></div><span class="muted">Top 12</span></div><div class="platform-list">${platformRows || `<p class="empty-state">No game files detected.</p>`}</div></article>
      <article class="setting-card list-card"><div class="card-heading"><div><span class="card-kicker">Runtime</span><h2>Detected emulators</h2></div><span class="muted">${driveOverview.emulatorCount}</span></div><div class="emulator-list">${emulatorChips || `<p class="empty-state">No RetroBat emulator folders found.</p>`}</div>${driveOverview.emulators.length > 28 ? `<p class="list-footnote">${driveOverview.emulators.length - 28} more folders. Hover for paths.</p>` : ""}</article>
    </div>
    ${warnings}
    <div class="settings-actions"><button class="secondary-button" data-refresh-drive type="button" ${driveBusy ? "disabled" : ""}>${driveBusy ? "Scanning…" : "Rescan drive"}</button><span>Read-only scan</span></div>`;
}

function renderUpdateContent(): string {
  const info = updateInfo;
  const available = info?.status === "available" && info.availableVersion;
  const notes = info?.notes ? `<div class="release-notes"><span class="card-kicker">Release notes</span><p>${escapeHtml(info.notes)}</p></div>` : "";
  return `
    <div class="update-layout">
      <article class="setting-card update-card ${available ? "has-update" : ""}">
        <div class="card-topline"><span class="card-kicker">Launcher version</span><span class="card-icon">↗</span></div>
        <div class="version-line"><span>v${escapeHtml(info?.currentVersion ?? launcherVersion)}</span>${available ? `<b>→</b><strong>v${escapeHtml(info.availableVersion ?? "")}</strong>` : ""}</div>
        <div class="update-status status-${info?.status ?? "notChecked"}">${escapeHtml(info ? updateStatusLabel(info.status) : "Not checked")}</div>
        <p class="update-detail">${escapeHtml(info?.detail ?? "Checking for signed releases…")}</p>
        <div class="update-actions"><button class="secondary-button" data-check-update type="button" ${updateBusy ? "disabled" : ""}>${updateBusy ? "Checking…" : "Check for updates"}</button>${available ? `<button class="primary-button" data-install-update type="button" ${updateBusy ? "disabled" : ""}>Install update</button>` : ""}</div>
        <div class="update-meta"><span>Last checked: ${escapeHtml(formatDate(info?.checkedAtUnix))}</span>${info?.publishedAt ? `<span>Published: ${escapeHtml(formatDate(Date.parse(info.publishedAt) / 1000))}</span>` : ""}</div>
      </article>
      <aside class="setting-card update-guide"><span class="card-kicker">Update channel</span><h2>Automatic checks</h2><p>A background check runs after launch. You can always check again here before installing.</p><div class="check-list"><span>✓ Signed manifest</span><span>✓ SHA-256 package</span><span>✓ Rollback copy</span></div></aside>
    </div>
    ${notes}`;
}

async function launch(id: EntryId): Promise<void> {
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
  settingsSection = section;
  settingsError = null;
  animateScreenEnter = true;
  animateSettingsPanel = true;
  animateSettingsTab = true;
  renderSettings();
  if (section === "overview" && !driveOverview) void refreshDriveOverview();
  if (section === "updates") void loadUpdateStatus();
}

function switchSettingsSection(section: SettingsSection): void {
  if (section === settingsSection) return;
  settingsSection = section;
  settingsError = null;
  animateSettingsPanel = true;
  animateSettingsTab = true;
  renderSettings();
  if (section === "updates") void loadUpdateStatus();
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
  }
}

function secondaryAction(): void {
  if (mode !== "settings" || updateBusy || driveBusy) return;
  if (settingsSection === "updates") void installUpdate();
  else if (settingsSection === "overview") void refreshDriveOverview();
}

function backAction(): void {
  if (mode === "settings" || mode === "exit" || mode === "error") {
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
  if (isTauriRuntime) await invoke("close_launcher");
}

window.addEventListener("keydown", (event) => {
  if (event.repeat) return;
  setInputMethod("keyboard");
  if ((event.key === "s" || event.key === "S") && mode === "home") {
    openSettings();
    return;
  }
  if (mode === "launching") return;
  if (mode === "settings") {
    if (event.key === "ArrowLeft") moveSettingsSection(-1);
    if (event.key === "ArrowRight") moveSettingsSection(1);
    if (event.key === "Enter" || event.key === " ") confirmAction();
    if (settingsSection === "overview" && (event.key === "r" || event.key === "R")) void refreshDriveOverview();
    if (settingsSection === "updates" && (event.key === "c" || event.key === "C")) void checkForUpdate();
    if (settingsSection === "updates" && (event.key === "i" || event.key === "I")) void installUpdate();
    if (event.key === "Backspace" || event.key === "Escape") backAction();
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

interface PadMemory {
  buttons: boolean[];
  horizontal: number;
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
    setInputMethod("gamepad");
    const memory = padMemory.get(pad.index) ?? {
      buttons: [],
      horizontal: 0,
      repeatAt: 0,
    };
    const horizontal =
      pad.buttons[14]?.pressed || (pad.axes[0] ?? 0) < -0.35
        ? -1
        : pad.buttons[15]?.pressed || (pad.axes[0] ?? 0) > 0.35
          ? 1
          : 0;
    if (
      (mode === "home" || mode === "error") &&
      horizontal !== 0 &&
      (horizontal !== memory.horizontal || now >= memory.repeatAt)
    ) {
      moveHomeFocus(horizontal);
      memory.repeatAt = now + (horizontal === memory.horizontal ? 100 : 350);
    }
    const pressedEdge = (index: number) =>
      Boolean(pad.buttons[index]?.pressed) && !Boolean(memory.buttons[index]);
    const shoulderDirection = pressedEdge(4) ? -1 : pressedEdge(5) ? 1 : 0;
    if (mode === "settings" && shoulderDirection !== 0) {
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
    renderHome();
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
