//! Secrets from the system keyring, injected as env vars at spawn time.
//! Never written to the vault.

use std::collections::BTreeMap;

/// Keyring service name for all Adjutant secrets.
#[allow(dead_code)] // used by Jira sync (Phase 2)
pub const SERVICE: &str = "adjutant";

/// Resolves `ENV_VAR -> keyring entry` pairs; missing entries are skipped.
pub fn resolve(secrets: &BTreeMap<String, String>) -> BTreeMap<String, String> {
    secrets
        .iter()
        .filter_map(|(var, entry)| get(entry).map(|v| (var.clone(), v)))
        .collect()
}

pub fn get(_entry: &str) -> Option<String> {
    // Keyring integration lands with Jira sync (Phase 2).
    None
}
