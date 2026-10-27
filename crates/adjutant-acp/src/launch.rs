//! How an agent process is started.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Everything needed to spawn one ACP agent process.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentLaunch {
    /// Stable id, e.g. `junie` or `claude-agent-acp`.
    pub id: String,
    /// Executable name or path, e.g. `junie`.
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// Extra environment (secrets from the keyring are injected here).
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// Working directory; the vault root.
    pub cwd: PathBuf,
    /// Wrap the command as `mise exec -- <program> <args>` so the agent's PATH
    /// contains exactly the tools pinned in the vault's `mise.toml`.
    #[serde(default = "default_true")]
    pub use_mise: bool,
    /// Path to the mise binary; `mise` on PATH when unset.
    #[serde(default)]
    pub mise_bin: Option<PathBuf>,
}

fn default_true() -> bool {
    true
}

impl AgentLaunch {
    /// Builds a command that runs `program args` in the launch environment.
    /// Used for the agent itself and for terminals the agent opens.
    pub fn command_for(&self, program: &str, args: &[String]) -> tokio::process::Command {
        let mut cmd = if self.use_mise {
            let mise = self
                .mise_bin
                .clone()
                .unwrap_or_else(|| PathBuf::from("mise"));
            let mut c = tokio::process::Command::new(mise);
            c.arg("exec").arg("--").arg(program).args(args);
            c
        } else {
            let mut c = tokio::process::Command::new(program);
            c.args(args);
            c
        };
        cmd.current_dir(&self.cwd).envs(&self.env);
        cmd
    }

    pub fn agent_command(&self) -> tokio::process::Command {
        self.command_for(&self.program, &self.args)
    }

    /// Human-readable command line, for logs and the UI.
    pub fn display(&self) -> String {
        let mut parts = Vec::new();
        if self.use_mise {
            parts.push("mise exec --".to_string());
        }
        parts.push(self.program.clone());
        parts.extend(self.args.iter().cloned());
        parts.join(" ")
    }
}
