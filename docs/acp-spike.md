# Phase 0 spike: ACP round-trip

Goal: a Rust ACP client spawns an agent through mise, sends a prompt, renders
streamed updates, and documents what each agent exposes over ACP.

## How to reproduce

```sh
cd examples/demo-vault && mise trust && mise install && cd ../..
cargo run -p adjutant-acp --example acp_spike -- \
    --vault examples/demo-vault --agent claude-agent-acp --dump-init \
    "Read tasks.md and reply with one sentence: what is due soonest?"
```

`--agent junie` runs `junie --acp=true` (add `"npm:@jetbrains/junie-cli" = "latest"`
to the vault's `mise.toml` first). Permission prompts are answered on stdin.

## Result (2026-10-27)

Round-trip works with `@agentclientprotocol/claude-agent-acp` 0.98.0 and
`agent-client-protocol` crate 3.5.0 (protocol v1):

```
» spawning: mise exec -- claude-agent-acp  (cwd …/examples/demo-vault)
» session 69ac298c-…
» tool [execute] Terminal (pending)
» tool toolu_0148… → completed
Soonest is the load-test results you're waiting on from [[people/sam]], due
2026-10-29 …
» stop reason: "end_turn"
```

### claude-agent-acp 0.98.0

| Feature | Exposed over ACP |
|---|---|
| `loadSession` | yes |
| Session list / resume / delete / close | yes |
| Prompt: images, embedded context | yes; audio no |
| MCP servers in `session/new` | http and sse (stdio always) |
| Slash commands (`available_commands_update`) | yes |
| Session title (`session_info_update`) | yes |
| Token usage (`usage_update`) | yes |
| Plans | yes |

Findings:

1. **The agent uses its own tools for reads and shell commands.** In the run
   above it read files with its built-in terminal tool rather than the
   client's `fs/read_text_file`. The client sandbox only covers requests the
   agent routes through the client; the agent's own tools are governed by its
   permission prompts. `AGENTS.md` rules and permission prompts are the
   control here, not the sandbox alone.
2. **It loads the user's global Claude Code setup** (`~/.claude`: skills,
   plugins, CLAUDE.md). The spike session advertised 98 slash commands from
   the developer's personal config. Fine for local debugging; for a clean
   vault agent, run it with a separate `CLAUDE_CONFIG_DIR` via
   `[agents.claude-agent-acp] env` in `.vault/config.toml`.
3. **stderr is chatty** (per-phase timing lines). Adjutant shows it under
   "agent log" in the chat panel instead of as errors.
4. **First spawn installs tools.** `mise exec` installs missing tools before
   starting the agent, so `initialize` has a generous timeout (180 s).

### JetBrains Junie

Not yet run in this spike (needs a JetBrains AI login). To fill in this table
on a machine with Junie:

```sh
cargo run -p adjutant-acp --example acp_spike -- \
    --vault <vault> --agent junie --dump-init "Say hi"
```

| Feature | Exposed over ACP |
|---|---|
| `loadSession` | ? |
| Slash commands | ? |
| Model switching (session config options) | ? |
| Client `fs/*` used for edits | ? |
| Client terminal used for commands | ? |

## Design decisions from the spike

- The ACP crate is pinned to `=3.5.0` and only `crates/adjutant-acp` depends
  on it. The rest of the app sees `AgentHandle` and JSON `AgentEvent`s.
- Handlers in the crate run inside the dispatch loop, so permission prompts
  and `terminal/wait_for_exit` are answered from spawned tasks.
- The agent is spawned by Adjutant (not by the crate's `AcpAgent`) to control
  cwd, env and stderr, and to kill the whole process tree (mise → npx → node)
  via a process group on Unix and `taskkill /T` on Windows.
- One-click actions are plain prompts, not slash commands, so they work with
  any agent.
