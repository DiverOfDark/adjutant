//! Adjutant's ACP layer.
//!
//! This is the only crate that depends on `agent-client-protocol`. The rest of
//! the app talks to agents through [`AgentHandle`] and receives
//! [`AgentEvent`]s, so upgrading the pinned protocol crate stays local.

mod client;
mod events;
mod launch;
mod sandbox;
mod terminal;

pub use client::{AgentHandle, AgentInfo, PromptPart};
pub use events::{AgentEvent, AskAlways, EventSink, PermissionPolicy, PolicyDecision};
pub use launch::AgentLaunch;
pub use sandbox::{Sandbox, SandboxError};
