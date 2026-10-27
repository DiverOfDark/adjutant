//! The vault: a plain folder of Markdown notes.
//!
//! Every path crossing this API is vault-relative and uses `/` separators on
//! every platform. [`resolve`] is the single gate that turns such a path into
//! a filesystem path and refuses anything that would escape the vault.

use std::collections::HashMap;
use std::fs;
use std::io::Write as _;
use std::path::{Component, Path, PathBuf};
use std::sync::LazyLock;

use anyhow::{Context as _, anyhow, bail};
use regex::Regex;
use serde::{Deserialize, Serialize};

/// Directories never shown or scanned: ".git", ".vault", "node_modules", and
/// "repos" (repos are listed separately).
pub const HIDDEN_DIRS: &[&str] = &[".git", ".vault", "node_modules", "repos"];

/// File extensions shown in the file tree.
pub const TREE_EXTENSIONS: &[&str] = &[
    "md", "markdown", "txt", "toml", "json", "yaml", "yml", "csv",
];

/// Where [`trash`] moves deleted entries.
pub const TRASH_DIR: &str = ".vault/trash";

/// Max length (in chars) of a line excerpt in backlinks and search hits.
pub(crate) const MAX_LINE_CHARS: usize = 300;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TreeNode {
    pub name: String,
    /// Vault-relative path; `""` for the root.
    pub path: String,
    pub is_dir: bool,
    pub children: Vec<TreeNode>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Backlink {
    pub file: String,
    /// 0-based line number.
    pub line: usize,
    /// The line, trimmed, max 300 chars.
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RepoInfo {
    pub name: String,
    pub path: String,
    pub remote: Option<String>,
    /// mtime of `.git/FETCH_HEAD`, else `.git/HEAD`.
    pub last_pulled: Option<chrono::DateTime<chrono::Utc>>,
}

// ---------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------

/// Whether a directory at vault-relative `rel` (with file name `name`) is
/// excluded from the tree, note lists and search. `repos` is only special at
/// the vault root; the others are hidden at any depth.
pub(crate) fn is_hidden_dir(rel: &str, name: &str) -> bool {
    if name == "repos" {
        return rel == "repos";
    }
    HIDDEN_DIRS.contains(&name)
}

fn has_extension(name: &str, exts: &[&str]) -> bool {
    Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| exts.iter().any(|x| x.eq_ignore_ascii_case(e)))
}

fn is_note(name: &str) -> bool {
    has_extension(name, &["md"])
}

/// Normalizes a vault-relative path to `a/b/c` form, rejecting absolute
/// paths and `..`. `""` and `"."` normalize to `""` (the vault root).
pub(crate) fn clean_rel(rel: &str) -> anyhow::Result<String> {
    let unified = rel.replace('\\', "/");
    if unified.starts_with('/') {
        bail!("absolute paths are not allowed: {rel}");
    }
    let mut parts = Vec::new();
    for comp in Path::new(&unified).components() {
        match comp {
            Component::Normal(p) => {
                let s = p
                    .to_str()
                    .ok_or_else(|| anyhow!("path is not valid UTF-8: {rel}"))?;
                if s.contains(':') && cfg!(windows) {
                    bail!("invalid path: {rel}");
                }
                parts.push(s.to_string());
            }
            Component::CurDir => {}
            Component::ParentDir => bail!("'..' is not allowed in vault paths: {rel}"),
            Component::RootDir | Component::Prefix(_) => {
                bail!("absolute paths are not allowed: {rel}")
            }
        }
    }
    Ok(parts.join("/"))
}

/// Resolve a vault-relative path safely: reject absolute paths, "..", and
/// anything that canonicalizes outside the vault (symlinks). Works for
/// not-yet-existing files.
pub fn resolve(vault: &Path, rel: &str) -> anyhow::Result<PathBuf> {
    let clean = clean_rel(rel)?;
    let root = dunce::canonicalize(vault)
        .with_context(|| format!("vault not found: {}", vault.display()))?;
    let full = if clean.is_empty() {
        root.clone()
    } else {
        root.join(&clean)
    };
    // Walk up to the deepest entry that exists (as a link or a file) and make
    // sure it canonicalizes inside the vault. A dangling symlink fails to
    // canonicalize and is rejected, so writes can't follow it outward.
    let mut probe = full.as_path();
    loop {
        if probe.symlink_metadata().is_ok() {
            let canon =
                dunce::canonicalize(probe).with_context(|| format!("cannot resolve {rel}"))?;
            if !canon.starts_with(&root) {
                bail!("path escapes the vault: {rel}");
            }
            break;
        }
        match probe.parent() {
            Some(parent) => probe = parent,
            None => bail!("path escapes the vault: {rel}"),
        }
    }
    Ok(full)
}

/// Converts an absolute path under `root` back into a vault-relative path.
pub(crate) fn rel_of(root: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(root).ok()?;
    let parts: Option<Vec<&str>> = rel.components().map(|c| c.as_os_str().to_str()).collect();
    Some(parts?.join("/"))
}

/// Cuts a line to [`MAX_LINE_CHARS`] characters on a char boundary.
pub(crate) fn truncate_chars(s: &str, max: usize) -> &str {
    match s.char_indices().nth(max) {
        Some((i, _)) => &s[..i],
        None => s,
    }
}

// ---------------------------------------------------------------------------
// Tree and listing
// ---------------------------------------------------------------------------

/// File tree: dirs first then files, each sorted case-insensitively; only
/// directories and files with extensions md, markdown, txt, toml, json, yaml,
/// yml, csv (others omitted); hidden dot-entries omitted. Empty dirs included.
pub fn tree(vault: &Path) -> anyhow::Result<TreeNode> {
    let root = resolve(vault, "")?;
    let name = root
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_string();
    Ok(TreeNode {
        name,
        path: String::new(),
        is_dir: true,
        children: tree_children(&root, "")?,
    })
}

fn tree_children(dir: &Path, rel: &str) -> anyhow::Result<Vec<TreeNode>> {
    let mut dirs = Vec::new();
    let mut files = Vec::new();
    for entry in fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))? {
        let entry = entry?;
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if name.starts_with('.') {
            continue;
        }
        let child_rel = join_rel(rel, &name);
        let ft = entry.file_type()?;
        if ft.is_symlink() {
            // Show symlinked files that stay inside the vault; never descend
            // into symlinked directories (loops, escapes).
            let path = entry.path();
            match fs::metadata(&path) {
                Ok(m) if m.is_file() && has_extension(&name, TREE_EXTENSIONS) => {
                    files.push(leaf(name, child_rel));
                }
                _ => {}
            }
        } else if ft.is_dir() {
            if is_hidden_dir(&child_rel, &name) {
                continue;
            }
            let children = tree_children(&entry.path(), &child_rel)?;
            dirs.push(TreeNode {
                name,
                path: child_rel,
                is_dir: true,
                children,
            });
        } else if ft.is_file() && has_extension(&name, TREE_EXTENSIONS) {
            files.push(leaf(name, child_rel));
        }
    }
    let key = |n: &TreeNode| (n.name.to_lowercase(), n.name.clone());
    dirs.sort_by_key(key);
    files.sort_by_key(key);
    dirs.extend(files);
    Ok(dirs)
}

fn leaf(name: String, path: String) -> TreeNode {
    TreeNode {
        name,
        path,
        is_dir: false,
        children: Vec::new(),
    }
}

fn join_rel(base: &str, name: &str) -> String {
    if base.is_empty() {
        name.to_string()
    } else {
        format!("{base}/{name}")
    }
}

/// All note paths (.md) excluding HIDDEN_DIRS and dot-entries, sorted. Used
/// for `[[` autocomplete.
pub fn list_notes(vault: &Path) -> anyhow::Result<Vec<String>> {
    let root = resolve(vault, "")?;
    let mut out = Vec::new();
    collect_notes(&root, "", &mut out)?;
    out.sort();
    Ok(out)
}

fn collect_notes(dir: &Path, rel: &str, out: &mut Vec<String>) -> anyhow::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if name.starts_with('.') {
            continue;
        }
        let child_rel = join_rel(rel, &name);
        let ft = entry.file_type()?;
        if ft.is_dir() {
            if !is_hidden_dir(&child_rel, &name) {
                collect_notes(&entry.path(), &child_rel, out)?;
            }
        } else if is_note(&name)
            && (ft.is_file() || fs::metadata(entry.path()).is_ok_and(|m| m.is_file()))
        {
            out.push(child_rel);
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Note I/O
// ---------------------------------------------------------------------------

pub fn read_note(vault: &Path, rel: &str) -> anyhow::Result<String> {
    let path = resolve(vault, rel)?;
    fs::read_to_string(&path).with_context(|| format!("reading {rel}"))
}

/// Atomic write (temp file in same dir + rename), creating parent dirs.
pub fn write_note(vault: &Path, rel: &str, content: &str) -> anyhow::Result<()> {
    let path = resolve(vault, rel)?;
    if clean_rel(rel)?.is_empty() {
        bail!("cannot write to the vault root");
    }
    atomic_write(&path, content.as_bytes()).with_context(|| format!("writing {rel}"))
}

fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().ok_or(std::io::ErrorKind::InvalidInput)?;
    fs::create_dir_all(dir)?;
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("note");
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or_default();
    let mut attempt = 0u32;
    let (tmp, mut file) = loop {
        let tmp = dir.join(format!(
            ".{name}.{}-{nanos}-{attempt}.tmp",
            std::process::id()
        ));
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp)
        {
            Ok(f) => break (tmp, f),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && attempt < 100 => {
                attempt += 1;
            }
            Err(e) => return Err(e),
        }
    };
    let result = file
        .write_all(bytes)
        .and_then(|_| file.sync_all())
        .and_then(|_| {
            drop(file);
            fs::rename(&tmp, path)
        });
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

/// Create a new note; error if it exists. If rel has no extension, append
/// ".md". Returns final rel path.
pub fn create_note(vault: &Path, rel: &str, content: &str) -> anyhow::Result<String> {
    let mut clean = clean_rel(rel)?;
    if clean.is_empty() {
        bail!("note path is empty");
    }
    let file_name = clean.rsplit('/').next().unwrap_or_default();
    if Path::new(file_name).extension().is_none() {
        clean.push_str(".md");
    }
    let path = resolve(vault, &clean)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                anyhow!("{clean} already exists")
            } else {
                anyhow!("creating {clean}: {e}")
            }
        })?;
    file.write_all(content.as_bytes())?;
    file.sync_all()?;
    Ok(clean)
}

pub fn create_dir(vault: &Path, rel: &str) -> anyhow::Result<()> {
    if clean_rel(rel)?.is_empty() {
        bail!("folder path is empty");
    }
    let path = resolve(vault, rel)?;
    fs::create_dir_all(&path).with_context(|| format!("creating folder {rel}"))
}

// ---------------------------------------------------------------------------
// Wikilinks
// ---------------------------------------------------------------------------

/// `[[target#heading|alias]]`: group 1 = target, group 2 = `#heading|alias`
/// suffix (both parts optional).
static WIKILINK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\[\[([^\[\]|#\n]+)((?:#[^\[\]|\n]*)?(?:\|[^\[\]\n]*)?)\]\]").unwrap()
});

fn strip_md(s: &str) -> &str {
    if s.len() >= 3 && s[s.len() - 3..].eq_ignore_ascii_case(".md") {
        &s[..s.len() - 3]
    } else {
        s
    }
}

/// Wikilink target -> note path: "people/alex" -> "people/alex.md" if exists;
/// also matches by file stem anywhere ("alex" -> "people/alex.md", shortest
/// path wins); None if not found.
pub fn resolve_link(notes: &[String], target: &str) -> Option<String> {
    let target = target.trim();
    let target = target.split(['#', '|']).next().unwrap_or_default().trim();
    let target = target.trim_start_matches("./").trim_start_matches('/');
    let base = strip_md(target).trim_end_matches('/');
    if base.is_empty() {
        return None;
    }
    let exact = format!("{base}.md");
    if let Some(n) = notes.iter().find(|n| **n == exact) {
        return Some(n.clone());
    }
    if let Some(n) = notes.iter().find(|n| n.eq_ignore_ascii_case(&exact)) {
        return Some(n.clone());
    }
    // Suffix match: "alex" matches ".../alex.md", "team/alex" matches
    // ".../team/alex.md". Shortest path wins, then alphabetical.
    let suffix = format!("/{exact}").to_lowercase();
    notes
        .iter()
        .filter(|n| n.to_lowercase().ends_with(&suffix))
        .min_by(|a, b| a.len().cmp(&b.len()).then_with(|| a.cmp(b)))
        .cloned()
}

/// Lines in other notes linking to `rel` (match [[path-without-ext]],
/// [[stem]] when the stem resolves to rel, with optional |alias or #heading).
pub fn backlinks(vault: &Path, rel: &str) -> anyhow::Result<Vec<Backlink>> {
    let rel = clean_rel(rel)?;
    let root = resolve(vault, "")?;
    let notes = list_notes(vault)?;
    let mut out = Vec::new();
    for note in notes.iter().filter(|n| **n != rel) {
        let Ok(text) = fs::read_to_string(root.join(note)) else {
            continue;
        };
        for (i, line) in text.lines().enumerate() {
            let links_here = WIKILINK
                .captures_iter(line)
                .any(|c| resolve_link(&notes, &c[1]).as_deref() == Some(rel.as_str()));
            if links_here {
                out.push(Backlink {
                    file: note.clone(),
                    line: i,
                    text: truncate_chars(line.trim(), MAX_LINE_CHARS).to_string(),
                });
            }
        }
    }
    Ok(out)
}

/// Rewrites links in `text` whose target resolved (before the move) to a
/// moved note. Returns `None` when nothing changed.
fn rewrite_links(
    text: &str,
    old_notes: &[String],
    new_notes: &[String],
    moved: &HashMap<String, String>,
) -> Option<String> {
    let mut changed = false;
    let out = WIKILINK.replace_all(text, |c: &regex::Captures| {
        let target = &c[1];
        let suffix = &c[2];
        if let Some(old) = resolve_link(old_notes, target)
            && let Some(new) = moved.get(&old)
            && resolve_link(new_notes, target).as_ref() != Some(new)
        {
            changed = true;
            return format!("[[{}{suffix}]]", strip_md(new));
        }
        c[0].to_string()
    });
    changed.then(|| out.into_owned())
}

// ---------------------------------------------------------------------------
// Rename and trash
// ---------------------------------------------------------------------------

/// Rename/move a file or folder; error if target exists. When a .md note
/// moves (directly or inside a moved folder), rewrite `[[old/path]]`,
/// `[[old/path|alias]]` and `[[old/path#h]]` wikilinks (and stem links that
/// would no longer resolve to it) in all vault notes to the new path (without
/// .md). Returns number of files whose links were updated.
pub fn rename(vault: &Path, from: &str, to: &str) -> anyhow::Result<usize> {
    let from_rel = clean_rel(from)?;
    let to_rel = clean_rel(to)?;
    if from_rel.is_empty() || to_rel.is_empty() {
        bail!("cannot rename the vault root");
    }
    if from_rel == to_rel {
        return Ok(0);
    }
    if to_rel.starts_with(&format!("{from_rel}/")) {
        bail!("cannot move a folder into itself");
    }
    let src = resolve(vault, &from_rel)?;
    let dst = resolve(vault, &to_rel)?;
    let meta = src
        .symlink_metadata()
        .with_context(|| format!("{from_rel} does not exist"))?;
    // Case-only renames on case-insensitive filesystems report the target as
    // existing; allow those.
    if dst.symlink_metadata().is_ok() && !from_rel.eq_ignore_ascii_case(&to_rel) {
        bail!("{to_rel} already exists");
    }

    let old_notes = list_notes(vault)?;
    let mut moved = HashMap::new();
    if meta.is_dir() {
        let prefix = format!("{from_rel}/");
        for n in old_notes.iter().filter(|n| n.starts_with(&prefix)) {
            moved.insert(n.clone(), format!("{to_rel}/{}", &n[prefix.len()..]));
        }
    } else if is_note(&from_rel) && is_note(&to_rel) {
        moved.insert(from_rel.clone(), to_rel.clone());
    }

    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::rename(&src, &dst).with_context(|| format!("moving {from_rel} to {to_rel}"))?;

    if moved.is_empty() {
        return Ok(0);
    }
    let root = resolve(vault, "")?;
    let new_notes = list_notes(vault)?;
    let mut updated = 0;
    for note in &new_notes {
        let path = root.join(note);
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        if let Some(new_text) = rewrite_links(&text, &old_notes, &new_notes, &moved) {
            atomic_write(&path, new_text.as_bytes())
                .with_context(|| format!("updating links in {note}"))?;
            updated += 1;
        }
    }
    Ok(updated)
}

/// "Delete" = move into `.vault/trash/<timestamp>/<rel>` (never hard-delete
/// user notes).
pub fn trash(vault: &Path, rel: &str) -> anyhow::Result<()> {
    let clean = clean_rel(rel)?;
    if clean.is_empty() {
        bail!("cannot trash the vault root");
    }
    if clean == ".vault" || clean.starts_with(".vault/") {
        bail!("cannot trash Adjutant's own files: {clean}");
    }
    let src = resolve(vault, &clean)?;
    src.symlink_metadata()
        .with_context(|| format!("{clean} does not exist"))?;
    let root = resolve(vault, "")?;
    let stamp = chrono::Local::now()
        .format("%Y-%m-%dT%H-%M-%S%.3f")
        .to_string();
    let mut bucket = root.join(TRASH_DIR).join(&stamp);
    let mut n = 1;
    while bucket.join(&clean).symlink_metadata().is_ok() {
        bucket = root.join(TRASH_DIR).join(format!("{stamp}-{n}"));
        n += 1;
    }
    let dst = bucket.join(&clean);
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::rename(&src, &dst).with_context(|| format!("moving {clean} to trash"))
}

// ---------------------------------------------------------------------------
// Repos
// ---------------------------------------------------------------------------

/// Dirs in `repos/` containing `.git`; remote parsed from `.git/config`
/// `[remote "origin"]` url. Sorted by name.
pub fn list_repos(vault: &Path) -> anyhow::Result<Vec<RepoInfo>> {
    let dir = resolve(vault, "repos")?;
    let entries = match fs::read_dir(&dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    let mut out = Vec::new();
    for entry in entries {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        let git = entry.path().join(".git");
        if !git.is_dir() {
            continue;
        }
        let remote = fs::read_to_string(git.join("config"))
            .ok()
            .and_then(|c| origin_url(&c));
        let last_pulled = ["FETCH_HEAD", "HEAD"]
            .iter()
            .find_map(|f| fs::metadata(git.join(f)).and_then(|m| m.modified()).ok())
            .map(chrono::DateTime::<chrono::Utc>::from);
        out.push(RepoInfo {
            path: format!("repos/{name}"),
            name,
            remote,
            last_pulled,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

fn origin_url(config: &str) -> Option<String> {
    let mut in_origin = false;
    for line in config.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            let header: String = line.chars().filter(|c| !c.is_whitespace()).collect();
            in_origin = header == "[remote\"origin\"]";
        } else if in_origin
            && let Some((k, v)) = line.split_once('=')
            && k.trim() == "url"
        {
            return Some(v.trim().trim_matches('"').to_string());
        }
    }
    None
}

// ---------------------------------------------------------------------------
// Scaffold and templates
// ---------------------------------------------------------------------------

const SCAFFOLD_DIRS: &[&str] = &[
    "inbox",
    "people",
    "teams",
    "me",
    "log",
    "reports",
    "jira",
    "repos",
    "sources",
    "templates",
    "tasks/done",
    ".vault",
];

const SCAFFOLD_FILES: &[(&str, &str)] = &[
    ("AGENTS.md", include_str!("scaffold/AGENTS.md")),
    ("tasks.md", include_str!("scaffold/tasks.md")),
    ("mise.toml", include_str!("scaffold/mise.toml")),
    (".vault/config.toml", include_str!("scaffold/config.toml")),
    (".gitignore", include_str!("scaffold/gitignore")),
    (
        "templates/one-on-one.md",
        include_str!("scaffold/templates/one-on-one.md"),
    ),
    (
        "templates/meeting.md",
        include_str!("scaffold/templates/meeting.md"),
    ),
    (
        "templates/person.md",
        include_str!("scaffold/templates/person.md"),
    ),
    (
        "templates/decision.md",
        include_str!("scaffold/templates/decision.md"),
    ),
];

/// Is this an initialized Adjutant vault (has tasks.md or AGENTS.md or
/// .vault/)?
pub fn is_initialized(vault: &Path) -> bool {
    vault.join("tasks.md").is_file()
        || vault.join("AGENTS.md").is_file()
        || vault.join(".vault").is_dir()
}

/// Scaffold a new vault in place without overwriting existing files. Returns
/// list of created rel paths (directories first, then files).
pub fn scaffold(vault: &Path) -> anyhow::Result<Vec<String>> {
    let mut created = Vec::new();
    for dir in SCAFFOLD_DIRS {
        let path = resolve(vault, dir)?;
        if path.symlink_metadata().is_err() {
            fs::create_dir_all(&path).with_context(|| format!("creating {dir}"))?;
            created.push(dir.to_string());
        }
    }
    for (rel, content) in SCAFFOLD_FILES {
        let path = resolve(vault, rel)?;
        if path.symlink_metadata().is_ok() {
            continue;
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut f) => {
                f.write_all(content.as_bytes())?;
                created.push(rel.to_string());
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(anyhow!("creating {rel}: {e}")),
        }
    }
    Ok(created)
}

/// Template names (`templates/*.md` without `.md`), sorted.
pub fn list_templates(vault: &Path) -> anyhow::Result<Vec<String>> {
    let dir = resolve(vault, "templates")?;
    let entries = match fs::read_dir(&dir) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    let mut out = Vec::new();
    for entry in entries {
        let entry = entry?;
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if name.starts_with('.') || !is_note(&name) || !entry.path().is_file() {
            continue;
        }
        out.push(strip_md(&name).to_string());
    }
    out.sort_by_key(|n| (n.to_lowercase(), n.clone()));
    Ok(out)
}

/// Renders `templates/<name>.md`: replaces `{{date}}` (YYYY-MM-DD),
/// `{{title}}` and `{{time}}` (HH:MM).
pub fn render_template(
    vault: &Path,
    name: &str,
    title: &str,
    now: chrono::NaiveDateTime,
) -> anyhow::Result<String> {
    let name = strip_md(name.trim());
    if name.is_empty() || name.contains(['/', '\\']) || name.starts_with('.') {
        bail!("invalid template name: {name}");
    }
    let text = read_note(vault, &format!("templates/{name}.md"))
        .with_context(|| format!("template {name} not found"))?;
    Ok(text
        .replace("{{date}}", &now.format("%Y-%m-%d").to_string())
        .replace("{{time}}", &now.format("%H:%M").to_string())
        .replace("{{title}}", title))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn vault_with(files: &[(&str, &str)]) -> TempDir {
        let dir = TempDir::new().unwrap();
        for (rel, content) in files {
            let p = dir.path().join(rel);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, content).unwrap();
        }
        dir
    }

    fn read(dir: &TempDir, rel: &str) -> String {
        fs::read_to_string(dir.path().join(rel)).unwrap()
    }

    fn names(nodes: &[TreeNode]) -> Vec<&str> {
        nodes.iter().map(|n| n.name.as_str()).collect()
    }

    #[test]
    fn tree_filters_and_sorts() {
        let v = vault_with(&[
            ("b.md", ""),
            ("A.md", ""),
            ("c.txt", ""),
            ("image.png", ""),
            ("data.json", ""),
            (".hidden.md", ""),
            ("Zeta/note.md", ""),
            ("alpha/x.yml", ""),
            ("alpha/skip.exe", ""),
            (".git/config", ""),
            (".vault/config.toml", ""),
            ("repos/proj/README.md", ""),
            ("node_modules/x/index.md", ""),
            ("teams/repos/keep.md", ""),
        ]);
        fs::create_dir(v.path().join("empty")).unwrap();
        let t = tree(v.path()).unwrap();
        assert_eq!(t.path, "");
        assert!(t.is_dir);
        assert_eq!(
            names(&t.children),
            [
                "alpha",
                "empty",
                "teams",
                "Zeta",
                "A.md",
                "b.md",
                "c.txt",
                "data.json"
            ]
        );
        let alpha = &t.children[0];
        assert_eq!(alpha.path, "alpha");
        assert_eq!(names(&alpha.children), ["x.yml"]);
        assert_eq!(alpha.children[0].path, "alpha/x.yml");
        assert!(!alpha.children[0].is_dir);
        assert!(t.children[1].children.is_empty());
        // "repos" is only hidden at the root.
        assert_eq!(t.children[2].children[0].path, "teams/repos");
    }

    #[test]
    fn resolve_rejects_escapes() {
        let v = vault_with(&[("a.md", "")]);
        let root = dunce::canonicalize(v.path()).unwrap();
        assert_eq!(resolve(v.path(), "a.md").unwrap(), root.join("a.md"));
        assert_eq!(
            resolve(v.path(), "new/dir/b.md").unwrap(),
            root.join("new/dir/b.md")
        );
        assert_eq!(resolve(v.path(), "./x\\y.md").unwrap(), root.join("x/y.md"));
        assert_eq!(resolve(v.path(), "").unwrap(), root);
        assert!(resolve(v.path(), "/etc/passwd").is_err());
        assert!(resolve(v.path(), "../x.md").is_err());
        assert!(resolve(v.path(), "a/../../x.md").is_err());
        assert!(resolve(v.path(), "a/..").is_err());
        assert!(resolve(v.path(), "..\\x.md").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn resolve_rejects_symlink_escapes() {
        let outside = TempDir::new().unwrap();
        fs::write(outside.path().join("secret.md"), "s").unwrap();
        let v = vault_with(&[("in.md", "")]);
        std::os::unix::fs::symlink(outside.path(), v.path().join("out")).unwrap();
        std::os::unix::fs::symlink(outside.path().join("secret.md"), v.path().join("s.md"))
            .unwrap();
        std::os::unix::fs::symlink(outside.path().join("nope.md"), v.path().join("dangling.md"))
            .unwrap();
        std::os::unix::fs::symlink(v.path().join("in.md"), v.path().join("ok.md")).unwrap();
        assert!(resolve(v.path(), "out/secret.md").is_err());
        assert!(resolve(v.path(), "out/new.md").is_err());
        assert!(resolve(v.path(), "s.md").is_err());
        assert!(resolve(v.path(), "dangling.md").is_err());
        assert!(write_note(v.path(), "dangling.md", "x").is_err());
        assert!(!outside.path().join("nope.md").exists());
        assert!(resolve(v.path(), "ok.md").is_ok());
        // Symlinked dirs are not walked.
        let t = tree(v.path()).unwrap();
        assert!(!names(&t.children).contains(&"out"));
    }

    #[test]
    fn read_write_create() {
        let v = vault_with(&[]);
        write_note(v.path(), "a/b/c.md", "hello").unwrap();
        assert_eq!(read_note(v.path(), "a/b/c.md").unwrap(), "hello");
        write_note(v.path(), "a/b/c.md", "again").unwrap();
        assert_eq!(read_note(v.path(), "a/b/c.md").unwrap(), "again");
        // No temp files left behind.
        let entries: Vec<_> = fs::read_dir(v.path().join("a/b")).unwrap().collect();
        assert_eq!(entries.len(), 1);
        assert!(write_note(v.path(), "", "x").is_err());

        assert_eq!(
            create_note(v.path(), "people/sam", "# Sam").unwrap(),
            "people/sam.md"
        );
        assert_eq!(read(&v, "people/sam.md"), "# Sam");
        assert!(create_note(v.path(), "people/sam", "x").is_err());
        assert!(create_note(v.path(), "people/sam.md", "x").is_err());
        assert_eq!(read(&v, "people/sam.md"), "# Sam");
        assert_eq!(create_note(v.path(), "data.txt", "").unwrap(), "data.txt");
        assert!(create_note(v.path(), "../evil", "").is_err());

        create_dir(v.path(), "teams/platform").unwrap();
        assert!(v.path().join("teams/platform").is_dir());
        assert!(create_dir(v.path(), "").is_err());
    }

    #[test]
    fn resolve_link_by_path_and_stem() {
        let notes: Vec<String> = [
            "archive/b/alex.md",
            "people/alex.md",
            "people/sam.md",
            "Readme.md",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert_eq!(
            resolve_link(&notes, "people/alex").as_deref(),
            Some("people/alex.md")
        );
        assert_eq!(
            resolve_link(&notes, "people/alex.md").as_deref(),
            Some("people/alex.md")
        );
        assert_eq!(
            resolve_link(&notes, "alex").as_deref(),
            Some("people/alex.md")
        );
        assert_eq!(
            resolve_link(&notes, "b/alex").as_deref(),
            Some("archive/b/alex.md")
        );
        assert_eq!(
            resolve_link(&notes, " sam#1:1s|Sam ").as_deref(),
            Some("people/sam.md")
        );
        assert_eq!(resolve_link(&notes, "readme").as_deref(), Some("Readme.md"));
        assert_eq!(resolve_link(&notes, "nobody"), None);
        assert_eq!(resolve_link(&notes, "ex"), None);
        assert_eq!(resolve_link(&notes, ""), None);
    }

    #[test]
    fn rename_updates_links() {
        let v = vault_with(&[
            (
                "people/alex.md",
                "# Alex\nSee [[people/alex#Goals]] for self.",
            ),
            (
                "tasks.md",
                "- [ ] 1:1 [[people/alex]]\n- [ ] talk [[people/alex|Alex]] and [[people/alex#1:1s|1:1]]\n",
            ),
            (
                "teams/x.md",
                "by stem [[alex]] and unrelated [[people/sam]]\n",
            ),
            ("other.md", "nothing here [[people/alexander]]"),
        ]);
        let n = rename(v.path(), "people/alex.md", "people/former/alex-doe.md").unwrap();
        assert_eq!(n, 3);
        assert!(!v.path().join("people/alex.md").exists());
        assert_eq!(
            read(&v, "tasks.md"),
            "- [ ] 1:1 [[people/former/alex-doe]]\n- [ ] talk [[people/former/alex-doe|Alex]] and [[people/former/alex-doe#1:1s|1:1]]\n"
        );
        assert_eq!(
            read(&v, "teams/x.md"),
            "by stem [[people/former/alex-doe]] and unrelated [[people/sam]]\n"
        );
        assert_eq!(
            read(&v, "people/former/alex-doe.md"),
            "# Alex\nSee [[people/former/alex-doe#Goals]] for self."
        );
        assert_eq!(read(&v, "other.md"), "nothing here [[people/alexander]]");
    }

    #[test]
    fn rename_keeps_stem_links_that_still_resolve() {
        let v = vault_with(&[("people/alex.md", ""), ("a.md", "[[alex]] [[people/alex]]")]);
        let n = rename(v.path(), "people/alex.md", "archive/alex.md").unwrap();
        assert_eq!(n, 1);
        assert_eq!(read(&v, "a.md"), "[[alex]] [[archive/alex]]");
    }

    #[test]
    fn rename_folder_updates_links_and_rejects_existing() {
        let v = vault_with(&[
            ("teams/old/decision.md", ""),
            ("teams/old/notes.txt", ""),
            ("index.md", "[[teams/old/decision|D]]"),
            ("taken.md", ""),
        ]);
        assert!(rename(v.path(), "index.md", "taken.md").is_err());
        assert!(rename(v.path(), "missing.md", "x.md").is_err());
        assert!(rename(v.path(), "teams", "teams/inner").is_err());
        assert!(rename(v.path(), "index.md", "../x.md").is_err());
        let n = rename(v.path(), "teams/old", "teams/new").unwrap();
        assert_eq!(n, 1);
        assert_eq!(read(&v, "index.md"), "[[teams/new/decision|D]]");
        assert!(v.path().join("teams/new/notes.txt").exists());
        // Non-note files move without touching links.
        assert_eq!(
            rename(v.path(), "teams/new/notes.txt", "notes.txt").unwrap(),
            0
        );
    }

    #[test]
    fn trash_moves_into_vault_trash() {
        let v = vault_with(&[("people/alex.md", "keep me"), ("teams/p/a.md", "")]);
        trash(v.path(), "people/alex.md").unwrap();
        assert!(!v.path().join("people/alex.md").exists());
        let buckets: Vec<_> = fs::read_dir(v.path().join(TRASH_DIR))
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        assert_eq!(buckets.len(), 1);
        assert_eq!(
            fs::read_to_string(buckets[0].join("people/alex.md")).unwrap(),
            "keep me"
        );
        trash(v.path(), "teams").unwrap();
        assert!(!v.path().join("teams").exists());
        assert!(trash(v.path(), "missing.md").is_err());
        assert!(trash(v.path(), "").is_err());
        assert!(trash(v.path(), ".vault/trash").is_err());
        assert!(trash(v.path(), "../x").is_err());
        // Trash is invisible.
        assert!(list_notes(v.path()).unwrap().is_empty());
    }

    #[test]
    fn list_notes_skips_hidden() {
        let v = vault_with(&[
            ("b.md", ""),
            ("a/c.md", ""),
            ("a/d.txt", ""),
            (".vault/x.md", ""),
            ("repos/r/README.md", ""),
            ("node_modules/m.md", ""),
            (".obsidian/y.md", ""),
        ]);
        assert_eq!(list_notes(v.path()).unwrap(), ["a/c.md", "b.md"]);
    }

    #[test]
    fn backlinks_by_path_and_stem() {
        let v = vault_with(&[
            ("people/alex.md", "self [[people/alex]]"),
            (
                "tasks.md",
                "# Tasks\n- [ ] prep [[people/alex|Alex]]\n- [ ] other [[people/sam]]\n   - by stem [[alex#Goals]] twice [[alex]]  \n",
            ),
            ("log/2026-10.md", "met [[people/alex.md]]"),
            ("notes/archive/alex.md", "different alex"),
            ("misc.md", "[[notes/archive/alex]]"),
        ]);
        let bl = backlinks(v.path(), "people/alex.md").unwrap();
        assert_eq!(
            bl,
            vec![
                Backlink {
                    file: "log/2026-10.md".into(),
                    line: 0,
                    text: "met [[people/alex.md]]".into()
                },
                Backlink {
                    file: "tasks.md".into(),
                    line: 1,
                    text: "- [ ] prep [[people/alex|Alex]]".into()
                },
                Backlink {
                    file: "tasks.md".into(),
                    line: 3,
                    text: "- by stem [[alex#Goals]] twice [[alex]]".into()
                },
            ]
        );
        let bl = backlinks(v.path(), "notes/archive/alex.md").unwrap();
        assert_eq!(bl.len(), 1);
        assert_eq!(bl[0].file, "misc.md");
    }

    #[test]
    fn backlink_text_is_truncated() {
        let long = format!("[[a]] {}", "é".repeat(400));
        let v = vault_with(&[("a.md", ""), ("b.md", &long)]);
        let bl = backlinks(v.path(), "a.md").unwrap();
        assert_eq!(bl[0].text.chars().count(), 300);
    }

    #[test]
    fn list_repos_reads_git_metadata() {
        let v = vault_with(&[
            (
                "repos/svc/.git/config",
                "[core]\n\tbare = false\n[remote \"upstream\"]\n\turl = https://x/up.git\n[remote \"origin\"]\n\turl = git@github.com:acme/svc.git\n\tfetch = +refs/heads/*:refs/remotes/origin/*\n",
            ),
            ("repos/svc/.git/HEAD", "ref: refs/heads/main\n"),
            ("repos/svc/.git/FETCH_HEAD", ""),
            ("repos/local/.git/HEAD", "ref: refs/heads/main\n"),
            ("repos/notgit/file.txt", ""),
            ("repos/file.md", ""),
        ]);
        let repos = list_repos(v.path()).unwrap();
        assert_eq!(repos.len(), 2);
        assert_eq!(repos[0].name, "local");
        assert_eq!(repos[0].path, "repos/local");
        assert_eq!(repos[0].remote, None);
        assert!(repos[0].last_pulled.is_some());
        assert_eq!(repos[1].name, "svc");
        assert_eq!(
            repos[1].remote.as_deref(),
            Some("git@github.com:acme/svc.git")
        );
        let age = chrono::Utc::now() - repos[1].last_pulled.unwrap();
        assert!(age.num_minutes() < 5);

        let empty = vault_with(&[]);
        assert!(list_repos(empty.path()).unwrap().is_empty());
    }

    #[test]
    fn scaffold_is_idempotent_and_never_overwrites() {
        let v = vault_with(&[("tasks.md", "# My tasks\n- [ ] keep\n")]);
        assert!(is_initialized(v.path()));
        let created = scaffold(v.path()).unwrap();
        assert!(!created.contains(&"tasks.md".to_string()));
        for p in [
            "inbox",
            "tasks/done",
            ".vault",
            "AGENTS.md",
            "mise.toml",
            ".vault/config.toml",
            ".gitignore",
            "templates/one-on-one.md",
            "templates/person.md",
            "templates/meeting.md",
            "templates/decision.md",
        ] {
            assert!(created.contains(&p.to_string()), "missing {p}");
            assert!(v.path().join(p).exists());
        }
        assert_eq!(read(&v, "tasks.md"), "# My tasks\n- [ ] keep\n");
        fs::write(v.path().join("AGENTS.md"), "custom").unwrap();
        assert!(scaffold(v.path()).unwrap().is_empty());
        assert_eq!(read(&v, "AGENTS.md"), "custom");
        // .gitignore keeps repos out of git; mise pins junie.
        assert!(read(&v, ".gitignore").contains("repos/"));
        assert!(read(&v, "mise.toml").contains("\"npm:@jetbrains/junie-cli\" = \"latest\""));
    }

    #[test]
    fn scaffold_fresh_vault() {
        let v = vault_with(&[]);
        assert!(!is_initialized(v.path()));
        let created = scaffold(v.path()).unwrap();
        assert!(created.contains(&"tasks.md".to_string()));
        assert!(is_initialized(v.path()));
        let tasks = read(&v, "tasks.md");
        for s in ["## Now", "## Next", "## Waiting on", "## Someday"] {
            assert!(tasks.contains(s));
        }
    }

    #[test]
    fn scaffold_config_parses() {
        let v = vault_with(&[]);
        scaffold(v.path()).unwrap();
        let cfg = crate::config::VaultConfig::load(v.path()).unwrap();
        let ids: Vec<_> = cfg.schedules.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "jira-sync",
                "task-archive",
                "weekly-report",
                "end-of-day",
                "morning-plan"
            ]
        );
        let morning = cfg
            .schedules
            .iter()
            .find(|s| s.id == "morning-plan")
            .unwrap();
        assert!(!morning.enabled);
        assert!(
            cfg.schedules
                .iter()
                .filter(|s| s.id != "morning-plan")
                .all(|s| s.enabled)
        );
        // Commented-out examples must be valid TOML once uncommented.
        let uncommented: String = read(&v, ".vault/config.toml")
            .lines()
            .map(|l| {
                l.strip_prefix("# ")
                    .filter(|r| r.starts_with('[') || r.contains(" = "))
                    .unwrap_or(l)
            })
            .collect::<Vec<_>>()
            .join("\n");
        let full: crate::config::VaultConfig = toml::from_str(&uncommented).unwrap();
        assert_eq!(full.agent.default.as_deref(), Some("junie"));
        let jira = full.jira.as_ref().unwrap();
        assert_eq!(
            jira.queries["mine"],
            "assignee = currentUser() AND resolution = Unresolved ORDER BY updated DESC"
        );
        assert!(full.is_always_allowed("junie", "read"));
        assert_eq!(full.folders["people"].agent.as_deref(), Some("none"));
        assert!(full.agents.contains_key("my-agent"));
    }

    #[test]
    fn templates_list_and_render() {
        let v = vault_with(&[]);
        assert!(list_templates(v.path()).unwrap().is_empty());
        scaffold(v.path()).unwrap();
        fs::write(v.path().join("templates/notes.txt"), "").unwrap();
        assert_eq!(
            list_templates(v.path()).unwrap(),
            ["decision", "meeting", "one-on-one", "person"]
        );
        let now = chrono::NaiveDate::from_ymd_opt(2026, 10, 27)
            .unwrap()
            .and_hms_opt(9, 5, 0)
            .unwrap();
        let out = render_template(v.path(), "meeting", "Roadmap sync", now).unwrap();
        assert!(out.starts_with("# Roadmap sync\n"));
        assert!(out.contains("2026-10-27 09:05"));
        assert!(!out.contains("{{"));
        let one = render_template(v.path(), "one-on-one.md", "Alex", now).unwrap();
        assert!(one.contains("# 1:1 Alex — 2026-10-27"));
        for s in [
            "## Their topics",
            "## My topics",
            "## Feedback",
            "## Action items",
        ] {
            assert!(one.contains(s), "missing {s}");
        }
        let person = render_template(v.path(), "person", "Sam", now).unwrap();
        for s in [
            "Role",
            "## Goals",
            "## 1:1 log",
            "## Feedback",
            "## Review evidence",
        ] {
            assert!(person.contains(s), "missing {s}");
        }
        assert!(render_template(v.path(), "missing", "x", now).is_err());
        assert!(render_template(v.path(), "../tasks", "x", now).is_err());
    }

    #[test]
    fn origin_url_parsing() {
        assert_eq!(
            origin_url("[remote \"origin\"]\n  url = \"https://h/r.git\"\n").as_deref(),
            Some("https://h/r.git")
        );
        assert_eq!(origin_url("[remote \"fork\"]\nurl = x\n"), None);
    }
}
