# Adjutant — product requirements

Adjutant is a desktop app (Tauri; Linux first, also macOS and Windows) that turns a local Markdown folder into an agentic work knowledge base: notes, a living task list, brainstorming, repo Q&A and Jira reporting, driven by an AI agent over the [Agent Client Protocol](https://agentclientprotocol.com) (ACP). Any ACP agent works; JetBrains Junie is the reference agent for teams on a JetBrains AI subscription.

## Problem

An engineering manager leading several teams has context scattered across Jira, repos, 1:1 notes, meeting notes and memory. Writing things down is easy; keeping them organized, finding them again and turning them into reports, review drafts and promotion evidence is not. Existing tools either need a specific cloud AI the employer may not allow, are CLI-only, or are not built around a manager's workflow.

**Bet.** Plain Markdown files the user owns, plus a swappable ACP agent with CLI tools and MCP servers, plus a notes-first UI with a living task list, gives a second brain that stays useful for years and survives tool changes.

## Goals

1. Capture anything (note, task, link, meeting dump) in under 5 seconds from anywhere on the desktop.
2. Let the agent organize, link and summarize notes, so the vault stays navigable without manual filing.
3. Run the day from one place: one always-current task list, a "Waiting on" list per report, and Jira context beside them.
4. Answer questions across the vault, cloned repos and Jira with one agent session.
5. Produce recurring outputs automatically: weekly Jira report, end-of-day log, review and promotion evidence.
6. Stay agent-agnostic via ACP and tool-agnostic via [mise](https://mise.jdx.dev), so adding an agent or CLI is a config change.

## Non-goals (v1)

- Writing to Jira from the app itself (the agent may, with approval).
- Two-way task sync with Jira or any task manager.
- Mobile app, multi-user collaboration or cloud sync (git is the sync story).
- Building an LLM, embeddings index or agent loop.
- Supporting non-ACP agents.

## User and core use cases

The primary user is an engineering manager with direct reports across one or more teams, developer-comfortable: edits config files, writes mise tasks, reads logs.

| Use case | Example | Agent? |
|---|---|---|
| Quick capture | Global hotkey → "ask the platform team about alert noise" lands in `tasks.md` | No |
| Inbox triage | "Organize inbox" files raw notes into `people/`, `teams/`, `me/` with links | Yes |
| Daily plan | Open tasks + Jira tickets → proposed top 3, slipping items flagged | Yes |
| Task extraction | Paste 1:1 notes → action items under Now or Waiting on, linked to the person | Yes |
| Brainstorm | Promotion case, team structure, roadmap trade-offs; result saved as a note | Yes |
| Repo Q&A | Paste a repo URL → agent clones into `repos/` and answers questions | Yes |
| Jira at a glance | A task mentioning `ABC-123` shows live status; "My Jira" panel | No |
| Weekly report | Every Friday: `reports/2026-W44.md` from Jira snapshot + daily logs | Yes, scheduled |
| Review drafting | "Draft X's mid-year review" from `people/x.md` and 6 months of logs | Yes |
| 1:1 prep | Open a person → their Waiting on items, recent notes, open tickets | Partly |

## Constraints

- **Agent choice is the employer's.** Only approved agents are used; the app never ships its own model access or proxies.
- **Cross-platform.** No dependency on any vendor's desktop app or OS-only features.
- **Data stays local.** Everything is work data, including performance reviews. It lives in the vault on the user's machine.
- **Credits.** Agent usage is usually metered. Routine work (syncing Jira, rendering tasks, archiving) never calls the agent.
- **Scheduled jobs** run through the app's own ACP session with the user's normal agent login, so the app must be running when jobs are due.
- **Local fallback.** A local-model ACP agent (e.g. Ollama-backed) can be pinned to sensitive folders.

## Functional requirements

P0 = MVP, P1 = shortly after, P2 = later.

### Vault and editor
- **P0** Open a folder as the vault; file tree; Markdown editor with live preview; `[[wikilinks]]` with autocomplete and backlinks.
- **P0** Full-text search (ripgrep-backed) across the vault.
- **P0** Global quick-capture hotkey: a small window that appends a task to `tasks.md` or a note to `inbox/`.
- **P1** Templates for 1:1, meeting, person, decision; command palette.
- **P2** Graph view of links.

### Tasks
- **P0** One always-current task list, `tasks.md`, with sections `## Now`, `## Next`, `## Waiting on`, `## Someday`; tasks are `- [ ]` checkboxes. No per-day files.
- **P0** Checking a task off stamps `@done(YYYY-MM-DD)` and moves it to `tasks/done/YYYY-MM.md`.
- **P0** Tasks panel: every open task across the vault, grouped by Now / Waiting on / Due soon; ticking one edits the source file.
- **P0** Inline syntax: `@due(YYYY-MM-DD)`, `#tag`, `[[people/name]]`, Jira keys like `ABC-123`.
- **P1** Per-person view for 1:1 prep.
- **P1** Activity log `log/YYYY-MM.md`, appended by the end-of-day action.

### Agent (ACP)
- **P0** Chat panel bound to the vault as working directory; streams messages, tool calls, plans and diffs.
- **P0** ACP client capabilities: file read/write scoped to the vault, terminal, permission prompts with allow once / always / deny.
- **P0** Agent picker listing the ACP agents installed by the vault's `mise.toml`.
- **P0** Agent discovery: `mise ls --current --json` matched against a built-in table of known ACP agents (Junie `--acp=true`, Gemini CLI `--acp`, claude-agent-acp, codex-acp). `[agents.*]` in `.vault/config.toml` add or override entries. Known agents not installed show greyed out with the `mise.toml` line to add them.
- **P0** One-click actions: Organize inbox, Plan my day, Extract tasks from this note, End-of-day summary, Draft review for person.
- **P1** Per-folder agent override, session history and resume, context mentions (`@file`, `@person`, `@ticket`).

### Repos
- **P0** Paste a repo URL → agent clones it into `repos/` (gitignored) and answers questions about it.
- **P1** Repo list with last-pulled time; refresh.

### Ingest (P1)
- Import a page by URL or from the current browser tab through a browser MCP server, saved as Markdown with front matter (source URL, title, fetched date).
- Prefer approved APIs (e.g. Confluence via the Atlassian MCP server) over the browser.
- Re-sync an ingested page with a diff before overwriting. Browser access is read-only.

### Jira (Cloud)
- **P0** Background sync every 15 minutes via Jira Cloud REST: my tickets plus configured JQL; writes `jira/snapshot.md` and `jira/cache.json`.
- **P0** Status badge next to every Jira key in notes and tasks, linking to Jira.
- **P0** My Jira panel; one click creates a vault task referencing the ticket.
- **P1** Agent access to Jira via an MCP server, snapshot first; agent-proposed Jira writes only through permission prompts.

### Tooling (mise)
- **P0** `mise.toml` in the vault pins the ACP agents and the CLIs exposed to them.
- **P0** The agent is spawned as `mise exec -- <agent command>`, so its PATH holds exactly those tools.
- **P0** `AGENTS.md` documents each tool with exact commands.
- **P1** Secrets come from the system keyring and are injected as env vars at spawn time.

### Scheduling
- **P0** Built-in scheduler (cron expressions in `.vault/config.toml`); autostart on login, close minimizes to tray.
- **P0** Deterministic jobs run in-process (Jira sync, task archive); agent jobs open an ACP session and send a stored prompt.
- **P0** Missed runs catch up once on wake or start, never twice.
- **P0** Schedules screen: list, toggle, edit, run now, last status and log.
- **P0** Job outputs land in `reports/`; the app shows a badge and a desktop notification.

## Architecture

The app owns the UI and all deterministic work (files, tasks, Jira sync); the agent is a child process it talks to over ACP and can swap at any time. File edits requested through ACP go through the client, which enforces the vault sandbox and permission prompts.

Stack: Tauri 2 with a Rust backend; the `agent-client-protocol` crate (pinned, isolated in `crates/adjutant-acp`); Svelte 5 + CodeMirror 6 frontend; ripgrep for search; `notify` for file watching; the system keyring; git for history and sync.

## Vault layout

```
vault/
  inbox/                 raw captures, triaged by the agent
  tasks.md               Now / Next / Waiting on / Someday
  tasks/done/2026-10.md  completed tasks, stamped @done
  log/2026-10.md         end-of-day activity log
  me/                    goals, brag doc, promotion case, manager 1:1s
  people/<name>.md       one file per direct report
  teams/<team>/          decisions, incidents, roadmap notes
  reports/2026-W44.md    generated reports
  jira/snapshot.md       generated by Jira sync (+ cache.json)
  repos/                 cloned repos (gitignored)
  sources/               ingested pages, with source URL and date
  templates/
  AGENTS.md              agent guidelines: rules, folder meanings, tool usage
  mise.toml              pinned agents + CLIs
  .vault/config.toml     agents, permissions, Jira, schedules, hotkeys
  .vault/audit.log       every agent tool call and file access
  .vault/sessions/       saved agent sessions (gitignored)
```

Task line grammar:

```
- [ ] Review ABC-123 rollout plan #platform @due(2026-10-30)
- [ ] Promotion doc feedback [[people/manager]] @due(2026-11-05)
```

## Security and privacy

- **Sandboxed file access.** ACP file read/write is limited to the vault.
- **Permission prompts by default.** "Always allow" is per tool kind and per agent, stored in `.vault/config.toml`.
- **Secrets in the keyring**, never in the vault or git.
- **Sensitive folders** can be pinned to a local-model agent or marked "no agent".
- **No telemetry.** Only the agent and the Jira client talk to the network.
- **Audit log** of every agent tool call and file access in `.vault/audit.log`.
- **Check your policies** before putting review notes or candidate data through a cloud model.

## Milestones

| Phase | Scope | Exit gate |
|---|---|---|
| 0. Spike | Rust ACP client spawns an agent, sends a prompt, renders streamed updates | Prompt round-trip works; agent feature set documented |
| 1. MVP | Vault tree, editor, search, quick capture, tasks with done archive, Tasks panel, chat panel, one-click actions, mise-spawned agent | Used daily for a week |
| 2. Jira | REST sync, snapshot, badges, My Jira panel | Morning plan uses Jira context without copy-paste |
| 3. Schedules | Scheduler, autostart and tray, Schedules screen, weekly report and end-of-day jobs | Four weekly reports generated unattended |
| 4. Polish | Ingest, per-person view, per-folder agents, session history, templates | Review season run from the vault |

## Risks

| Risk | Mitigation |
|---|---|
| An agent exposes less over ACP than in its CLI | Actions are plain prompts, not slash commands; spike documents feature sets |
| ACP spec and crate change between versions | Crate pinned exactly; ACP layer isolated in one crate |
| Credit burn | Snapshot-first Jira, no agent for routine work |
| App not running when a job is due | Autostart, tray mode, catch-up on start |
| Building a notes app is a big surface | MVP limited to the P0 list |
