// Typed wrappers around Tauri commands and events.
// Outside Tauri (plain browser, used for UI work and screenshots) calls go to
// an in-memory mock backend with demo data.
import type { UnlistenFn } from "@tauri-apps/api/event";

export const inTauri = typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (inTauri) {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke<T>(cmd, args);
  }
  const { mockInvoke } = await import("./mock");
  return mockInvoke<T>(cmd, args ?? {});
}

export async function listenTo<T>(event: string, cb: (payload: T) => void): Promise<UnlistenFn> {
  if (inTauri) {
    const { listen } = await import("@tauri-apps/api/event");
    return listen<T>(event, (e) => cb(e.payload));
  }
  const { mockListen } = await import("./mock");
  return mockListen(event, cb as (p: unknown) => void);
}

// ---------- Vault ----------

export interface VaultInfo {
  path: string;
  name: string;
  initialized: boolean;
}

export interface TreeNode {
  name: string;
  path: string;
  isDir: boolean;
  children: TreeNode[];
}

export interface Backlink {
  file: string;
  line: number;
  text: string;
}

export interface SearchHit {
  file: string;
  line: number;
  text: string;
  ranges: [number, number][];
}

export interface SearchResults {
  fileMatches: string[];
  hits: SearchHit[];
  truncated: boolean;
}

export interface RepoInfo {
  name: string;
  path: string;
  remote: string | null;
  lastPulled: string | null;
}

export interface Visibility {
  /** Agent that may read this file, null when hidden from agents. */
  agent: string | null;
  hidden: boolean;
}

// ---------- Tasks ----------

export interface Task {
  file: string;
  line: number;
  raw: string;
  text: string;
  title: string;
  done: boolean;
  section: string | null;
  due: string | null;
  doneOn: string | null;
  tags: string[];
  links: string[];
  people: string[];
  jiraKeys: string[];
  indent: number;
}

export const SECTIONS = ["Now", "Next", "Waiting on", "Someday"] as const;

// ---------- Agents ----------

export interface AgentEntry {
  id: string;
  name: string;
  command: string;
  args: string[];
  useMise: boolean;
  installed: boolean;
  version: string | null;
  miseLine: string | null;
  builtin: boolean;
}

export interface Discovery {
  agents: AgentEntry[];
  defaultAgent: string | null;
  miseBin: string | null;
  miseError: string | null;
  miseUntrusted: boolean;
}

export interface AgentInfo {
  agentInfo: { name?: string; title?: string; version?: string } | null;
  agentCapabilities: Record<string, unknown>;
  protocolVersion: unknown;
}

export type PromptPart = { type: "text"; text: string } | { type: "file"; path: string };

export interface PermissionOption {
  optionId: string;
  name: string;
  kind: "allow_once" | "allow_always" | "reject_once" | "reject_always";
}

export type AgentEvent = { agentId: string } & (
  | { type: "ready"; agentInfo: unknown; agentCapabilities: unknown }
  | { type: "sessionUpdate"; sessionId: string; update: SessionUpdate }
  | { type: "permissionRequest"; requestId: number; sessionId: string; toolCall: ToolCallUpdate; options: PermissionOption[] }
  | { type: "permissionDecided"; requestId: number; optionId: string | null; automatic: boolean }
  | { type: "fileRead"; sessionId: string; path: string }
  | { type: "fileWritten"; sessionId: string; path: string }
  | { type: "fileAccessDenied"; sessionId: string; path: string; reason: string }
  | { type: "terminalStarted"; sessionId: string; terminalId: string; command: string }
  | { type: "promptFinished"; sessionId: string; stopReason: string }
  | { type: "stderr"; line: string }
  | { type: "error"; message: string }
  | { type: "exited"; code: number | null }
);

export interface ContentBlock {
  type: string;
  text?: string;
  uri?: string;
  name?: string;
}

export interface ToolCallContent {
  type: "content" | "diff" | "terminal";
  content?: ContentBlock;
  path?: string;
  oldText?: string | null;
  newText?: string;
  terminalId?: string;
}

export interface ToolCallUpdate {
  toolCallId: string;
  title?: string;
  kind?: string;
  status?: "pending" | "in_progress" | "completed" | "failed";
  content?: ToolCallContent[];
  locations?: { path: string; line?: number }[];
  rawInput?: unknown;
}

export interface PlanEntry {
  content: string;
  priority: "high" | "medium" | "low";
  status: "pending" | "in_progress" | "completed";
}

export type SessionUpdate = { sessionUpdate: string; [key: string]: any };

export interface SavedSession {
  id: string;
  agentId: string;
  title: string;
  updatedAt: string;
  items: unknown[];
}

// ---------- Jira ----------

export interface JiraIssue {
  key: string;
  summary: string;
  status: string;
  statusCategory: "new" | "indeterminate" | "done" | string;
  issueType: string | null;
  priority: string | null;
  assignee: string | null;
  updated: string | null;
  due: string | null;
  url: string;
}

export interface JiraCache {
  syncedAt: string;
  baseUrl: string;
  issues: Record<string, JiraIssue>;
  queries: Record<string, string[]>;
}

export interface JiraSettings {
  baseUrl: string;
  email: string;
  queries: Record<string, string>;
  hasToken: boolean;
}

// ---------- Schedules ----------

export interface RunRecord {
  startedAt: string;
  finishedAt: string | null;
  status: "running" | "ok" | "failed" | "skipped";
  message: string | null;
  output: string | null;
  log: string;
  trigger: "schedule" | "catchup" | "manual";
}

export interface ScheduleView {
  id: string;
  name: string;
  cron: string;
  kind: "jira-sync" | "task-archive" | "agent";
  enabled: boolean;
  nextRun: string | null;
  lastRun: RunRecord | null;
  prompt: string | null;
  output: string | null;
  agent: string | null;
  error: string | null;
}

export interface AppSettings {
  autostart: boolean;
  captureHotkey: string;
  closeToTray: boolean;
}

export const api = {
  // vault
  vaultOpen: (path: string) => call<VaultInfo>("vault_open", { path }),
  vaultLast: () => call<string | null>("vault_last"),
  vaultScaffold: () => call<string[]>("vault_scaffold"),
  vaultTree: () => call<TreeNode>("vault_tree"),
  noteRead: (path: string) => call<string>("note_read", { path }),
  noteWrite: (path: string, content: string) => call<void>("note_write", { path, content }),
  noteCreate: (path: string, template?: string) => call<string>("note_create", { path, template }),
  noteRename: (from: string, to: string) => call<number>("note_rename", { from, to }),
  noteTrash: (path: string) => call<void>("note_trash", { path }),
  dirCreate: (path: string) => call<void>("dir_create", { path }),
  notesList: () => call<string[]>("notes_list"),
  backlinks: (path: string) => call<Backlink[]>("backlinks", { path }),
  search: (query: string) => call<SearchResults>("search", { query }),
  templatesList: () => call<string[]>("templates_list"),
  reposList: () => call<RepoInfo[]>("repos_list"),
  noteVisibility: (path: string) => call<Visibility>("note_visibility", { path }),
  // tasks
  tasksList: () => call<Task[]>("tasks_list"),
  taskComplete: (t: Task) => call<{ archivedTo: string | null }>("task_complete", { file: t.file, line: t.line, raw: t.raw }),
  taskReopen: (t: Task) => call<void>("task_reopen", { file: t.file, line: t.line, raw: t.raw }),
  taskAdd: (section: string, text: string) => call<void>("task_add", { section, text }),
  taskMove: (t: Task, section: string) => call<void>("task_move", { line: t.line, raw: t.raw, section }),
  captureNote: (text: string) => call<string>("capture_note", { text }),
  // agents
  agentsDiscover: () => call<Discovery>("agents_discover"),
  agentsTrustVault: () => call<Discovery>("agents_trust_vault"),
  agentConnect: (agentId: string | null) => call<AgentInfo>("agent_connect", { agentId }),
  agentDisconnect: () => call<void>("agent_disconnect"),
  chatNewSession: () => call<string>("chat_new_session"),
  chatPrompt: (sessionId: string, parts: PromptPart[]) => call<string>("chat_prompt", { sessionId, parts }),
  chatCancel: (sessionId: string) => call<void>("chat_cancel", { sessionId }),
  permissionRespond: (requestId: number, optionId: string | null, alwaysToolKind?: string) =>
    call<void>("permission_respond", { requestId, optionId, alwaysToolKind }),
  sessionsList: () => call<SavedSession[]>("sessions_list"),
  sessionSave: (session: SavedSession) => call<void>("session_save", { session }),
  // jira
  jiraSettings: () => call<JiraSettings>("jira_settings"),
  jiraSaveSettings: (settings: Omit<JiraSettings, "hasToken">, token: string | null) =>
    call<void>("jira_save_settings", { settings, token }),
  jiraTest: () => call<string>("jira_test"),
  jiraSync: () => call<JiraCache>("jira_sync"),
  jiraCache: () => call<JiraCache | null>("jira_cache"),
  // schedules
  schedulesList: () => call<ScheduleView[]>("schedules_list"),
  scheduleToggle: (id: string, enabled: boolean) => call<void>("schedule_toggle", { id, enabled }),
  scheduleUpdate: (id: string, cron: string | null, prompt: string | null, output: string | null) =>
    call<void>("schedule_update", { id, cron, prompt, output }),
  scheduleRun: (id: string) => call<void>("schedule_run", { id }),
  // app
  appSettings: () => call<AppSettings>("app_settings"),
  appSaveSettings: (settings: AppSettings) => call<void>("app_save_settings", { settings }),
  captureHide: () => call<void>("capture_hide"),
};

export const onAgentEvent = (cb: (e: AgentEvent) => void) => listenTo<AgentEvent>("agent-event", cb);
export const onVaultChanged = (cb: (paths: string[]) => void) => listenTo<string[]>("vault-changed", cb);
export const onJiraSynced = (cb: (c: JiraCache) => void) => listenTo<JiraCache>("jira-synced", cb);
export const onScheduleRun = (cb: (r: { id: string; record: RunRecord }) => void) =>
  listenTo<{ id: string; record: RunRecord }>("schedule-run", cb);
