//! `.vault/config.toml`: agents, permissions, Jira, schedules, hotkeys.
//!
//! Every section is optional; a vault without a config file works.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const CONFIG_PATH: &str = ".vault/config.toml";

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct VaultConfig {
    pub agent: AgentSettings,
    /// `[agents.<id>]` add agents or override built-in launch settings.
    pub agents: BTreeMap<String, AgentOverride>,
    pub permissions: Permissions,
    /// `[folders."people"]` per-folder agent pinning (P1).
    pub folders: BTreeMap<String, FolderRule>,
    pub jira: Option<JiraConfig>,
    #[serde(rename = "schedules")]
    pub schedules: Vec<ScheduleConfig>,
    pub hotkeys: Hotkeys,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct AgentSettings {
    /// Default agent id, e.g. `junie`.
    pub default: Option<String>,
    /// Explicit path to the mise binary.
    pub mise_bin: Option<PathBuf>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct AgentOverride {
    pub name: Option<String>,
    pub command: Option<String>,
    pub args: Option<Vec<String>>,
    /// mise tool key that provides the agent, e.g. `npm:@jetbrains/junie-cli`.
    /// When unset for a custom agent, it is assumed to be installed.
    pub mise_tool: Option<String>,
    pub use_mise: Option<bool>,
    pub env: BTreeMap<String, String>,
    /// Env var name -> keyring entry name; injected at spawn time.
    pub secrets: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Permissions {
    /// "Always allow" rules, written when the user picks "Always allow".
    pub always_allow: Vec<PermissionRule>,
    /// Tool kinds scheduled (unattended) agent jobs may use without asking.
    /// Everything else is rejected; the app writes job output itself.
    #[serde(default = "default_job_allow")]
    pub job_allow: Vec<String>,
}

fn default_job_allow() -> Vec<String> {
    ["read", "search", "think", "fetch", "execute"]
        .map(String::from)
        .to_vec()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PermissionRule {
    pub agent: String,
    /// ACP tool kind (`read`, `edit`, `execute`, ...) or `*`.
    pub tool: String,
}

impl Default for Permissions {
    fn default() -> Self {
        Self {
            always_allow: Vec::new(),
            job_allow: default_job_allow(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct FolderRule {
    /// Agent id, or `none` to hide the folder from agents.
    pub agent: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct JiraConfig {
    /// e.g. `https://example.atlassian.net`
    pub base_url: String,
    /// Atlassian account email (Jira Cloud basic auth).
    pub email: String,
    /// Named JQL queries; `mine` is shown in the My Jira panel.
    pub queries: BTreeMap<String, String>,
    /// Sync interval in minutes (default 15).
    pub sync_minutes: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ScheduleConfig {
    pub id: String,
    pub name: Option<String>,
    /// Cron expression with seconds optional, e.g. `0 17 * * FRI`.
    pub cron: String,
    #[serde(flatten)]
    pub job: JobConfig,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum JobConfig {
    JiraSync,
    TaskArchive,
    Agent {
        /// Agent id; the vault default when unset.
        agent: Option<String>,
        prompt: String,
        /// Output file pattern relative to the vault, e.g. `reports/{week}.md`.
        output: Option<String>,
        /// Whether a run replaces the output file or appends a dated section.
        #[serde(default)]
        output_mode: OutputMode,
    },
}

/// How a scheduled agent job writes its output file.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum OutputMode {
    /// Overwrite the file with the latest run's output.
    #[default]
    Replace,
    /// Append a `## <date>` section to the file.
    Append,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Hotkeys {
    pub capture: String,
}

impl Default for Hotkeys {
    fn default() -> Self {
        Self {
            capture: "CommandOrControl+Shift+Space".into(),
        }
    }
}

fn default_true() -> bool {
    true
}

impl VaultConfig {
    pub fn load(vault: &Path) -> anyhow::Result<Self> {
        let path = vault.join(CONFIG_PATH);
        match std::fs::read_to_string(&path) {
            Ok(text) => {
                toml::from_str(&text).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.into()),
        }
    }

    /// Appends an "always allow" rule, preserving the rest of the file
    /// (comments included) by editing the TOML document in place.
    pub fn add_always_allow(vault: &Path, rule: &PermissionRule) -> anyhow::Result<()> {
        let path = vault.join(CONFIG_PATH);
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let mut doc: toml_edit::DocumentMut = text.parse()?;
        let perms = doc
            .entry("permissions")
            .or_insert_with(|| toml_edit::Item::Table(toml_edit::Table::new()));
        let arr = perms
            .as_table_like_mut()
            .ok_or_else(|| anyhow::anyhow!("[permissions] is not a table"))?
            .entry("always_allow")
            .or_insert_with(|| toml_edit::Item::ArrayOfTables(Default::default()));
        let arr = arr
            .as_array_of_tables_mut()
            .ok_or_else(|| anyhow::anyhow!("permissions.always_allow must be [[...]]"))?;
        let exists = arr.iter().any(|t| {
            t.get("agent").and_then(|v| v.as_str()) == Some(&rule.agent)
                && t.get("tool").and_then(|v| v.as_str()) == Some(&rule.tool)
        });
        if !exists {
            let mut t = toml_edit::Table::new();
            t["agent"] = toml_edit::value(&rule.agent);
            t["tool"] = toml_edit::value(&rule.tool);
            arr.push(t);
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, doc.to_string())?;
        Ok(())
    }

    /// Writes the `[jira]` section, keeping the rest of the file intact.
    pub fn set_jira(vault: &Path, jira: &JiraConfig) -> anyhow::Result<()> {
        let path = vault.join(CONFIG_PATH);
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let mut doc: toml_edit::DocumentMut = text.parse()?;
        let mut table = toml_edit::Table::new();
        table["base_url"] = toml_edit::value(jira.base_url.trim_end_matches('/'));
        table["email"] = toml_edit::value(&jira.email);
        if let Some(m) = jira.sync_minutes {
            table["sync_minutes"] = toml_edit::value(m as i64);
        }
        let mut queries = toml_edit::Table::new();
        for (name, jql) in &jira.queries {
            queries[name.as_str()] = toml_edit::value(jql);
        }
        table["queries"] = toml_edit::Item::Table(queries);
        doc["jira"] = toml_edit::Item::Table(table);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, doc.to_string())?;
        Ok(())
    }

    /// Folders hidden from agents (`[folders."x"] agent = "none"`).
    pub fn hidden_folders(&self) -> Vec<String> {
        self.folders
            .iter()
            .filter(|(_, r)| r.agent.as_deref() == Some("none"))
            .map(|(f, _)| f.trim_matches('/').to_string())
            .collect()
    }

    /// The agent pinned to the folder containing `rel`, if any (longest match).
    pub fn folder_agent(&self, rel: &str) -> Option<&str> {
        self.folders
            .iter()
            .filter(|(f, _)| {
                let f = f.trim_matches('/');
                rel == f || rel.starts_with(&format!("{f}/"))
            })
            .max_by_key(|(f, _)| f.len())
            .and_then(|(_, r)| r.agent.as_deref())
    }

    pub fn is_always_allowed(&self, agent: &str, tool_kind: &str) -> bool {
        self.permissions
            .always_allow
            .iter()
            .any(|r| r.agent == agent && (r.tool == "*" || r.tool == tool_kind))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_full_config() {
        let cfg: VaultConfig = toml::from_str(
            r#"
            [agent]
            default = "junie"

            [agents.ollama]
            command = "my-ollama-acp"
            use_mise = false

            [[permissions.always_allow]]
            agent = "junie"
            tool = "read"

            [folders."people"]
            agent = "none"

            [jira]
            base_url = "https://example.atlassian.net"
            email = "me@example.com"
            queries = { mine = "assignee = currentUser() AND resolution = Unresolved" }

            [[schedules]]
            id = "weekly-report"
            cron = "0 0 16 * * FRI"
            kind = "agent"
            prompt = "Write the weekly report"
            output = "reports/{week}.md"

            [[schedules]]
            id = "eod-log"
            cron = "0 18 * * MON-FRI"
            kind = "agent"
            prompt = "Write the end-of-day log"
            output = "logs/{month}.md"
            output_mode = "append"

            [[schedules]]
            id = "jira"
            cron = "0 */15 * * * *"
            kind = "jira-sync"
            "#,
        )
        .unwrap();
        assert_eq!(cfg.agent.default.as_deref(), Some("junie"));
        assert!(cfg.is_always_allowed("junie", "read"));
        assert!(!cfg.is_always_allowed("junie", "edit"));
        assert_eq!(cfg.schedules.len(), 3);
        assert!(matches!(
            cfg.schedules[0].job,
            JobConfig::Agent {
                output_mode: OutputMode::Replace,
                ..
            }
        ));
        assert!(matches!(
            cfg.schedules[1].job,
            JobConfig::Agent {
                output_mode: OutputMode::Append,
                ..
            }
        ));
        assert!(matches!(cfg.schedules[2].job, JobConfig::JiraSync));
        assert_eq!(cfg.hotkeys.capture, "CommandOrControl+Shift+Space");
    }

    #[test]
    fn add_always_allow_keeps_comments() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".vault")).unwrap();
        std::fs::write(
            dir.path().join(CONFIG_PATH),
            "# keep me\n[agent]\ndefault = \"junie\"\n",
        )
        .unwrap();
        let rule = PermissionRule {
            agent: "junie".into(),
            tool: "read".into(),
        };
        VaultConfig::add_always_allow(dir.path(), &rule).unwrap();
        VaultConfig::add_always_allow(dir.path(), &rule).unwrap();
        let text = std::fs::read_to_string(dir.path().join(CONFIG_PATH)).unwrap();
        assert!(text.contains("# keep me"));
        assert_eq!(text.matches("[[permissions.always_allow]]").count(), 1);
        assert!(
            VaultConfig::load(dir.path())
                .unwrap()
                .is_always_allowed("junie", "read")
        );
    }
}
