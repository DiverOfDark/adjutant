//! Tasks: parsing `- [ ]` checkboxes out of Markdown files and the small set
//! of deterministic edits Adjutant performs on them (tick, reopen, add, move,
//! archive, quick capture).
//!
//! The single always-current task list lives in `tasks.md` with sections
//! `## Now`, `## Next`, `## Waiting on`, `## Someday`. Ticking a task there
//! moves it (with its nested children) to `tasks/done/YYYY-MM.md`; ticking a
//! task anywhere else edits it in place.
//!
//! All writes are atomic (temp file in the same directory, then rename) and
//! preserve the file's line endings and trailing newline.

use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::sync::LazyLock;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, bail};
use chrono::{NaiveDate, NaiveDateTime};
use regex::Regex;
use serde::{Deserialize, Serialize};

pub const TASKS_FILE: &str = "tasks.md";
pub const SECTIONS: [&str; 4] = ["Now", "Next", "Waiting on", "Someday"];
pub const DEFAULT_TASKS_MD: &str = "# Tasks\n\n## Now\n\n## Next\n\n## Waiting on\n\n## Someday\n";

/// Directory holding the monthly archives of completed tasks.
const DONE_DIR: &str = "tasks/done";
/// Directory for quick-captured notes.
const INBOX_DIR: &str = "inbox";
/// Directories (vault-relative) never scanned for tasks.
const SKIP_DIRS: [&str; 7] = [
    ".git",
    ".vault",
    "repos",
    "node_modules",
    "tasks/done",
    "jira",
    "templates",
];

static CHECKBOX_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^(\s*)([-*+]) \[( |x|X)\] (.*)$").unwrap());
static DUE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"@due\((\d{4}-\d{2}-\d{2})\)").unwrap());
static DONE_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"@done\((\d{4}-\d{2}-\d{2})\)").unwrap());
static DONE_STRIP_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\s*@done\(\d{4}-\d{2}-\d{2}\)").unwrap());
static TITLE_STRIP_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"@(?:due|done)\(\d{4}-\d{2}-\d{2}\)").unwrap());
static TAG_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:^|\s)#([\p{L}\p{N}_/-]+)").unwrap());
static LINK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\[\[([^\]|#]+)(?:[#|][^\]]*)?\]\]").unwrap());
static JIRA_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b[A-Z][A-Z0-9]+-\d+\b").unwrap());

/// Errors callers may want to distinguish (available via `anyhow::Error::downcast_ref`).
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum TaskError {
    /// The line on disk no longer matches what the UI saw.
    #[error("task changed on disk; reload")]
    Changed,
    /// The addressed line is not a checkbox task.
    #[error("line {0} is not a task")]
    NotATask(usize),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    /// Vault-relative path with '/' separators, e.g. "tasks.md", "people/alex.md".
    pub file: String,
    /// 0-based line index.
    pub line: usize,
    /// The full original line.
    pub raw: String,
    /// Text after "- [ ] " (tokens kept).
    pub text: String,
    /// `text` with @due(...)/@done(...) removed and whitespace collapsed.
    pub title: String,
    pub done: bool,
    /// Nearest preceding "## " heading text.
    pub section: Option<String>,
    pub due: Option<NaiveDate>,
    pub done_on: Option<NaiveDate>,
    /// Tags without '#'.
    pub tags: Vec<String>,
    /// `[[target]]` targets (part before '|' or '#').
    pub links: Vec<String>,
    /// Subset of `links` starting with "people/".
    pub people: Vec<String>,
    pub jira_keys: Vec<String>,
    /// Leading whitespace width (tab = 4).
    pub indent: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CompleteOutcome {
    /// Vault-relative archive path, e.g. "tasks/done/2026-10.md"; `None` for in-place edits.
    pub archived_to: Option<String>,
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

pub fn parse_tasks(file: &str, content: &str) -> Vec<Task> {
    let lines: Vec<&str> = content.lines().collect();
    let mask = fence_mask(&lines);
    let mut section: Option<String> = None;
    let mut tasks = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if mask[i] {
            continue;
        }
        if let Some((level, text)) = heading(line) {
            if level == 2 {
                section = Some(text.to_string());
            }
            continue;
        }
        if let Some(task) = parse_line(file, i, line, section.as_deref()) {
            tasks.push(task);
        }
    }
    tasks
}

fn parse_line(file: &str, line_no: usize, line: &str, section: Option<&str>) -> Option<Task> {
    let caps = CHECKBOX_RE.captures(line)?;
    let indent = indent_width(&caps[1]);
    let done = &caps[3] != " ";
    let text = caps[4].to_string();

    let due = DUE_RE
        .captures(&text)
        .and_then(|c| NaiveDate::parse_from_str(&c[1], "%Y-%m-%d").ok());
    let done_on = DONE_RE
        .captures(&text)
        .and_then(|c| NaiveDate::parse_from_str(&c[1], "%Y-%m-%d").ok());
    let title = TITLE_STRIP_RE
        .replace_all(&text, " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    let mut tags = Vec::new();
    for c in TAG_RE.captures_iter(&text) {
        let tag = &c[1];
        if tag.chars().all(|ch| ch.is_ascii_digit()) {
            continue;
        }
        push_unique(&mut tags, tag.to_string());
    }
    let mut links = Vec::new();
    for c in LINK_RE.captures_iter(&text) {
        let target = c[1].trim();
        if !target.is_empty() {
            push_unique(&mut links, target.to_string());
        }
    }
    let people = links
        .iter()
        .filter(|l| l.starts_with("people/"))
        .cloned()
        .collect();
    let mut jira_keys = Vec::new();
    for m in JIRA_RE.find_iter(&text) {
        push_unique(&mut jira_keys, m.as_str().to_string());
    }

    Some(Task {
        file: file.to_string(),
        line: line_no,
        raw: line.to_string(),
        text,
        title,
        done,
        section: section.map(str::to_string),
        due,
        done_on,
        tags,
        links,
        people,
        jira_keys,
        indent,
    })
}

fn push_unique(v: &mut Vec<String>, s: String) {
    if !v.contains(&s) {
        v.push(s);
    }
}

fn indent_width(ws: &str) -> usize {
    ws.chars().map(|c| if c == '\t' { 4 } else { 1 }).sum()
}

fn line_indent(line: &str) -> usize {
    let ws_len = line.len() - line.trim_start().len();
    indent_width(&line[..ws_len])
}

fn leading_ws(line: &str) -> &str {
    &line[..line.len() - line.trim_start().len()]
}

/// ATX heading: returns (level, trimmed text).
fn heading(line: &str) -> Option<(usize, &str)> {
    let t = line.trim_end();
    let stripped = t.trim_start_matches(' ');
    if t.len() - stripped.len() > 3 {
        return None;
    }
    let level = stripped.chars().take_while(|&c| c == '#').count();
    if level == 0 || level > 6 {
        return None;
    }
    let rest = &stripped[level..];
    if rest.is_empty() {
        return Some((level, ""));
    }
    if !rest.starts_with([' ', '\t']) {
        return None;
    }
    Some((level, rest.trim()))
}

/// `true` for every line that is part of a fenced code block (fences included).
fn fence_mask<S: AsRef<str>>(lines: &[S]) -> Vec<bool> {
    let mut mask = Vec::with_capacity(lines.len());
    let mut open: Option<(char, usize)> = None;
    for line in lines {
        let t = line.as_ref().trim_start();
        let fence = ['`', '~'].into_iter().find_map(|ch| {
            let n = t.chars().take_while(|&c| c == ch).count();
            (n >= 3).then_some((ch, n))
        });
        match (open, fence) {
            (None, Some(f)) => {
                open = Some(f);
                mask.push(true);
            }
            (Some((ch, n)), Some((fch, fn_))) if ch == fch && fn_ >= n => {
                open = None;
                mask.push(true);
            }
            (Some(_), _) => mask.push(true),
            (None, None) => mask.push(false),
        }
    }
    mask
}

// ---------------------------------------------------------------------------
// Scanning
// ---------------------------------------------------------------------------

/// Every task in the vault's .md files (open and done).
pub fn scan_vault(vault: &Path) -> anyhow::Result<Vec<Task>> {
    let root = vault.to_path_buf();
    let filter_root = root.clone();
    let walker = ignore::WalkBuilder::new(&root)
        .hidden(true)
        .git_ignore(true)
        .require_git(false)
        .filter_entry(move |entry| {
            if !entry.file_type().is_some_and(|t| t.is_dir()) {
                return true;
            }
            let Ok(rel) = entry.path().strip_prefix(&filter_root) else {
                return true;
            };
            let rel = rel_string(rel);
            let name = entry.file_name().to_string_lossy();
            !(SKIP_DIRS.contains(&rel.as_str()) || name == ".git" || name == "node_modules")
        })
        .build();

    let mut tasks = Vec::new();
    let mut files = Vec::new();
    for entry in walker {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let Ok(rel) = path.strip_prefix(&root) else {
            continue;
        };
        files.push((rel_string(rel), path.to_path_buf()));
    }
    files.sort();
    for (rel, path) in files {
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let Ok(content) = String::from_utf8(bytes) else {
            continue;
        };
        tasks.extend(parse_tasks(&rel, &content));
    }
    Ok(tasks)
}

fn rel_string(rel: &Path) -> String {
    rel.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

// ---------------------------------------------------------------------------
// File plumbing
// ---------------------------------------------------------------------------

/// A text file split into lines, remembering its line ending and trailing newline.
struct Doc {
    lines: Vec<String>,
    eol: &'static str,
    trailing: bool,
}

impl Doc {
    fn parse(content: &str) -> Self {
        Doc {
            lines: content.lines().map(str::to_string).collect(),
            eol: if content.contains("\r\n") {
                "\r\n"
            } else {
                "\n"
            },
            trailing: content.ends_with('\n'),
        }
    }

    fn read(path: &Path) -> anyhow::Result<Self> {
        let content =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        Ok(Self::parse(&content))
    }

    fn render(&self) -> String {
        if self.lines.is_empty() {
            return String::new();
        }
        let mut out = self.lines.join(self.eol);
        if self.trailing {
            out.push_str(self.eol);
        }
        out
    }
}

/// Resolve a vault-relative path, refusing anything that escapes the vault.
fn resolve(vault: &Path, file: &str) -> anyhow::Result<PathBuf> {
    let rel = Path::new(file);
    if file.is_empty() || !rel.components().all(|c| matches!(c, Component::Normal(_))) {
        bail!("invalid vault path: {file:?}");
    }
    Ok(vault.join(rel))
}

static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Write `contents` to `path` atomically (temp file in the same dir, then rename).
fn write_atomic(path: &Path, contents: &str) -> anyhow::Result<()> {
    use std::io::Write;
    let dir = path.parent().context("path has no parent directory")?;
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let name = path
        .file_name()
        .context("path has no file name")?
        .to_string_lossy();
    let tmp = dir.join(format!(
        ".{name}.adjutant-{}-{}.tmp",
        std::process::id(),
        TMP_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| -> anyhow::Result<()> {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(contents.as_bytes())?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&tmp, path)?;
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result.with_context(|| format!("writing {}", path.display()))
}

/// Load `file`, verify `line` still equals `expected_raw` and is a task.
fn load_checked(
    vault: &Path,
    file: &str,
    line: usize,
    expected_raw: &str,
) -> anyhow::Result<(PathBuf, Doc, Task)> {
    let path = resolve(vault, file)?;
    if !path.exists() {
        return Err(TaskError::Changed.into());
    }
    let doc = Doc::read(&path)?;
    if doc.lines.get(line).map(String::as_str) != Some(expected_raw) {
        return Err(TaskError::Changed.into());
    }
    let task = parse_tasks(file, &doc.render())
        .into_iter()
        .find(|t| t.line == line)
        .ok_or(TaskError::NotATask(line))?;
    Ok((path, doc, task))
}

/// Exclusive end index of the block made of the task at `start` and its nested children.
fn block_end<S: AsRef<str>>(lines: &[S], start: usize) -> usize {
    let base = line_indent(lines[start].as_ref());
    let mut j = start + 1;
    while j < lines.len() {
        let l = lines[j].as_ref();
        if l.trim().is_empty() {
            let next = (j + 1..lines.len()).find(|&k| !lines[k].as_ref().trim().is_empty());
            match next {
                Some(k) if line_indent(lines[k].as_ref()) > base => j = k,
                _ => break,
            }
        } else if line_indent(l) > base {
            j += 1;
        } else {
            break;
        }
    }
    j
}

/// Remove the task at `start` with its children; returns the removed lines with the
/// task's own indentation stripped from every line (so it becomes top-level).
fn take_block(lines: &mut Vec<String>, start: usize) -> Vec<String> {
    let end = block_end(lines, start);
    let ws = leading_ws(&lines[start]).to_string();
    lines
        .drain(start..end)
        .map(|l| match l.strip_prefix(ws.as_str()) {
            Some(rest) => rest.to_string(),
            None => l,
        })
        .collect()
}

/// Index after the last non-blank line of `## section`, or `None` if the section is missing.
fn section_insert_index(lines: &[String], section: &str) -> Option<usize> {
    let mask = fence_mask(lines);
    let wanted = section.trim();
    let h = (0..lines.len()).find(|&i| {
        !mask[i] && matches!(heading(&lines[i]), Some((2, t)) if t.eq_ignore_ascii_case(wanted))
    })?;
    let end = (h + 1..lines.len())
        .find(|&i| !mask[i] && matches!(heading(&lines[i]), Some((l, _)) if l <= 2))
        .unwrap_or(lines.len());
    let last = (h + 1..end).rev().find(|&i| !lines[i].trim().is_empty());
    Some(last.map_or(h + 1, |i| i + 1))
}

/// Insert `new` at the end of `## section`, creating the section at the end if missing.
fn insert_into_section(doc: &mut Doc, section: &str, new: Vec<String>) {
    match section_insert_index(&doc.lines, section) {
        Some(idx) => {
            doc.lines.splice(idx..idx, new);
        }
        None => {
            if doc.lines.last().is_some_and(|l| !l.trim().is_empty()) {
                doc.lines.push(String::new());
            }
            doc.lines.push(format!("## {}", section.trim()));
            doc.lines.extend(new);
        }
    }
    doc.trailing = true;
}

fn archive_rel(month: &str) -> String {
    format!("{DONE_DIR}/{month}.md")
}

/// Append lines to `tasks/done/<month>.md`, creating it with a header if missing.
fn append_to_archive(vault: &Path, month: &str, new: &[String]) -> anyhow::Result<String> {
    let rel = archive_rel(month);
    let path = resolve(vault, &rel)?;
    let mut content = if path.exists() {
        std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?
    } else {
        format!("# Done {month}\n\n")
    };
    let eol = if content.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    if !content.is_empty() && !content.ends_with('\n') {
        content.push_str(eol);
    }
    for l in new {
        content.push_str(l);
        content.push_str(eol);
    }
    write_atomic(&path, &content)?;
    Ok(rel)
}

/// Turn the first line of an archived block into "- [x] <text> @done(date)".
fn done_line(text: &str, date: NaiveDate) -> String {
    let text = text.trim_end();
    if DONE_RE.is_match(text) {
        format!("- [x] {text}")
    } else {
        format!("- [x] {text} @done({})", date.format("%Y-%m-%d"))
    }
}

fn single_line(text: &str) -> anyhow::Result<String> {
    let t = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if t.is_empty() {
        bail!("task text is empty");
    }
    Ok(t)
}

// ---------------------------------------------------------------------------
// Operations
// ---------------------------------------------------------------------------

/// Tick a task. In `tasks.md` the task (with children) moves to the monthly archive;
/// elsewhere it is ticked in place and stamped `@done(today)`.
pub fn complete_task(
    vault: &Path,
    file: &str,
    line: usize,
    expected_raw: &str,
    today: NaiveDate,
) -> anyhow::Result<CompleteOutcome> {
    let (path, mut doc, task) = load_checked(vault, file, line, expected_raw)?;

    if file == TASKS_FILE {
        let mut block = take_block(&mut doc.lines, line);
        block[0] = done_line(&task.text, today);
        let month = today.format("%Y-%m").to_string();
        // Archive first: a crash in between duplicates a task rather than losing it.
        let rel = append_to_archive(vault, &month, &block)?;
        write_atomic(&path, &doc.render())?;
        return Ok(CompleteOutcome {
            archived_to: Some(rel),
        });
    }

    let caps = CHECKBOX_RE
        .captures(expected_raw)
        .ok_or(TaskError::NotATask(line))?;
    let mark = caps.get(3).expect("group 3").range();
    let mut new_line = format!(
        "{}x{}",
        &expected_raw[..mark.start],
        &expected_raw[mark.end..]
    );
    if !DONE_RE.is_match(&task.text) {
        new_line = format!(
            "{} @done({})",
            new_line.trim_end(),
            today.format("%Y-%m-%d")
        );
    }
    doc.lines[line] = new_line;
    write_atomic(&path, &doc.render())?;
    Ok(CompleteOutcome { archived_to: None })
}

/// Reopen a done task in place: "- [x]" -> "- [ ]", strip `@done(...)`.
pub fn reopen_task(
    vault: &Path,
    file: &str,
    line: usize,
    expected_raw: &str,
) -> anyhow::Result<()> {
    let (path, mut doc, _task) = load_checked(vault, file, line, expected_raw)?;
    let caps = CHECKBOX_RE
        .captures(expected_raw)
        .ok_or(TaskError::NotATask(line))?;
    let mark = caps.get(3).expect("group 3").range();
    let reopened = format!(
        "{} {}",
        &expected_raw[..mark.start],
        &expected_raw[mark.end..]
    );
    doc.lines[line] = DONE_STRIP_RE
        .replace_all(&reopened, "")
        .trim_end()
        .to_string();
    write_atomic(&path, &doc.render())
}

/// Append "- [ ] <text>" to the end of `## <section>` in tasks.md.
pub fn add_task(vault: &Path, section: &str, text: &str) -> anyhow::Result<()> {
    let text = single_line(text)?;
    if section.trim().is_empty() {
        bail!("section is empty");
    }
    let path = resolve(vault, TASKS_FILE)?;
    let mut doc = if path.exists() {
        Doc::read(&path)?
    } else {
        Doc::parse(DEFAULT_TASKS_MD)
    };
    insert_into_section(&mut doc, section, vec![format!("- [ ] {text}")]);
    write_atomic(&path, &doc.render())
}

/// Move a task (with children) within tasks.md to the end of another section.
pub fn move_task(
    vault: &Path,
    line: usize,
    expected_raw: &str,
    to_section: &str,
) -> anyhow::Result<()> {
    if to_section.trim().is_empty() {
        bail!("section is empty");
    }
    let (path, mut doc, _task) = load_checked(vault, TASKS_FILE, line, expected_raw)?;
    let block = take_block(&mut doc.lines, line);
    insert_into_section(&mut doc, to_section, block);
    write_atomic(&path, &doc.render())
}

/// Quick capture: writes `inbox/YYYY-MM-DD-HHMM-<slug>.md`; returns its vault-relative path.
pub fn capture_note(vault: &Path, text: &str, now: NaiveDateTime) -> anyhow::Result<String> {
    let body = text.trim_end();
    if body.trim().is_empty() {
        bail!("note text is empty");
    }
    let base = format!("{}-{}", now.format("%Y-%m-%d-%H%M"), slugify(body));
    let mut n = 1;
    loop {
        let rel = if n == 1 {
            format!("{INBOX_DIR}/{base}.md")
        } else {
            format!("{INBOX_DIR}/{base}-{n}.md")
        };
        let path = resolve(vault, &rel)?;
        if !path.exists() {
            write_atomic(&path, &format!("{body}\n"))?;
            return Ok(rel);
        }
        n += 1;
    }
}

/// First ~6 words, lowercase ASCII alphanumerics and '-', at most 40 chars.
fn slugify(text: &str) -> String {
    let words: Vec<String> = text
        .split_whitespace()
        .map(|w| {
            w.chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() {
                        c.to_ascii_lowercase()
                    } else {
                        '-'
                    }
                })
                .collect::<String>()
        })
        .map(|w| {
            w.split('-')
                .filter(|p| !p.is_empty())
                .collect::<Vec<_>>()
                .join("-")
        })
        .filter(|w| !w.is_empty())
        .take(6)
        .collect();
    let mut slug = words.join("-");
    if slug.len() > 40 {
        slug.truncate(40);
    }
    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        "note".to_string()
    } else {
        slug
    }
}

/// Move every done task in tasks.md (with children) to its monthly archive.
/// Month = the task's `@done` date if present, else `today`. Returns the number moved.
pub fn archive_done(vault: &Path, today: NaiveDate) -> anyhow::Result<usize> {
    let path = resolve(vault, TASKS_FILE)?;
    if !path.exists() {
        return Ok(0);
    }
    let mut doc = Doc::read(&path)?;
    let tasks = parse_tasks(TASKS_FILE, &doc.render());

    // Pick top-most done blocks (a done task nested inside a moved block travels with it).
    let mut picks: Vec<(usize, usize, &Task)> = Vec::new();
    for t in tasks.iter().filter(|t| t.done) {
        if picks.iter().any(|&(s, e, _)| t.line >= s && t.line < e) {
            continue;
        }
        picks.push((t.line, block_end(&doc.lines, t.line), t));
    }
    if picks.is_empty() {
        return Ok(0);
    }

    let mut by_month: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for &(start, _, t) in &picks {
        let date = t.done_on.unwrap_or(today);
        let ws = leading_ws(&doc.lines[start]).to_string();
        let end = block_end(&doc.lines, start);
        let mut block: Vec<String> = doc.lines[start..end]
            .iter()
            .map(|l| l.strip_prefix(ws.as_str()).unwrap_or(l).to_string())
            .collect();
        block[0] = done_line(&t.text, date);
        by_month
            .entry(date.format("%Y-%m").to_string())
            .or_default()
            .extend(block);
    }
    for (month, lines) in &by_month {
        append_to_archive(vault, month, lines)?;
    }
    let count = picks.len();
    let ranges: Vec<(usize, usize)> = picks.iter().map(|&(s, e, _)| (s, e)).collect();
    for (s, e) in ranges.into_iter().rev() {
        doc.lines.drain(s..e);
    }
    write_atomic(&path, &doc.render())?;
    Ok(count)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn d(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    fn vault_with(files: &[(&str, &str)]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        for (rel, content) in files {
            let p = dir.path().join(rel);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, content).unwrap();
        }
        dir
    }

    fn read(dir: &tempfile::TempDir, rel: &str) -> String {
        fs::read_to_string(dir.path().join(rel)).unwrap()
    }

    #[test]
    fn parses_tokens() {
        let md = "## Now\n- [ ] Review PLAT-123 and PLAT-123 with [[people/alex|Alex]] #platform #42 @due(2026-10-30) see [[notes/x#h]]\n";
        let tasks = parse_tasks("tasks.md", md);
        assert_eq!(tasks.len(), 1);
        let t = &tasks[0];
        assert_eq!(t.line, 1);
        assert!(!t.done);
        assert_eq!(t.section.as_deref(), Some("Now"));
        assert_eq!(t.due, Some(d("2026-10-30")));
        assert_eq!(t.done_on, None);
        assert_eq!(t.tags, vec!["platform"]);
        assert_eq!(t.links, vec!["people/alex", "notes/x"]);
        assert_eq!(t.people, vec!["people/alex"]);
        assert_eq!(t.jira_keys, vec!["PLAT-123"]);
        assert_eq!(
            t.title,
            "Review PLAT-123 and PLAT-123 with [[people/alex|Alex]] #platform #42 see [[notes/x#h]]"
        );
        assert!(t.text.contains("@due(2026-10-30)"));
        assert_eq!(t.raw, md.lines().nth(1).unwrap());
    }

    #[test]
    fn parses_done_bullets_indent_and_unicode_tags() {
        let md = "* [x] Shipped @done(2026-10-01)\n  + [X] nested #команда\n- [] not a task\n-[ ] nor this\n";
        let tasks = parse_tasks("a.md", md);
        assert_eq!(tasks.len(), 2);
        assert!(tasks[0].done);
        assert_eq!(tasks[0].done_on, Some(d("2026-10-01")));
        assert_eq!(tasks[0].title, "Shipped");
        assert_eq!(tasks[0].section, None);
        assert!(tasks[1].done);
        assert_eq!(tasks[1].indent, 2);
        assert_eq!(tasks[1].tags, vec!["команда"]);
    }

    #[test]
    fn sections_follow_h2_headings() {
        let md = "# Tasks\n- [ ] a\n## Now\n- [ ] b\n### Sub\n- [ ] c\n## Waiting on \n- [ ] d\n";
        let s: Vec<_> = parse_tasks("tasks.md", md)
            .into_iter()
            .map(|t| t.section)
            .collect();
        assert_eq!(
            s,
            vec![
                None,
                Some("Now".into()),
                Some("Now".into()),
                Some("Waiting on".into())
            ]
        );
    }

    #[test]
    fn ignores_code_fences() {
        let md = "- [ ] real\n```md\n- [ ] fake\n## Fake\n```\n~~~~\n- [ ] fake2\n```\n- [ ] still fake\n~~~~\n- [ ] real2\n";
        let tasks = parse_tasks("a.md", md);
        let titles: Vec<_> = tasks.iter().map(|t| t.title.as_str()).collect();
        assert_eq!(titles, vec!["real", "real2"]);
        assert_eq!(tasks[1].line, 10);
        assert_eq!(tasks[1].section, None);
    }

    #[test]
    fn crlf_parse_has_no_carriage_returns() {
        let tasks = parse_tasks("a.md", "## Now\r\n- [ ] x #t\r\n");
        assert_eq!(tasks[0].raw, "- [ ] x #t");
        assert_eq!(tasks[0].tags, vec!["t"]);
    }

    #[test]
    fn scan_vault_skips_ignored_dirs() {
        let dir = vault_with(&[
            ("tasks.md", "## Now\n- [ ] top\n"),
            ("people/alex.md", "- [ ] ask alex\n- [x] done one\n"),
            ("tasks/done/2026-10.md", "- [x] archived\n"),
            ("repos/x/README.md", "- [ ] repo task\n"),
            ("jira/snapshot.md", "- [ ] jira\n"),
            ("templates/t.md", "- [ ] tpl\n"),
            (".vault/x.md", "- [ ] hidden\n"),
            ("node_modules/a/b.md", "- [ ] nm\n"),
            ("ignored/n.md", "- [ ] gitignored\n"),
            (".gitignore", "ignored/\n"),
            ("notes/n.txt", "- [ ] not md\n"),
        ]);
        let tasks = scan_vault(dir.path()).unwrap();
        let got: Vec<_> = tasks
            .iter()
            .map(|t| (t.file.as_str(), t.title.as_str()))
            .collect();
        assert_eq!(
            got,
            vec![
                ("people/alex.md", "ask alex"),
                ("people/alex.md", "done one"),
                ("tasks.md", "top"),
            ]
        );
    }

    #[test]
    fn complete_in_tasks_md_archives_with_children() {
        let md = "# Tasks\n\n## Now\n- [ ] Parent #x\n  - sub note\n\n  - [ ] child\n- [ ] Other\n\n## Next\n";
        let dir = vault_with(&[("tasks.md", md)]);
        let out = complete_task(
            dir.path(),
            "tasks.md",
            3,
            "- [ ] Parent #x",
            d("2026-10-27"),
        )
        .unwrap();
        assert_eq!(out.archived_to.as_deref(), Some("tasks/done/2026-10.md"));
        assert_eq!(
            read(&dir, "tasks.md"),
            "# Tasks\n\n## Now\n- [ ] Other\n\n## Next\n"
        );
        assert_eq!(
            read(&dir, "tasks/done/2026-10.md"),
            "# Done 2026-10\n\n- [x] Parent #x @done(2026-10-27)\n  - sub note\n\n  - [ ] child\n"
        );

        // Second completion appends to the existing archive.
        complete_task(dir.path(), "tasks.md", 3, "- [ ] Other", d("2026-10-28")).unwrap();
        assert!(
            read(&dir, "tasks/done/2026-10.md")
                .ends_with("  - [ ] child\n- [x] Other @done(2026-10-28)\n")
        );
        assert_eq!(read(&dir, "tasks.md"), "# Tasks\n\n## Now\n\n## Next\n");
    }

    #[test]
    fn complete_blank_line_then_unindented_ends_block() {
        let md = "## Now\n- [ ] A\n\n- [ ] B\n";
        let dir = vault_with(&[("tasks.md", md)]);
        complete_task(dir.path(), "tasks.md", 1, "- [ ] A", d("2026-11-02")).unwrap();
        assert_eq!(read(&dir, "tasks.md"), "## Now\n\n- [ ] B\n");
        assert_eq!(
            read(&dir, "tasks/done/2026-11.md"),
            "# Done 2026-11\n\n- [x] A @done(2026-11-02)\n"
        );
    }

    #[test]
    fn complete_preserves_crlf_and_missing_trailing_newline() {
        let dir = vault_with(&[("tasks.md", "## Now\r\n- [ ] a\r\n- [ ] b\r\n")]);
        complete_task(dir.path(), "tasks.md", 1, "- [ ] a", d("2026-10-27")).unwrap();
        assert_eq!(read(&dir, "tasks.md"), "## Now\r\n- [ ] b\r\n");

        let dir = vault_with(&[("n.md", "x\r\n- [ ] a")]);
        complete_task(dir.path(), "n.md", 1, "- [ ] a", d("2026-10-27")).unwrap();
        assert_eq!(read(&dir, "n.md"), "x\r\n- [x] a @done(2026-10-27)");
    }

    #[test]
    fn complete_in_other_file_edits_in_place() {
        let md = "# Alex\n  * [ ] Ask about promo \n- [ ] Pre-stamped @done(2026-01-01)\n";
        let dir = vault_with(&[("people/alex.md", md)]);
        let out = complete_task(
            dir.path(),
            "people/alex.md",
            1,
            "  * [ ] Ask about promo ",
            d("2026-10-27"),
        )
        .unwrap();
        assert_eq!(out.archived_to, None);
        complete_task(
            dir.path(),
            "people/alex.md",
            2,
            "- [ ] Pre-stamped @done(2026-01-01)",
            d("2026-10-27"),
        )
        .unwrap();
        assert_eq!(
            read(&dir, "people/alex.md"),
            "# Alex\n  * [x] Ask about promo @done(2026-10-27)\n- [x] Pre-stamped @done(2026-01-01)\n"
        );
        assert!(!dir.path().join("tasks/done").exists());
    }

    #[test]
    fn expected_raw_mismatch_is_rejected() {
        let dir = vault_with(&[("tasks.md", "## Now\n- [ ] a\n"), ("n.md", "- [x] b\n")]);
        let err =
            complete_task(dir.path(), "tasks.md", 1, "- [ ] old", d("2026-10-27")).unwrap_err();
        assert_eq!(err.to_string(), "task changed on disk; reload");
        assert_eq!(err.downcast_ref::<TaskError>(), Some(&TaskError::Changed));
        assert!(complete_task(dir.path(), "tasks.md", 9, "- [ ] a", d("2026-10-27")).is_err());
        assert!(reopen_task(dir.path(), "n.md", 0, "- [x] c").is_err());
        assert!(move_task(dir.path(), 1, "- [ ] z", "Next").is_err());
        assert!(complete_task(dir.path(), "missing.md", 0, "- [ ] a", d("2026-10-27")).is_err());
        // Nothing changed.
        assert_eq!(read(&dir, "tasks.md"), "## Now\n- [ ] a\n");
    }

    #[test]
    fn rejects_paths_outside_vault_and_non_tasks() {
        let dir = vault_with(&[("n.md", "# Title\n")]);
        assert!(complete_task(dir.path(), "../n.md", 0, "# Title", d("2026-10-27")).is_err());
        assert!(complete_task(dir.path(), "/etc/passwd", 0, "x", d("2026-10-27")).is_err());
        let err = complete_task(dir.path(), "n.md", 0, "# Title", d("2026-10-27")).unwrap_err();
        assert_eq!(
            err.downcast_ref::<TaskError>(),
            Some(&TaskError::NotATask(0))
        );
    }

    #[test]
    fn reopen_strips_done() {
        let dir = vault_with(&[(
            "n.md",
            "- [x] Ship it @done(2026-10-20) #x\n- [X] Two @done(2026-10-21)\n",
        )]);
        reopen_task(dir.path(), "n.md", 0, "- [x] Ship it @done(2026-10-20) #x").unwrap();
        reopen_task(dir.path(), "n.md", 1, "- [X] Two @done(2026-10-21)").unwrap();
        assert_eq!(read(&dir, "n.md"), "- [ ] Ship it #x\n- [ ] Two\n");
    }

    #[test]
    fn add_task_to_existing_section() {
        let md = "# Tasks\n\n## Now\n- [ ] a\n\n## Next\n- [ ] b\n";
        let dir = vault_with(&[("tasks.md", md)]);
        add_task(dir.path(), "Now", "  new\nthing  ").unwrap();
        add_task(dir.path(), "Next", "c").unwrap();
        assert_eq!(
            read(&dir, "tasks.md"),
            "# Tasks\n\n## Now\n- [ ] a\n- [ ] new thing\n\n## Next\n- [ ] b\n- [ ] c\n"
        );
        assert!(add_task(dir.path(), "Now", "   ").is_err());
    }

    #[test]
    fn add_task_missing_section_and_file() {
        let dir = vault_with(&[]);
        add_task(dir.path(), "Now", "first").unwrap();
        assert_eq!(
            read(&dir, "tasks.md"),
            "# Tasks\n\n## Now\n- [ ] first\n\n## Next\n\n## Waiting on\n\n## Someday\n"
        );
        add_task(dir.path(), "Someday", "later").unwrap();
        add_task(dir.path(), "Ideas", "new section").unwrap();
        assert!(
            read(&dir, "tasks.md")
                .ends_with("## Someday\n- [ ] later\n\n## Ideas\n- [ ] new section\n")
        );

        let dir = vault_with(&[("tasks.md", "# Tasks\r\n## Now\r\n- [ ] a")]);
        add_task(dir.path(), "Next", "b").unwrap();
        assert_eq!(
            read(&dir, "tasks.md"),
            "# Tasks\r\n## Now\r\n- [ ] a\r\n\r\n## Next\r\n- [ ] b\r\n"
        );
    }

    #[test]
    fn add_task_ignores_headings_in_fences() {
        let md = "## Now\n```\n## Next\n```\n- [ ] a\n\n## Next\n";
        let dir = vault_with(&[("tasks.md", md)]);
        add_task(dir.path(), "Now", "b").unwrap();
        assert_eq!(
            read(&dir, "tasks.md"),
            "## Now\n```\n## Next\n```\n- [ ] a\n- [ ] b\n\n## Next\n"
        );
    }

    #[test]
    fn move_task_with_children() {
        let md = "## Now\n- [ ] a\n  - detail\n- [ ] b\n\n## Waiting on\n- [ ] w\n\n## Someday\n";
        let dir = vault_with(&[("tasks.md", md)]);
        move_task(dir.path(), 1, "- [ ] a", "Waiting on").unwrap();
        assert_eq!(
            read(&dir, "tasks.md"),
            "## Now\n- [ ] b\n\n## Waiting on\n- [ ] w\n- [ ] a\n  - detail\n\n## Someday\n"
        );
        // Moving to a missing section creates it; nested task becomes top-level.
        let md = "## Now\n- [ ] p\n    - [ ] nested\n      - note\n";
        let dir = vault_with(&[("tasks.md", md)]);
        move_task(dir.path(), 2, "    - [ ] nested", "Next").unwrap();
        assert_eq!(
            read(&dir, "tasks.md"),
            "## Now\n- [ ] p\n\n## Next\n- [ ] nested\n  - note\n"
        );
    }

    #[test]
    fn capture_note_slug_and_collisions() {
        let dir = vault_with(&[]);
        let now = NaiveDateTime::parse_from_str("2026-10-27 09:05", "%Y-%m-%d %H:%M").unwrap();
        let p1 = capture_note(
            dir.path(),
            "Talk to Sam about Q1 budget, hiring & more stuff",
            now,
        )
        .unwrap();
        assert_eq!(p1, "inbox/2026-10-27-0905-talk-to-sam-about-q1-budget.md");
        assert_eq!(
            read(&dir, &p1),
            "Talk to Sam about Q1 budget, hiring & more stuff\n"
        );
        let p2 = capture_note(dir.path(), "Talk to Sam about Q1 budget!", now).unwrap();
        assert_eq!(p2, "inbox/2026-10-27-0905-talk-to-sam-about-q1-budget-2.md");
        let p3 = capture_note(dir.path(), "talk to sam about q1 budget", now).unwrap();
        assert_eq!(p3, "inbox/2026-10-27-0905-talk-to-sam-about-q1-budget-3.md");
        let p4 = capture_note(dir.path(), "Привет мир", now).unwrap();
        assert_eq!(p4, "inbox/2026-10-27-0905-note.md");
        assert!(capture_note(dir.path(), "  \n", now).is_err());
    }

    #[test]
    fn slug_is_capped() {
        let s = slugify("Supercalifragilisticexpialidocious antidisestablishmentarianism words");
        assert!(s.len() <= 40);
        assert!(!s.ends_with('-'));
        assert_eq!(slugify("C++ / Rust: (ok)"), "c-rust-ok");
    }

    #[test]
    fn archive_done_moves_by_month() {
        let md = "# Tasks\n\n## Now\n- [x] old @done(2026-09-30)\n  - kid\n- [ ] open\n  - [x] done kid\n- [X] fresh\n\n## Next\n```\n- [x] in fence\n```\n";
        let dir = vault_with(&[
            ("tasks.md", md),
            (
                "tasks/done/2026-10.md",
                "# Done 2026-10\n\n- [x] prior @done(2026-10-01)",
            ),
        ]);
        let n = archive_done(dir.path(), d("2026-10-27")).unwrap();
        assert_eq!(n, 3);
        assert_eq!(
            read(&dir, "tasks.md"),
            "# Tasks\n\n## Now\n- [ ] open\n\n## Next\n```\n- [x] in fence\n```\n"
        );
        assert_eq!(
            read(&dir, "tasks/done/2026-09.md"),
            "# Done 2026-09\n\n- [x] old @done(2026-09-30)\n  - kid\n"
        );
        assert_eq!(
            read(&dir, "tasks/done/2026-10.md"),
            "# Done 2026-10\n\n- [x] prior @done(2026-10-01)\n- [x] done kid @done(2026-10-27)\n- [x] fresh @done(2026-10-27)\n"
        );
        assert_eq!(archive_done(dir.path(), d("2026-10-27")).unwrap(), 0);
    }

    #[test]
    fn archive_done_without_tasks_file() {
        let dir = vault_with(&[]);
        assert_eq!(archive_done(dir.path(), d("2026-10-27")).unwrap(), 0);
    }

    #[test]
    fn demo_vault_parses() {
        let md = include_str!("../../../examples/demo-vault/tasks.md");
        let tasks = parse_tasks("tasks.md", md);
        assert!(!tasks.is_empty());
        assert!(tasks.iter().all(|t| !t.done && t.section.is_some()));
        assert!(tasks.iter().any(|t| t.people == vec!["people/alex"]));
        assert!(tasks.iter().any(|t| t.jira_keys == vec!["PLAT-123"]));
    }

    #[test]
    fn task_serializes_camel_case() {
        let t = &parse_tasks("a.md", "- [ ] x ABC-1 @done(2026-01-02)\n")[0];
        let v = serde_json::to_value(t).unwrap();
        assert_eq!(v["jiraKeys"][0], "ABC-1");
        assert_eq!(v["doneOn"], "2026-01-02");
        let back: Task = serde_json::from_value(v).unwrap();
        assert_eq!(&back, t);
    }
}
