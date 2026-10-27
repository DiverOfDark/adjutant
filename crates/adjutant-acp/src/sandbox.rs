//! File-access sandbox for ACP `fs/*` requests.
//!
//! The agent may only read and write inside the configured roots (the vault,
//! which includes `repos/`). Paths are resolved before checking, so `..` and
//! symlinks cannot escape a root.

use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone)]
pub struct Sandbox {
    roots: Vec<PathBuf>,
}

#[derive(Debug, thiserror::Error)]
pub enum SandboxError {
    #[error("path must be absolute: {0}")]
    NotAbsolute(PathBuf),
    #[error("path is outside the vault sandbox: {0}")]
    Outside(PathBuf),
}

impl Sandbox {
    /// Creates a sandbox. Roots that exist are canonicalized.
    pub fn new(roots: impl IntoIterator<Item = PathBuf>) -> Self {
        let roots = roots
            .into_iter()
            .map(|r| dunce::canonicalize(&r).unwrap_or(r))
            .collect();
        Self { roots }
    }

    pub fn roots(&self) -> &[PathBuf] {
        &self.roots
    }

    /// Resolves `path` and returns it if it lies within one of the roots.
    ///
    /// Works for files that do not exist yet: the longest existing ancestor
    /// is canonicalized and the remaining components are appended lexically.
    pub fn check(&self, path: &Path) -> Result<PathBuf, SandboxError> {
        if !path.is_absolute() {
            return Err(SandboxError::NotAbsolute(path.to_path_buf()));
        }
        let resolved = resolve(path);
        if self.roots.iter().any(|root| resolved.starts_with(root)) {
            Ok(resolved)
        } else {
            Err(SandboxError::Outside(path.to_path_buf()))
        }
    }
}

fn resolve(path: &Path) -> PathBuf {
    let normalized = normalize_lexically(path);
    let mut existing = normalized.as_path();
    let mut tail = Vec::new();
    loop {
        if let Ok(canon) = dunce::canonicalize(existing) {
            let mut out = canon;
            for part in tail.iter().rev() {
                out.push(part);
            }
            return out;
        }
        match (existing.parent(), existing.file_name()) {
            (Some(parent), Some(name)) => {
                tail.push(name.to_os_string());
                existing = parent;
            }
            _ => return normalized,
        }
    }
}

fn normalize_lexically(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_inside_and_new_files() {
        let dir = tempfile::tempdir().unwrap();
        let sb = Sandbox::new([dir.path().to_path_buf()]);
        assert!(sb.check(&dir.path().join("notes/new.md")).is_ok());
        assert!(sb.check(&dir.path().join("a/../b.md")).is_ok());
    }

    #[test]
    fn rejects_escapes() {
        let dir = tempfile::tempdir().unwrap();
        let sb = Sandbox::new([dir.path().join("vault")]);
        std::fs::create_dir(dir.path().join("vault")).unwrap();
        assert!(sb.check(&dir.path().join("vault/../secret")).is_err());
        assert!(sb.check(Path::new("relative.md")).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_escape() {
        let dir = tempfile::tempdir().unwrap();
        let vault = dir.path().join("vault");
        std::fs::create_dir(&vault).unwrap();
        std::os::unix::fs::symlink(dir.path(), vault.join("link")).unwrap();
        let sb = Sandbox::new([vault.clone()]);
        assert!(sb.check(&vault.join("link/outside.md")).is_err());
    }
}
