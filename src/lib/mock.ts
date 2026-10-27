// In-memory backend used when the UI runs outside Tauri (browser dev,
// screenshots). Mirrors the Tauri commands closely enough to exercise the UI.
import type {
  AgentEvent,
  JiraCache,
  RunRecord,
  ScheduleView,
  SearchResults,
  Task,
  TreeNode,
} from "./api";

const today = new Date();
const iso = (d: Date) => d.toISOString().slice(0, 10);
const plus = (days: number) => iso(new Date(today.getTime() + days * 86400000));

const files: Record<string, string> = {
  "tasks.md": `# Tasks

## Now
- [ ] Review PLAT-123 rollout plan #platform @due(${plus(1)})
- [ ] Prepare 1:1 agenda for [[people/alex]] @due(${plus(0)})
- [ ] Send promotion packet feedback to [[people/maria]] #reviews @due(${plus(-1)})

## Next
- [ ] Draft Q1 roadmap trade-offs #platform
- [ ] Decide on on-call rotation change DATA-88

## Waiting on
- [ ] Load-test results from [[people/sam]] @due(${plus(2)})
- [ ] Hiring plan numbers from [[people/maria]]
- [ ] Incident write-up for INC-42 from [[people/alex]]

## Someday
- [ ] Write a team charter
`,
  "people/alex.md": `# Alex Doe

Senior engineer on the Platform team. Wants to lead the ingestion rewrite.

## 1:1s

### ${plus(-7)}
- Wants to lead the ingestion rewrite; agreed to draft a proposal by Friday.
- Concerned about on-call load — 3 pages last week, two were alert noise.
- [ ] Ask SRE about tuning the rebalancing alert #platform

### ${plus(-21)}
- Discussed growth goals: system design, mentoring Sam.

## Feedback & review evidence
- Led the PLAT-101 incident response calmly; good write-up in [[teams/platform/incidents]].
`,
  "people/sam.md": "# Sam Roe\n\nEngineer, Platform team. Running the load tests for PLAT-123.\n",
  "people/maria.md": "# Maria Lopez\n\nEngineering manager, Data team.\n",
  "teams/platform/roadmap.md": "# Platform roadmap\n\n## Q1 candidates\n- Ingestion rewrite ([[people/alex]])\n- Alert noise reduction\n",
  "teams/platform/incidents.md": "# Incidents\n\n## INC-42 — rebalancing stalled\nOwner: [[people/alex]]\n",
  "me/goals.md": "# My goals\n\n- Grow two senior engineers into tech leads.\n- Ship the ingestion rewrite by end of Q1.\n",
  "inbox/2026-10-27-alert-noise.md": "ask platform about rebalancing alert noise - too many pages last week, maybe tune thresholds. Sam mentioned PLAT-130.\n",
  "inbox/2026-10-26-offsite.md": "offsite idea: half day on roadmap, half day on team health survey results\n",
  "AGENTS.md": "# Agent guidelines for this vault\n\nNever delete notes. Move, link and append instead.\n",
  "reports/2026-W43.md": "# Weekly report 2026-W43\n\n## Shipped\n- PLAT-118 shard balancer config\n",
  "log/2026-10.md": `# 2026-10\n\n## ${plus(-1)}\n- Closed PLAT-118.\n- Decided to postpone the ingestion rewrite kickoff to next sprint.\n`,
};

function buildTree(): TreeNode {
  const root: TreeNode = { name: "demo-vault", path: "", isDir: true, children: [] };
  for (const path of Object.keys(files).sort()) {
    const parts = path.split("/");
    let node = root;
    parts.forEach((part, i) => {
      const p = parts.slice(0, i + 1).join("/");
      let child = node.children.find((c) => c.name === part);
      if (!child) {
        child = { name: part, path: p, isDir: i < parts.length - 1, children: [] };
        node.children.push(child);
      }
      node = child;
    });
  }
  const sort = (n: TreeNode) => {
    n.children.sort((a, b) => Number(b.isDir) - Number(a.isDir) || a.name.localeCompare(b.name));
    n.children.forEach(sort);
  };
  sort(root);
  return root;
}

const TASK_RE = /^(\s*)[-*+] \[( |x|X)\] (.*)$/;

function parseTasks(): Task[] {
  const out: Task[] = [];
  for (const [file, content] of Object.entries(files)) {
    let section: string | null = null;
    content.split("\n").forEach((raw, line) => {
      if (raw.startsWith("## ")) section = raw.slice(3).trim();
      const m = raw.match(TASK_RE);
      if (!m) return;
      const text = m[3];
      const links = [...text.matchAll(/\[\[([^\]|#]+)/g)].map((x) => x[1]);
      out.push({
        file,
        line,
        raw,
        text,
        title: text.replace(/@(due|done)\([^)]*\)/g, "").replace(/\s+/g, " ").trim(),
        done: m[2] !== " ",
        section,
        due: text.match(/@due\((\d{4}-\d{2}-\d{2})\)/)?.[1] ?? null,
        doneOn: null,
        tags: [...text.matchAll(/(?:^|\s)#([\w/-]+)/g)].map((x) => x[1]),
        links,
        people: links.filter((l) => l.startsWith("people/")),
        jiraKeys: [...text.matchAll(/\b[A-Z][A-Z0-9]+-\d+\b/g)].map((x) => x[0]),
        indent: m[1].length,
      });
    });
  }
  return out;
}

const jira: JiraCache = {
  syncedAt: new Date(today.getTime() - 4 * 60000).toISOString(),
  baseUrl: "https://example.atlassian.net",
  issues: Object.fromEntries(
    [
      ["PLAT-123", "Roll out shard rebalancer v2", "In Review", "indeterminate", "Alex Doe"],
      ["PLAT-130", "Rebalancing alert fires on transient lag", "To Do", "new", "Sam Roe"],
      ["PLAT-118", "Shard balancer config", "Done", "done", "Alex Doe"],
      ["DATA-88", "On-call rotation proposal", "In Progress", "indeterminate", "Kirill"],
      ["INC-42", "Rebalancing stalled in eu-west", "Post-mortem", "indeterminate", "Alex Doe"],
    ].map(([key, summary, status, cat, assignee]) => [
      key,
      {
        key,
        summary,
        status,
        statusCategory: cat,
        issueType: "Task",
        priority: "Medium",
        assignee,
        updated: plus(-1),
        due: null,
        url: `https://example.atlassian.net/browse/${key}`,
      },
    ]),
  ),
  queries: { mine: ["DATA-88", "PLAT-123"], platform: ["PLAT-123", "PLAT-130", "INC-42", "PLAT-118"] },
};

const run = (status: RunRecord["status"], hoursAgo: number, message: string, output: string | null = null): RunRecord => ({
  startedAt: new Date(today.getTime() - hoursAgo * 3600000).toISOString(),
  finishedAt: new Date(today.getTime() - hoursAgo * 3600000 + 42000).toISOString(),
  status,
  message,
  output,
  log: `${message}\n`,
  trigger: "schedule",
});

const schedules: ScheduleView[] = [
  { id: "jira-sync", name: "Jira sync", cron: "0 */15 * * * *", kind: "jira-sync", enabled: true, nextRun: new Date(today.getTime() + 11 * 60000).toISOString(), lastRun: run("ok", 0.07, "Synced 5 issues in 2 queries"), prompt: null, output: null, agent: null, error: null },
  { id: "task-archive", name: "Archive done tasks", cron: "0 55 23 * * *", kind: "task-archive", enabled: true, nextRun: new Date(today.getTime() + 6 * 3600000).toISOString(), lastRun: run("ok", 17, "Archived 3 tasks"), prompt: null, output: null, agent: null, error: null },
  { id: "end-of-day", name: "End-of-day log", cron: "0 0 18 * * MON-FRI", kind: "agent", enabled: true, nextRun: new Date(today.getTime() + 2 * 3600000).toISOString(), lastRun: run("ok", 22, "Appended to log/2026-10.md", "log/2026-10.md"), prompt: "Summarize what moved today: tasks completed (tasks/done), notes changed (git log), decisions. Append to the activity log.", output: "log/{month}.md", agent: null, error: null },
  { id: "weekly-report", name: "Weekly report", cron: "0 0 16 * * FRI", kind: "agent", enabled: true, nextRun: new Date(today.getTime() + 3 * 86400000).toISOString(), lastRun: run("failed", 98, "Agent exited: not logged in"), prompt: "Write this week's report from jira/snapshot.md, log/ and tasks/done/.", output: "reports/{week}.md", agent: "junie", error: null },
  { id: "morning-plan", name: "Morning plan", cron: "0 30 8 * * MON-FRI", kind: "agent", enabled: false, nextRun: null, lastRun: null, prompt: "Propose my top 3 for today.", output: "reports/plan-{date}.md", agent: null, error: null },
];

type Listener = (p: unknown) => void;
const listeners = new Map<string, Set<Listener>>();
function emit(event: string, payload: unknown) {
  listeners.get(event)?.forEach((l) => l(payload));
}
export function mockListen(event: string, cb: Listener) {
  if (!listeners.has(event)) listeners.set(event, new Set());
  listeners.get(event)!.add(cb);
  return Promise.resolve(() => listeners.get(event)!.delete(cb));
}

const agentEvent = (e: Record<string, unknown>) => emit("agent-event", { agentId: "claude-agent-acp", ...e } as AgentEvent);
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

async function fakeTurn(sessionId: string, prompt: string) {
  const up = (update: Record<string, unknown>) => agentEvent({ type: "sessionUpdate", sessionId, update });
  up({ sessionUpdate: "plan", entries: [
    { content: "Read tasks.md and jira/snapshot.md", priority: "high", status: "in_progress" },
    { content: "Pick the top three", priority: "medium", status: "pending" },
  ] });
  await sleep(300);
  up({ sessionUpdate: "tool_call", toolCallId: "t1", title: "Read tasks.md", kind: "read", status: "completed" });
  up({ sessionUpdate: "tool_call", toolCallId: "t2", title: "rg -n PLAT-123 jira/snapshot.md", kind: "search", status: "completed" });
  await sleep(300);
  up({ sessionUpdate: "plan", entries: [
    { content: "Read tasks.md and jira/snapshot.md", priority: "high", status: "completed" },
    { content: "Pick the top three", priority: "medium", status: "completed" },
  ] });
  const text = prompt.toLowerCase().includes("plan")
    ? "Here's a plan for today:\n\n1. **Send promotion feedback to [[people/maria]]** — it was due yesterday.\n2. **Prepare the 1:1 agenda for [[people/alex]]** — the meeting is today; bring up on-call load and the ingestion proposal.\n3. **Review PLAT-123** — it's *In Review* and due tomorrow.\n\nSlipping: load-test results from [[people/sam]] are due in two days and PLAT-130 is still *To Do*."
    : "The soonest item is the **promotion packet feedback** for [[people/maria]], due yesterday. Next is the 1:1 agenda for [[people/alex]] (today).";
  for (const chunk of text.match(/.{1,24}/gs) ?? []) {
    up({ sessionUpdate: "agent_message_chunk", content: { type: "text", text: chunk } });
    await sleep(25);
  }
  agentEvent({
    type: "permissionRequest",
    requestId: 7,
    sessionId,
    toolCall: {
      toolCallId: "t3",
      title: "Edit tasks.md",
      kind: "edit",
      content: [{ type: "diff", path: "tasks.md", oldText: "## Now\n- [ ] Review PLAT-123 rollout plan", newText: "## Now\n- [ ] Send promotion packet feedback to [[people/maria]]\n- [ ] Review PLAT-123 rollout plan" }],
    },
    options: [
      { optionId: "allow", name: "Allow", kind: "allow_once" },
      { optionId: "always", name: "Always allow", kind: "allow_always" },
      { optionId: "reject", name: "Reject", kind: "reject_once" },
    ],
  });
}

let mockSession = 0;

export async function mockInvoke<T>(cmd: string, a: Record<string, any>): Promise<T> {
  await sleep(40);
  const r = (v: unknown) => v as T;
  switch (cmd) {
    case "vault_last": return r("/home/demo/work-vault");
    case "vault_open": return r({ path: a.path, name: "work-vault", initialized: true });
    case "vault_scaffold": return r([]);
    case "vault_tree": return r(buildTree());
    case "note_read": return r(files[a.path] ?? "");
    case "note_write": files[a.path] = a.content; return r(undefined);
    case "note_create": files[a.path] = `# ${a.path.split("/").pop()?.replace(/\.md$/, "")}\n`; return r(a.path);
    case "notes_list": return r(Object.keys(files).filter((f) => f.endsWith(".md")).sort());
    case "backlinks": {
      const stem = a.path.replace(/\.md$/, "");
      return r(Object.entries(files).flatMap(([file, c]) =>
        file === a.path ? [] : c.split("\n").flatMap((text, line) => (text.includes(`[[${stem}`) ? [{ file, line, text: text.trim() }] : [])),
      ));
    }
    case "search": {
      const q = String(a.query).toLowerCase();
      const res: SearchResults = { fileMatches: Object.keys(files).filter((f) => f.toLowerCase().includes(q)), hits: [], truncated: false };
      for (const [file, c] of Object.entries(files))
        c.split("\n").forEach((text, line) => {
          const i = text.toLowerCase().indexOf(q);
          if (q && i >= 0) res.hits.push({ file, line, text, ranges: [[i, i + q.length]] });
        });
      return r(res);
    }
    case "templates_list": return r(["one-on-one", "meeting", "person", "decision"]);
    case "repos_list": return r([{ name: "shard-balancer", path: "repos/shard-balancer", remote: "git@github.com:example/shard-balancer.git", lastPulled: plus(-2) }]);
    case "note_visibility": return r(a.path.startsWith("people/") ? { agent: null, hidden: true, pinned: false } : { agent: "claude-agent-acp", hidden: false, pinned: false });
    case "tasks_list": return r(parseTasks());
    case "task_complete": {
      const lines = files[a.file].split("\n");
      lines.splice(a.line, 1);
      files[a.file] = lines.join("\n");
      return r({ archivedTo: "tasks/done/2026-10.md" });
    }
    case "task_add": {
      files["tasks.md"] = files["tasks.md"].replace(`## ${a.section}\n`, `## ${a.section}\n- [ ] ${a.text}\n`);
      return r(undefined);
    }
    case "capture_note": return r("inbox/capture.md");
    case "agents_discover":
      return r({
        agents: [
          { id: "claude-agent-acp", name: "Claude Agent", command: "claude-agent-acp", args: [], useMise: true, installed: true, version: "0.98.0", miseLine: null, builtin: true },
          { id: "junie", name: "JetBrains Junie", command: "junie", args: ["--acp=true"], useMise: true, installed: false, version: null, miseLine: '"npm:@jetbrains/junie-cli" = "latest"', builtin: true },
          { id: "gemini", name: "Gemini CLI", command: "gemini", args: ["--acp"], useMise: true, installed: false, version: null, miseLine: '"npm:@google/gemini-cli" = "latest"', builtin: true },
        ],
        defaultAgent: "claude-agent-acp",
        miseBin: "/usr/bin/mise",
        miseError: null,
        miseUntrusted: false,
      });
    case "agent_connect": return r({ agentInfo: { title: "Claude Agent", version: "0.98.0" }, agentCapabilities: {}, protocolVersion: 1 });
    case "chat_new_session": return r(`s${++mockSession}`);
    case "chat_prompt": await fakeTurn(a.sessionId, a.parts[0]?.text ?? ""); return r("end_turn");
    case "sessions_list": return r([
      { id: "s-old-1", agentId: "claude-agent-acp", title: "Plan my day", updatedAt: plus(-1) + "T08:31:00Z", items: [] },
      { id: "s-old-2", agentId: "claude-agent-acp", title: "Where is index rebalancing triggered?", updatedAt: plus(-3) + "T14:02:00Z", items: [] },
    ]);
    case "session_load": return r({ id: a.id, agentId: "claude-agent-acp", title: "Plan my day", updatedAt: plus(-1), items: [
      { kind: "user", id: "h1", text: "Plan my day" },
      { kind: "agent", id: "h2", text: "Top three: **PLAT-123 review**, the 1:1 with [[people/alex]], and the hiring plan." },
    ] });
    case "repo_pull": return r("Already up to date.");
    case "jira_settings": return r({ baseUrl: jira.baseUrl, email: "me@example.com", queries: { mine: "assignee = currentUser() AND resolution = Unresolved", platform: "project = PLAT AND resolution = Unresolved" }, hasToken: true });
    case "jira_cache":
    case "jira_sync": return r(jira);
    case "jira_test": return r("Demo User");
    case "schedules_list": return r(schedules);
    case "schedule_toggle": { const s = schedules.find((s) => s.id === a.id); if (s) s.enabled = a.enabled; return r(undefined); }
    case "app_settings": return r({ autostart: true, captureHotkey: "CommandOrControl+Shift+Space", closeToTray: true });
    default: return r(undefined);
  }
}
