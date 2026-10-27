# Adjutant

**An agentic Markdown knowledge base for managers.** Your notes, one living task
list, and an AI agent that files, links and summarizes them — all in a plain
folder you own.

> Status: early development. Phases 0–3 of the [roadmap](docs/PRD.md#milestones)
> are implemented: vault, live-preview editor, tasks, agent chat, Jira Cloud
> sync and the scheduler. Expect rough edges.

## What's in it

- **Today**: the day on one page. Now, Waiting on (grouped by person), due this week, inbox count, your Jira tickets, and one-click *Plan my day* / *End-of-day summary*.
- **Editor**: CodeMirror 6 with live preview. Markdown syntax hides away from the cursor, `[[wikilinks]]` autocomplete and click through, checkboxes tick, Jira keys show live status, backlinks under every note.
- **Tasks**: one `tasks.md` with Now / Next / Waiting on / Someday. Ticking a task stamps `@done(date)` and moves it to `tasks/done/YYYY-MM.md`. Open tasks from every note in one list.
- **People**: a person's note shows what you're waiting on from them and what's open, for 1:1 prep, plus *Draft review*.
- **Agent**: any ACP agent installed by the vault's `mise.toml`. Streams messages, plans, tool calls and diffs; asks before acting; "always allow" per tool kind. Actions: plan my day, organize inbox, extract tasks, end-of-day summary, draft review, ask about a repo.
- **Quick capture**: a global shortcut (default `Ctrl+Shift+Space`) opens a small window. Enter adds a task, Ctrl+Enter files a note to `inbox/`.
- **Jira Cloud**: background sync every 15 minutes into `jira/snapshot.md` and `jira/cache.json`; token in the system keyring.
- **Schedules**: cron jobs in `.vault/config.toml`: Jira sync, task archive, and agent jobs (weekly report, end-of-day log, morning plan) that write to `reports/` and `log/`. Missed runs catch up once. Tray icon, autostart, desktop notifications.
- **Search**: ripgrep-powered full-text search; `Ctrl+K` command palette for notes, commands and templates.

## Why

Context for a people manager is scattered across Jira, repos, 1:1 notes and
memory. Adjutant keeps it in Markdown files and lets an agent do the
organizing, while the app does all routine work (tasks, Jira sync, schedules)
without spending agent credits.

- **Your files.** A vault is a folder of Markdown versioned with git. No database, no cloud.
- **Any agent.** Talks to agents over the [Agent Client Protocol](https://agentclientprotocol.com):
  JetBrains Junie, Claude Agent, Gemini CLI, Codex, or your own.
- **Your tools.** The vault's `mise.toml` pins the agents and the CLIs they may use.
  Adding an agent is one line of config.
- **Safe by default.** File access is sandboxed to the vault, tool calls need
  permission, and every agent action lands in `.vault/audit.log`.

## Install

New here? Follow [Getting started](docs/getting-started.md): install, mise,
and a guided setup for the agent, privacy and schedules.

Download the latest build for Linux (AppImage, deb, rpm), macOS (dmg) or
Windows (exe) from [Releases](https://github.com/DiverOfDark/adjutant/releases/latest).
Every push to `main` is a release (`0.1.<run>`); the app downloads updates in
the background and installs them when you quit from the tray, or right away
with *Restart to update* in Settings. On Linux, auto-update works for the
AppImage.

## Build from source

Requirements: [mise](https://mise.jdx.dev), Rust (stable), and the
[Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for your OS.

```sh
git clone https://github.com/DiverOfDark/adjutant && cd adjutant
mise install && npm install
(cd examples/demo-vault && mise trust && mise install)
mise run dev          # opens the app on examples/demo-vault
```

The demo vault pins `claude-agent-acp`, which uses your Claude Code login. To
use JetBrains Junie instead, add `"npm:@jetbrains/junie-cli" = "latest"` to the
vault's `mise.toml` and pick Junie in the agent picker.

ACP spike without the UI:

```sh
cargo run -p adjutant-acp --example acp_spike -- \
    --vault examples/demo-vault "What is due soonest?"
```

## Layout

| Path | What |
|---|---|
| `crates/adjutant-acp` | ACP client: agent process, sandboxed fs, terminals, permissions. The only crate that depends on `agent-client-protocol`. |
| `crates/adjutant-core` | Deterministic core: config, agent discovery, (soon) tasks, search, Jira, scheduler. |
| `src-tauri` | Tauri app: commands, events, audit log. |
| `src` | Svelte 5 frontend. |
| `examples/demo-vault` | A sample vault with `AGENTS.md`, `mise.toml` and `.vault/config.toml`. |
| `docs` | [PRD](docs/PRD.md), [ACP spike notes](docs/acp-spike.md). |

## License

[Apache-2.0](LICENSE)
