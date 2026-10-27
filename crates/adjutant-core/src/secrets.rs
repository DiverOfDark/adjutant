//! Secrets in the OS credential store (Secret Service on Linux/BSD,
//! Keychain on macOS, Credential Manager on Windows) via `keyring` v4.
//!
//! Entries live under service [`SERVICE`] with the secret name as the
//! user/account. These calls block (they talk to D-Bus / the OS), so async
//! callers should wrap them in `spawn_blocking`.

use anyhow::{Context, anyhow};
use keyring::{Entry, Error};

pub const SERVICE: &str = "adjutant";

/// Jira Cloud API token for `[jira].email`.
pub const JIRA_TOKEN: &str = "jira-api-token";

/// Reads a secret; `Ok(None)` when no entry exists.
pub fn get_secret(name: &str) -> anyhow::Result<Option<String>> {
    match entry(name)?.get_password() {
        Ok(v) => Ok(Some(v)),
        Err(Error::NoEntry) => Ok(None),
        Err(e) => Err(describe(e)).with_context(|| format!("reading secret `{name}`")),
    }
}

/// Creates or replaces a secret.
pub fn set_secret(name: &str, value: &str) -> anyhow::Result<()> {
    entry(name)?
        .set_password(value)
        .map_err(describe)
        .with_context(|| format!("saving secret `{name}`"))
}

/// Deletes a secret; succeeds when it does not exist.
pub fn delete_secret(name: &str) -> anyhow::Result<()> {
    match entry(name)?.delete_credential() {
        Ok(()) | Err(Error::NoEntry) => Ok(()),
        Err(e) => Err(describe(e)).with_context(|| format!("deleting secret `{name}`")),
    }
}

fn entry(name: &str) -> anyhow::Result<Entry> {
    if name.is_empty() {
        return Err(anyhow!("secret name must not be empty"));
    }
    if let Err(e) = Entry::store_status() {
        return Err(unavailable(&e.to_string()));
    }
    Entry::new(SERVICE, name).map_err(describe)
}

fn describe(e: Error) -> anyhow::Error {
    match e {
        Error::NoDefaultStore => unavailable("no default credential store"),
        Error::NoStorageAccess(inner) => anyhow!(
            "the system keyring is locked or access was denied ({inner}); unlock it and retry"
        ),
        other => anyhow!("system keyring error: {other}"),
    }
}

fn unavailable(detail: &str) -> anyhow::Error {
    let hint = if cfg!(any(target_os = "macos", target_os = "windows")) {
        "the OS credential store could not be opened"
    } else {
        "no Secret Service provider is running (install/start GNOME Keyring or KWallet)"
    };
    anyhow!("system keyring unavailable: {hint} ({detail})")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_name_rejected() {
        assert!(get_secret("").is_err());
    }

    /// Touches the real OS keyring; run manually with `--ignored`.
    #[test]
    #[ignore]
    fn roundtrip_real_keyring() {
        let name = "adjutant-test-secret";
        set_secret(name, "s3cret").unwrap();
        assert_eq!(get_secret(name).unwrap().as_deref(), Some("s3cret"));
        delete_secret(name).unwrap();
        assert_eq!(get_secret(name).unwrap(), None);
        delete_secret(name).unwrap();
    }
}
