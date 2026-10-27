//! Watches the vault and emits `vault-changed` with vault-relative paths.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use notify::RecursiveMode;
use notify_debouncer_full::{DebounceEventResult, Debouncer, RecommendedCache, new_debouncer};
use tauri::{AppHandle, Emitter};

pub struct VaultWatcher {
    _debouncer: Debouncer<notify::RecommendedWatcher, RecommendedCache>,
    /// Paths the app itself just wrote; their next change is not re-emitted.
    own_writes: Arc<Mutex<HashMap<String, Instant>>>,
}

const IGNORED: &[&str] = &[".git", ".vault", "node_modules", "repos"];

impl VaultWatcher {
    pub fn start(app: AppHandle, vault: &Path) -> anyhow::Result<Self> {
        let root: PathBuf = vault.to_path_buf();
        let own_writes: Arc<Mutex<HashMap<String, Instant>>> = Arc::default();
        let own = own_writes.clone();
        let mut debouncer = new_debouncer(
            Duration::from_millis(300),
            None,
            move |result: DebounceEventResult| {
                let Ok(events) = result else { return };
                let mut paths: Vec<String> = events
                    .iter()
                    .flat_map(|e| e.paths.iter())
                    .filter_map(|p| p.strip_prefix(&root).ok())
                    .map(|p| p.to_string_lossy().replace('\\', "/"))
                    .filter(|p| {
                        !p.is_empty()
                            && !IGNORED
                                .iter()
                                .any(|i| p == i || p.starts_with(&format!("{i}/")))
                    })
                    .collect();
                {
                    let mut own = own.lock().unwrap();
                    own.retain(|_, t| t.elapsed() < Duration::from_secs(2));
                    paths.retain(|p| own.remove(p).is_none());
                }
                paths.sort();
                paths.dedup();
                if !paths.is_empty() {
                    let _ = app.emit("vault-changed", paths);
                }
            },
        )?;
        debouncer.watch(vault, RecursiveMode::Recursive)?;
        Ok(Self {
            _debouncer: debouncer,
            own_writes,
        })
    }

    pub fn ignore_next(&self, rel: &str) {
        self.own_writes
            .lock()
            .unwrap()
            .insert(rel.to_string(), Instant::now());
    }
}
