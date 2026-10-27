// App-wide state: the open vault, navigation, tasks and Jira data.
import { api, type CompleteUndo, type JiraCache, type Task, type TreeNode, type VaultInfo } from "./api";

export type View =
  | { kind: "today" }
  | { kind: "note"; path: string; line?: number }
  | { kind: "tasks" }
  | { kind: "jira" }
  | { kind: "schedules" }
  | { kind: "settings" }
  | { kind: "setup" };

export type SidebarMode = "files" | "search" | "people";

export interface ToastAction {
  label: string;
  run: () => void;
}

export interface Toast {
  id: number;
  text: string;
  kind: "info" | "error";
  action?: ToastAction;
}

let toastSeq = 0;
let toastTimer: ReturnType<typeof setTimeout> | undefined;

class AppState {
  vault = $state<VaultInfo | null>(null);
  tree = $state<TreeNode | null>(null);
  notes = $state<string[]>([]);
  tasks = $state<Task[]>([]);
  jira = $state<JiraCache | null>(null);
  view = $state<View>({ kind: "today" });
  history: View[] = [];
  sidebar = $state<SidebarMode>("files");
  agentOpen = $state(true);
  paletteOpen = $state(false);
  toast = $state<Toast | null>(null);
  /** Bumped when files change on disk, so open views can reload. */
  revision = $state(0);

  openTasks = $derived(this.tasks.filter((t) => !t.done));

  async openVault(path: string) {
    this.vault = await api.vaultOpen(path);
    this.view = { kind: "today" };
    await this.refresh();
  }

  async refresh() {
    const [tree, notes, tasks, jira] = await Promise.all([
      api.vaultTree(),
      api.notesList(),
      api.tasksList(),
      api.jiraCache().catch(() => null),
    ]);
    this.tree = tree;
    this.notes = notes;
    this.tasks = tasks;
    this.jira = jira;
  }

  async refreshTasks() {
    this.tasks = await api.tasksList();
  }

  go(view: View) {
    this.history.push($state.snapshot(this.view) as View);
    if (this.history.length > 50) this.history.shift();
    this.view = view;
  }

  back() {
    const prev = this.history.pop();
    if (prev) this.view = prev;
  }

  openNote(path: string, line?: number) {
    this.go({ kind: "note", path, line });
  }

  /** Resolves a [[wikilink]] target to a note path, like the backend does. */
  resolveLink(target: string): string | null {
    const t = target.trim().replace(/\.md$/, "");
    const exact = this.notes.find((n) => n.replace(/\.md$/, "") === t);
    if (exact) return exact;
    const byStem = this.notes
      .filter((n) => n.replace(/\.md$/, "").split("/").pop() === t.split("/").pop())
      .sort((a, b) => a.length - b.length);
    return byStem[0] ?? null;
  }

  async followLink(target: string) {
    const path = this.resolveLink(target);
    if (path) return this.openNote(path);
    const created = await api.noteCreate(target.endsWith(".md") ? target : `${target}.md`);
    await this.refresh();
    this.openNote(created);
  }

  notify(text: string, kind: "info" | "error" = "info", action?: ToastAction) {
    this.toast = { id: ++toastSeq, text, kind, action };
    this.releaseToast();
  }

  /** Keeps the toast up (while the pointer is on it). */
  holdToast() {
    clearTimeout(toastTimer);
  }

  /** (Re)starts the toast's dismiss timer. */
  releaseToast() {
    clearTimeout(toastTimer);
    const t = this.toast;
    if (!t) return;
    toastTimer = setTimeout(() => {
      if (this.toast?.id === t.id) this.toast = null;
    }, t.action ? 10000 : t.kind === "error" ? 8000 : 3500);
  }

  async completeTask(task: Task) {
    try {
      const res = await api.taskComplete(task);
      this.tasks = this.tasks.filter((t) => !(t.file === task.file && t.line === task.line));
      this.notify(res.archivedTo ? `Done. Moved to ${res.archivedTo}` : "Done", "info", {
        label: "Undo",
        run: () => this.undoComplete(res.undo),
      });
      await this.refreshTasks();
      this.revision++;
    } catch (e) {
      this.notify(String(e), "error");
      await this.refreshTasks();
    }
  }

  async undoComplete(undo: CompleteUndo) {
    try {
      await api.taskUndoComplete(undo);
      this.notify("Task restored");
    } catch (e) {
      this.notify(`Couldn't undo: ${e}`, "error");
    }
    await this.refreshTasks();
    this.revision++;
  }
}

export const app = new AppState();

export function noteTitle(path: string) {
  return path.split("/").pop()!;
}

export function personName(link: string) {
  const stem = link.split("/").pop() ?? link;
  return stem.charAt(0).toUpperCase() + stem.slice(1).replace(/[-_]/g, " ");
}

export function daysFromToday(date: string): number {
  const d = new Date(date + "T00:00:00");
  const t = new Date();
  t.setHours(0, 0, 0, 0);
  return Math.round((d.getTime() - t.getTime()) / 86400000);
}

export function dueLabel(date: string): string {
  const n = daysFromToday(date);
  if (n === 0) return "today";
  if (n === 1) return "tomorrow";
  if (n === -1) return "yesterday";
  if (n < 0) return `${-n} days late`;
  if (n < 7) return new Date(date + "T00:00:00").toLocaleDateString(undefined, { weekday: "long" });
  return new Date(date + "T00:00:00").toLocaleDateString(undefined, { month: "short", day: "numeric" });
}

export function relTime(isoDate: string): string {
  const diff = (Date.now() - new Date(isoDate).getTime()) / 1000;
  const fut = diff < 0;
  const s = Math.abs(diff);
  const v = s < 60 ? "just now" : s < 3600 ? `${Math.round(s / 60)} min` : s < 86400 ? `${Math.round(s / 3600)} h` : `${Math.round(s / 86400)} d`;
  if (v === "just now") return v;
  return fut ? `in ${v}` : `${v} ago`;
}
