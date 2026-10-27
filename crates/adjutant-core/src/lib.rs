//! Adjutant's deterministic core. Nothing in here calls an agent: files,
//! tasks, search, agent discovery, Jira sync and scheduling are plain code,
//! so routine work never spends agent credits.

pub mod agents;
pub mod config;
pub mod jira;
pub mod scheduler;
pub mod search;
pub mod secrets;
pub mod tasks;
pub mod vault;
