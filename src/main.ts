import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./styles.css";
import type {
  ChildEndedEvent,
  EntryId,
  LauncherEntry,
  LauncherState,
  SkillStatus,
  SkillSummary,
  UiMode,
} from "./types";

function requireApplicationRoot(): HTMLDivElement {
  const root = document.querySelector<HTMLDivElement>("#app");
  if (!root) throw new Error("Application root is missing");
  return root;
}

const app = requireApplicationRoot();
const isTauriRuntime = "__TAURI_INTERNALS__" in window;

let entries: LauncherEntry[] = [];
let skills: SkillSummary[] = [];
let selected: EntryId = "retrobat";
let selectedSkillIndex = 0;
let mode: UiMode = "home";
let errorEntry: EntryId | null = null;
let skillError: string | null = null;
let skillBusy = false;
let inputLockedUntilNeutral = false;

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

function skillStatusLabel(status: SkillStatus): string {
  const labels: Record<SkillStatus, string> = {
    notInstalled: "Not installed",
    upToDate: "Up to date",
    updateAvailable: "Update available",
    localChanges: "Local changes",
    error: "Unavailable",
  };
  return labels[status];
}

function renderSkills(): void {
  const current = skills[selectedSkillIndex];
  const title = current ? escapeHtml(current.displayName) : "Skills";
  const status = current ? skillStatusLabel(current.status) : "No skills available";
  const description = current
    ? escapeHtml(current.description)
    : "Install a signed RetroDrive Skill release to make it available here.";
  const message = skillError ? `<p class="skill-error">${escapeHtml(skillError)}</p>` : "";

  app.innerHTML = `
    <main class="shell" aria-label="RetroDrive skills">
      <header class="brand">RetroDrive <span class="brand-secondary">Skills</span></header>
      <section class="skills-panel" aria-label="Skills">
        <div class="skill-list">
          ${skills.length === 0 ? `<p class="empty-state">No bundled Skills found.</p>` : skills
            .map((skill, index) => `
              <button class="skill-row ${index === selectedSkillIndex ? "is-selected" : ""}"
                data-skill-index="${index}" type="button" ${skillBusy ? "disabled" : ""}>
                <span class="skill-row-title">${escapeHtml(skill.displayName)}</span>
                <span class="skill-row-status status-${skill.status}">${skillStatusLabel(skill.status)}</span>
              </button>`)
            .join("")}
        </div>
        <aside class="skill-detail">
          <div class="skill-detail-heading">${title}</div>
          <div class="skill-detail-status">${status}</div>
          <p>${description}</p>
          ${message}
        </aside>
      </section>
      <footer class="hints ${skillBusy ? "is-muted" : ""}">
        <span><kbd>A</kbd> Sync</span><span><kbd>X</kbd> Sync all</span><span><kbd>B</kbd> Back</span>
      </footer>
    </main>`;

  document.querySelectorAll<HTMLButtonElement>("[data-skill-index]").forEach((row) => {
    row.addEventListener("mouseenter", () => {
      if (!skillBusy) {
        selectedSkillIndex = Number(row.dataset.skillIndex);
        renderSkills();
      }
    });
    row.addEventListener("click", () => void syncSelectedSkill());
  });
}

function render(): void {
  if (mode === "skills") {
    renderSkills();
    return;
  }
  const launchCopy = mode === "launching" ? `Starting ${entryTitle(selected)}…` : "";
  const errorCopy =
    mode === "error" && errorEntry
      ? `Unable to start ${entryTitle(errorEntry)}`
      : "";

  app.innerHTML = `
    <main class="shell" aria-label="RetroDrive launcher">
      <header class="brand">RetroDrive</header>
      <section class="tiles" aria-label="Applications">
        ${entries
          .map((entry) => {
            const focused = selected === entry.id;
            const stateClass = focused ? "is-focused" : "is-idle";
            const launching = focused && mode === "launching";
            const failed = focused && mode === "error";
            return `
              <button
                class="launch-tile ${stateClass} ${launching ? "is-launching" : ""} ${failed ? "is-error" : ""}"
                data-entry="${entry.id}"
                type="button"
                aria-label="${entry.title}"
                aria-current="${focused ? "true" : "false"}"
                ${mode === "launching" || mode === "exit" ? "disabled" : ""}
              >
                <span class="icon-box"><img src="${iconPath(entry.id)}" alt="" draggable="false" /></span>
                <span class="tile-copy">
                  <span class="tile-title">${entry.title}</span>
                  ${launching ? `<span class="transient">${launchCopy}</span><span class="progress" aria-hidden="true"></span>` : ""}
                  ${failed ? `<span class="transient error-copy">${errorCopy}</span>` : ""}
                </span>
              </button>`;
          })
          .join("")}
      </section>
      ${
        mode === "exit"
          ? `<section class="dialog" role="dialog" aria-modal="true" aria-label="Exit RetroDrive">
               <p>Exit RetroDrive?</p>
               <div><span><kbd>A</kbd> Exit</span><span><kbd>B</kbd> Cancel</span></div>
             </section>`
          : ""
      }
      <footer class="hints ${mode === "launching" ? "is-muted" : ""}">
        ${
          mode === "error"
            ? `<span><kbd>A</kbd> Retry</span><span><kbd>B</kbd> Back</span><span><kbd>Y</kbd> Skills</span>`
            : `<span><kbd>A</kbd> Open</span><span><kbd>B</kbd> Exit</span><span><kbd>Y</kbd> Skills</span>`
        }
      </footer>
    </main>`;

  document.querySelectorAll<HTMLButtonElement>("[data-entry]").forEach((tile) => {
    tile.addEventListener("mouseenter", () => {
      if (mode === "home" || mode === "error") {
        selected = tile.dataset.entry as EntryId;
        mode = "home";
        render();
      }
    });
    tile.addEventListener("click", () => void launch(tile.dataset.entry as EntryId));
  });
}

function moveFocus(direction: number): void {
  if (mode !== "home" && mode !== "error") return;
  const index = entries.findIndex((entry) => entry.id === selected);
  const next = Math.max(0, Math.min(entries.length - 1, index + direction));
  const entry = entries[next];
  if (entry && entry.id !== selected) {
    selected = entry.id;
    mode = "home";
    errorEntry = null;
    render();
  }
}

function moveSkillFocus(direction: number): void {
  if (mode !== "skills" || skills.length === 0) return;
  selectedSkillIndex = Math.max(0, Math.min(skills.length - 1, selectedSkillIndex + direction));
  renderSkills();
}

async function launch(id: EntryId): Promise<void> {
  if (mode === "launching") return;
  selected = id;
  mode = "launching";
  errorEntry = null;
  render();
  try {
    await invoke("launch_entry", { entryId: id });
  } catch {
    mode = "error";
    errorEntry = id;
    render();
  }
}

async function refreshSkills(): Promise<void> {
  if (!isTauriRuntime) {
    skills = [];
    return;
  }
  skills = await invoke<SkillSummary[]>("list_skills");
  selectedSkillIndex = Math.min(selectedSkillIndex, Math.max(0, skills.length - 1));
}

async function openSkills(): Promise<void> {
  if (mode === "launching" || mode === "exit") return;
  skillError = null;
  try {
    await refreshSkills();
  } catch (error) {
    skills = [];
    skillError = error instanceof Error ? error.message : String(error);
  }
  mode = "skills";
  render();
}

async function syncSelectedSkill(): Promise<void> {
  const skill = skills[selectedSkillIndex];
  if (!skill || skillBusy || !isTauriRuntime) return;
  skillBusy = true;
  skillError = null;
  renderSkills();
  try {
    await invoke<SkillSummary>("sync_skill", { skillId: skill.id });
    await refreshSkills();
  } catch (error) {
    skillError = error instanceof Error ? error.message : String(error);
  } finally {
    skillBusy = false;
    renderSkills();
  }
}

async function syncAllSkills(): Promise<void> {
  if (skillBusy || !isTauriRuntime || skills.length === 0) return;
  skillBusy = true;
  skillError = null;
  renderSkills();
  try {
    await invoke<SkillSummary[]>("sync_all_skills");
    await refreshSkills();
  } catch (error) {
    skillError = error instanceof Error ? error.message : String(error);
  } finally {
    skillBusy = false;
    renderSkills();
  }
}

async function exitLauncher(): Promise<void> {
  await invoke("close_launcher");
}

function confirmAction(): void {
  if (mode === "exit") {
    void exitLauncher();
  } else if (mode === "error") {
    void launch(selected);
  } else if (mode === "home") {
    void launch(selected);
  } else if (mode === "skills") {
    void syncSelectedSkill();
  }
}

function backAction(): void {
  if (mode === "exit" || mode === "error" || mode === "skills") {
    mode = "home";
    errorEntry = null;
    skillError = null;
    render();
  } else if (mode === "home") {
    mode = "exit";
    render();
  }
}

window.addEventListener("keydown", (event) => {
  if (event.repeat) return;
  if ((event.key === "y" || event.key === "Y") && (mode === "home" || mode === "error")) {
    void openSkills();
    return;
  }
  if ((event.key === "x" || event.key === "X") && mode === "skills") {
    void syncAllSkills();
    return;
  }
  if (mode === "launching" || skillBusy) return;
  if (mode === "skills") {
    if (event.key === "ArrowLeft" || event.key === "ArrowUp") moveSkillFocus(-1);
    if (event.key === "ArrowRight" || event.key === "ArrowDown") moveSkillFocus(1);
    if (event.key === "Enter" || event.key === " ") confirmAction();
    if (event.key === "Backspace" || event.key === "Escape") backAction();
    return;
  }
  if (event.key === "ArrowLeft" || event.key === "ArrowUp") moveFocus(-1);
  if (event.key === "ArrowRight" || event.key === "ArrowDown") moveFocus(1);
  if (event.key === "Enter" || event.key === " ") confirmAction();
  if (event.key === "Backspace") backAction();
  if (event.key === "Escape") {
    if (mode === "home") {
      mode = "exit";
      render();
    } else backAction();
  }
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
    if (
      mode !== "skills" &&
      horizontal !== 0 &&
      (horizontal !== memory.horizontal || now >= memory.repeatAt)
    ) {
      moveFocus(horizontal);
      memory.repeatAt = now + (horizontal === memory.horizontal ? 100 : 350);
    }
    if (
      mode === "skills" &&
      vertical !== 0 &&
      (vertical !== memory.vertical || now >= memory.repeatAt)
    ) {
      moveSkillFocus(vertical);
      memory.repeatAt = now + (vertical === memory.vertical ? 100 : 350);
    }
    const pressedEdge = (index: number) =>
      Boolean(pad.buttons[index]?.pressed) && !Boolean(memory.buttons[index]);
    if (pressedEdge(3) && (mode === "home" || mode === "error")) void openSkills();
    else if (mode === "skills") {
      if (pressedEdge(0)) void syncSelectedSkill();
      if (pressedEdge(2)) void syncAllSkills();
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

async function boot(): Promise<void> {
  if (!isTauriRuntime) {
    entries = [
      { id: "retrobat", title: "RetroBat", primary: true },
      { id: "retroarch", title: "RetroArch", primary: false },
    ];
    selected = "retrobat";
    render();
    requestAnimationFrame(pollGamepads);
    return;
  }

  try {
    const [state, loadedEntries] = await Promise.all([
      invoke<LauncherState>("get_launcher_state"),
      invoke<LauncherEntry[]>("list_entries"),
    ]);
    entries = loadedEntries;
    selected = state.defaultEntry;
    await refreshSkills().catch(() => {
      skills = [];
    });
    await listen<ChildEndedEvent>("child-session-ended", () => {
      mode = "home";
      errorEntry = null;
      inputLockedUntilNeutral = true;
      render();
    });
    render();
    requestAnimationFrame(pollGamepads);
  } catch {
    entries = [
      { id: "retrobat", title: "RetroBat", primary: true },
      { id: "retroarch", title: "RetroArch", primary: false },
    ];
    selected = "retrobat";
    mode = "error";
    errorEntry = "retrobat";
    render();
  }
}

void boot();
