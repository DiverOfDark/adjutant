//! Agent discovery: which ACP agents the vault's `mise.toml` provides.
//!
//! Runs `mise ls --current --json` in the vault and matches installed tools
//! against a built-in table of known ACP agents. `[agents.*]` entries in
//! `.vault/config.toml` add agents or override launch settings.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::VaultConfig;

pub struct KnownAgent {
    pub id: &'static str,
    pub name: &'static str,
    pub command: &'static str,
    pub args: &'static [&'static str],
    /// mise tool keys that provide this agent (first one is suggested).
    pub mise_tools: &'static [&'static str],
}

pub const KNOWN_AGENTS: &[KnownAgent] = &[
    KnownAgent {
        id: "junie",
        name: "JetBrains Junie",
        command: "junie",
        args: &["--acp=true"],
        mise_tools: &["npm:@jetbrains/junie-cli", "junie"],
    },
    KnownAgent {
        id: "claude-agent-acp",
        name: "Claude Agent",
        command: "claude-agent-acp",
        args: &[],
        mise_tools: &[
            "npm:@agentclientprotocol/claude-agent-acp",
            "npm:@zed-industries/claude-agent-acp",
        ],
    },
    KnownAgent {
        id: "gemini",
        name: "Gemini CLI",
        command: "gemini",
        args: &["--acp"],
        mise_tools: &["npm:@google/gemini-cli", "gemini", "gemini-cli"],
    },
    KnownAgent {
        id: "codex-acp",
        name: "Codex",
        command: "codex-acp",
        args: &[],
        mise_tools: &["npm:@zed-industries/codex-acp"],
    },
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AgentEntry {
    pub id: String,
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub secrets: BTreeMap<String, String>,
    pub use_mise: bool,
    pub installed: bool,
    pub version: Option<String>,
    /// Line to add to `mise.toml` when the agent is not installed.
    pub mise_line: Option<String>,
    pub builtin: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Discovery {
    pub agents: Vec<AgentEntry>,
    pub default_agent: Option<String>,
    pub mise_bin: Option<PathBuf>,
    /// Set when mise is missing or failed; installed state is then unknown.
    pub mise_error: Option<String>,
    /// mise refuses to read an untrusted `mise.toml`; the UI offers `mise trust`.
    pub mise_untrusted: bool,
}

#[derive(Debug, Deserialize)]
struct MiseToolVersion {
    version: String,
    #[serde(default)]
    installed: bool,
}

/// Finds the mise binary. Desktop launchers often lack the shell PATH, so
/// common install locations are checked as well.
pub fn find_mise(configured: Option<&Path>) -> Option<PathBuf> {
    if let Some(p) = configured {
        return Some(p.to_path_buf());
    }
    let exe = if cfg!(windows) { "mise.exe" } else { "mise" };
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            let p = dir.join(exe);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    let home = dirs_home()?;
    let candidates = [
        home.join(".local/bin").join(exe),
        home.join(".local/share/mise/bin").join(exe),
        home.join("AppData/Local/mise/bin").join(exe),
        PathBuf::from("/opt/homebrew/bin/mise"),
        PathBuf::from("/usr/local/bin/mise"),
        PathBuf::from("/usr/bin/mise"),
    ];
    candidates.into_iter().find(|p| p.is_file())
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

/// Runs mise and builds the agent list. Never fails: problems are reported
/// in [`Discovery::mise_error`] so the UI can explain them.
pub fn discover(vault: &Path, config: &VaultConfig) -> Discovery {
    let mise_bin = find_mise(config.agent.mise_bin.as_deref());
    let (tools, mise_error, mise_untrusted) = match &mise_bin {
        None => (
            HashMap::new(),
            Some("mise not found. Install it from https://mise.jdx.dev".to_string()),
            false,
        ),
        Some(bin) => match mise_ls(bin, vault) {
            Ok(tools) => (tools, None, false),
            Err(e) => {
                let untrusted = e.contains("not trusted");
                (HashMap::new(), Some(e), untrusted)
            }
        },
    };
    let mut d = build(config, &tools);
    d.mise_bin = mise_bin;
    d.mise_error = mise_error;
    d.mise_untrusted = mise_untrusted;
    d
}

/// Runs `mise trust` in the vault.
pub fn trust(vault: &Path, mise_bin: &Path) -> anyhow::Result<()> {
    let out = std::process::Command::new(mise_bin)
        .arg("trust")
        .arg("--yes")
        .current_dir(vault)
        .output()?;
    anyhow::ensure!(
        out.status.success(),
        "mise trust failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    Ok(())
}

fn mise_ls(bin: &Path, vault: &Path) -> Result<HashMap<String, String>, String> {
    let out = std::process::Command::new(bin)
        .args(["ls", "--current", "--json"])
        .current_dir(vault)
        .output()
        .map_err(|e| format!("failed to run mise: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    parse_mise_ls(&out.stdout).map_err(|e| format!("unexpected `mise ls` output: {e}"))
}

/// Tool key -> installed version, for installed tools only.
fn parse_mise_ls(json: &[u8]) -> serde_json::Result<HashMap<String, String>> {
    let raw: HashMap<String, Vec<MiseToolVersion>> = serde_json::from_slice(json)?;
    Ok(raw
        .into_iter()
        .filter_map(|(tool, versions)| {
            versions
                .into_iter()
                .find(|v| v.installed)
                .map(|v| (tool, v.version))
        })
        .collect())
}

fn build(config: &VaultConfig, tools: &HashMap<String, String>) -> Discovery {
    let mut agents = Vec::new();
    for known in KNOWN_AGENTS {
        let found = known.mise_tools.iter().find_map(|t| tools.get(*t).cloned());
        let mut entry = AgentEntry {
            id: known.id.into(),
            name: known.name.into(),
            command: known.command.into(),
            args: known.args.iter().map(|s| s.to_string()).collect(),
            env: BTreeMap::new(),
            secrets: BTreeMap::new(),
            use_mise: true,
            installed: found.is_some(),
            version: found,
            mise_line: Some(format!("\"{}\" = \"latest\"", known.mise_tools[0])),
            builtin: true,
        };
        if let Some(o) = config.agents.get(known.id) {
            apply_override(&mut entry, o, tools);
        }
        agents.push(entry);
    }
    for (id, o) in &config.agents {
        if KNOWN_AGENTS.iter().any(|k| k.id == id) {
            continue;
        }
        let mut entry = AgentEntry {
            id: id.clone(),
            name: id.clone(),
            command: id.clone(),
            args: Vec::new(),
            env: BTreeMap::new(),
            secrets: BTreeMap::new(),
            use_mise: true,
            installed: true,
            version: None,
            mise_line: None,
            builtin: false,
        };
        apply_override(&mut entry, o, tools);
        agents.push(entry);
    }

    let default_agent = config
        .agent
        .default
        .clone()
        .or_else(|| agents.iter().find(|a| a.installed).map(|a| a.id.clone()));
    // Installed agents first, keeping table order otherwise.
    agents.sort_by_key(|a| !a.installed);
    Discovery {
        agents,
        default_agent,
        mise_bin: None,
        mise_error: None,
        mise_untrusted: false,
    }
}

fn apply_override(
    entry: &mut AgentEntry,
    o: &crate::config::AgentOverride,
    tools: &HashMap<String, String>,
) {
    if let Some(v) = &o.name {
        entry.name = v.clone();
    }
    if let Some(v) = &o.command {
        entry.command = v.clone();
    }
    if let Some(v) = &o.args {
        entry.args = v.clone();
    }
    if let Some(v) = o.use_mise {
        entry.use_mise = v;
    }
    if let Some(tool) = &o.mise_tool {
        entry.version = tools.get(tool).cloned();
        entry.installed = entry.version.is_some();
        entry.mise_line = Some(format!("\"{tool}\" = \"latest\""));
    }
    entry.env.extend(o.env.clone());
    entry.secrets.extend(o.secrets.clone());
}

#[cfg(test)]
mod tests {
    use super::*;

    const LS: &str = r#"{
      "node": [{"version": "24.22.1", "installed": true, "active": true}],
      "npm:@agentclientprotocol/claude-agent-acp": [{"version": "0.98.0", "installed": true}],
      "npm:@google/gemini-cli": [{"version": "0.65.0", "installed": false}]
    }"#;

    #[test]
    fn matches_installed_agents() {
        let tools = parse_mise_ls(LS.as_bytes()).unwrap();
        let d = build(&VaultConfig::default(), &tools);
        let claude = d
            .agents
            .iter()
            .find(|a| a.id == "claude-agent-acp")
            .unwrap();
        assert!(claude.installed);
        assert_eq!(claude.version.as_deref(), Some("0.98.0"));
        let gemini = d.agents.iter().find(|a| a.id == "gemini").unwrap();
        assert!(!gemini.installed);
        assert_eq!(d.agents[0].id, "claude-agent-acp");
        assert_eq!(d.default_agent.as_deref(), Some("claude-agent-acp"));
        let junie = d.agents.iter().find(|a| a.id == "junie").unwrap();
        assert_eq!(junie.args, vec!["--acp=true"]);
        assert_eq!(
            junie.mise_line.as_deref(),
            Some("\"npm:@jetbrains/junie-cli\" = \"latest\"")
        );
    }

    #[test]
    fn config_adds_and_overrides() {
        let cfg: VaultConfig = toml::from_str(
            r#"
            [agent]
            default = "junie"
            [agents.junie]
            args = ["--acp=true", "--model", "x"]
            [agents.local]
            name = "Local model"
            command = "ollama-acp"
            use_mise = false
            "#,
        )
        .unwrap();
        let d = build(&cfg, &HashMap::new());
        let junie = d.agents.iter().find(|a| a.id == "junie").unwrap();
        assert_eq!(junie.args.len(), 3);
        let local = d.agents.iter().find(|a| a.id == "local").unwrap();
        assert!(local.installed && !local.use_mise && !local.builtin);
        assert_eq!(d.default_agent.as_deref(), Some("junie"));
    }
}
