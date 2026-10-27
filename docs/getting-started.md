# Getting started

About 15 minutes on a fresh laptop, most of it downloads.

## Before you start

- **Check your company's rules.** Notes, 1:1s and reviews will go to whatever
  agent you pick. Use only an agent your company approves (for example
  JetBrains Junie on a company JetBrains AI subscription), and decide whether
  `people/` and `me/` should be hidden from it (setup offers this).
- **A JetBrains account** with AI access, if you use Junie.
- **Jira Cloud** (optional): an API token from
  <https://id.atlassian.com/manage-profile/security/api-tokens>, if personal
  tokens are allowed for local tools.

## 1. Install Adjutant

Download from [the latest release](https://github.com/DiverOfDark/adjutant/releases/latest):

| OS | File | Notes |
|---|---|---|
| Linux | `Adjutant_<version>_amd64.AppImage` | Updates itself. `chmod +x` it; on Ubuntu 22.04+ you may need `sudo apt install libfuse2`. |
| Linux | `.deb` / `.rpm` | Install with your package manager; update by installing the next release. |
| macOS | `.dmg` | Not notarized yet: right-click the app, then Open, the first time. |
| Windows | `_x64-setup.exe` | Not signed yet: SmartScreen asks once. |

## 2. Install mise

Adjutant installs the agent and its tools through [mise](https://mise.jdx.dev),
pinned in each vault's `mise.toml`.

```sh
curl https://mise.run | sh      # Linux and macOS (installs to ~/.local/bin)
winget install jdx.mise         # Windows
```

## 3. Open a vault and run setup

Start Adjutant and choose a folder: an empty one for a new vault, or an
existing folder of Markdown notes (nothing is overwritten). The setup screen
walks through the rest:

1. **Vault folders and files**: adds `tasks.md`, `AGENTS.md`, `mise.toml`,
   `.vault/config.toml`, folders and templates.
2. **mise**: found, or the command to install it.
3. **Agent and tools**: trusts the vault's `mise.toml` and runs `mise install`
   (Node, Junie, ripgrep, fd, jq, yq). The first run takes a few minutes.
4. **Agent login**: starts the agent without sending a prompt. If Junie needs
   your JetBrains login, *Log in from a terminal* opens one running
   `mise exec -- junie`; finish there and test again.
5. **Privacy**: hide `people/` and `me/` from agents, keep or turn off update
   checks, track the vault with git.
6. **In the background**: start on login, opt in to agent jobs (weekly
   report, end-of-day log, morning plan), and the quick capture shortcut.
7. **Jira** (optional).

You can run it again from Settings.

## Quick capture shortcut

**Ctrl+Shift+Space** (Cmd+Shift+Space on macOS) opens quick capture from
anywhere. The tray menu shows the current shortcut.

On Wayland, apps can't grab keys themselves. Adjutant asks the desktop
through its global-shortcuts portal (KDE Plasma 6, GNOME 48+), which shows a
dialog the first time: accept it. Change the key later in your system's
shortcut settings, or from Settings → *Change it…*. To do this, Adjutant adds
a hidden `dev.adjutant.app.desktop` entry to `~/.local/share/applications`.

On desktops without that portal, add a custom shortcut in your system
settings that runs the command setup shows, for example:

```sh
"$HOME/Apps/Adjutant.AppImage" --capture
```

## What talks to the network

- The agent you choose (for Junie: JetBrains AI).
- Jira Cloud, if you connect it.
- GitHub, to check for Adjutant updates (Settings → *Check GitHub for updates*).

Nothing else. Agent activity is logged in `.vault/audit.log`.

## Using another agent

Add it to the vault's `mise.toml` and pick it in the agent panel:

```toml
"npm:@agentclientprotocol/claude-agent-acp" = "latest"   # Claude Agent
"npm:@google/gemini-cli" = "latest"                       # Gemini CLI
```

Custom agents go in `.vault/config.toml` under `[agents.<id>]` with `command`
and `args`.
