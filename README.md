# Adjutant

**An agentic Markdown knowledge base for managers.** Your notes, one living task
list, and an AI agent that files, links and summarizes them — all in a plain
folder you own.

> Status: early development (Phase 0 of the [roadmap](docs/PRD.md#milestones)).
> The ACP client and a streaming chat work; the editor and tasks are next.

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

## Try it

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
