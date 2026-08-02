import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./styles.css";
import type {
  ChildEndedEvent,
  EntryId,
  LauncherEntry,
  LauncherState,
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
let selected: EntryId = "retrobat";
let mode: UiMode = "home";
let errorEntry: EntryId | null = null;
let inputLockedUntilNeutral = false;

const entryTitle = (id: EntryId) =>
  entries.find((entry) => entry.id === id)?.title ??
  (id === "retrobat" ? "RetroBat" : "RetroArch");

function iconPath(id: EntryId): string {
  return `/assets/icons/${id}.png`;
}

function render(): void {
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
            ? `<span><kbd>A</kbd> Retry</span><span><kbd>B</kbd> Back</span>`
            : `<span><kbd>A</kbd> Open</span><span><kbd>Start</kbd> RetroBat</span><span><kbd>B</kbd> Exit</span>`
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
  }
}

function backAction(): void {
  if (mode === "exit" || mode === "error") {
    mode = "home";
    errorEntry = null;
    render();
  } else if (mode === "home") {
    mode = "exit";
    render();
  }
}

window.addEventListener("keydown", (event) => {
  if (event.repeat || mode === "launching") return;
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
      repeatAt: 0,
    };
    const horizontal =
      pad.buttons[14]?.pressed || (pad.axes[0] ?? 0) < -0.35
        ? -1
        : pad.buttons[15]?.pressed || (pad.axes[0] ?? 0) > 0.35
          ? 1
          : 0;
    if (
      horizontal !== 0 &&
      (horizontal !== memory.horizontal || now >= memory.repeatAt)
    ) {
      moveFocus(horizontal);
      memory.repeatAt = now + (horizontal === memory.horizontal ? 100 : 350);
    }
    const pressedEdge = (index: number) =>
      Boolean(pad.buttons[index]?.pressed) && !Boolean(memory.buttons[index]);
    if (pressedEdge(0)) confirmAction();
    if (pressedEdge(1)) backAction();
    if (pressedEdge(9) && mode === "home") void launch("retrobat");
    memory.buttons = pad.buttons.map((button) => button.pressed);
    memory.horizontal = horizontal;
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
