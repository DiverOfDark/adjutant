//! Full-text search across the vault, built on the ripgrep libraries.
//!
//! Literal, smart-case, multi-word (AND within a line). The longest word is
//! searched with `grep-searcher` (fast, memory-mapped, binary-aware); the
//! remaining words are checked per matching line.

use std::path::Path;

use anyhow::Context as _;
use grep_regex::RegexMatcherBuilder;
use grep_searcher::{BinaryDetection, SearcherBuilder, sinks};
use regex::{Regex, RegexBuilder};
use serde::{Deserialize, Serialize};

use crate::vault::{MAX_LINE_CHARS, is_hidden_dir, rel_of, resolve, truncate_chars};

/// Extensions searched.
pub const SEARCH_EXTENSIONS: &[&str] = &["md", "markdown", "txt"];
/// Max hits returned per file.
pub const MAX_HITS_PER_FILE: usize = 20;
/// Max file-name matches returned.
pub const MAX_FILE_MATCHES: usize = 20;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub file: String,
    /// 0-based line number.
    pub line: usize,
    /// The line, trimmed, max 300 chars (a window around the first match
    /// when the line is longer).
    pub text: String,
    /// Byte ranges of matches within `text`, sorted, non-overlapping.
    pub ranges: Vec<(usize, usize)>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SearchResults {
    /// Notes whose path contains all query words, case-insensitive, max 20.
    pub file_matches: Vec<String>,
    pub hits: Vec<SearchHit>,
    /// More hits existed than were returned (global or per-file limit).
    pub truncated: bool,
}

/// Case-insensitive (smart-case: case-sensitive if query has an uppercase
/// letter) literal search of the query across vault files (md/markdown/txt,
/// respects .gitignore, skipping `vault::HIDDEN_DIRS`). With multiple words a
/// line must contain all of them. Max `limit` hits total, max 20 per file;
/// files whose path matches the query come first, then by path.
pub fn search(vault: &Path, query: &str, limit: usize) -> anyhow::Result<SearchResults> {
    let mut results = SearchResults {
        file_matches: Vec::new(),
        hits: Vec::new(),
        truncated: false,
    };
    let words: Vec<&str> = query.split_whitespace().collect();
    if words.is_empty() {
        return Ok(results);
    }
    let root = resolve(vault, "")?;
    let files = candidate_files(&root)?;

    let lower_words: Vec<String> = words.iter().map(|w| w.to_lowercase()).collect();
    let path_matches =
        |rel: &str| -> bool { lower_words.iter().all(|w| rel.to_lowercase().contains(w)) };
    let (mut ordered, rest): (Vec<String>, Vec<String>) =
        files.into_iter().partition(|f| path_matches(f));
    results.file_matches = ordered.iter().take(MAX_FILE_MATCHES).cloned().collect();
    ordered.extend(rest);

    let case_sensitive = query.chars().any(char::is_uppercase);
    let word_res: Vec<Regex> = words
        .iter()
        .map(|w| {
            RegexBuilder::new(&regex::escape(w))
                .case_insensitive(!case_sensitive)
                .build()
        })
        .collect::<Result<_, _>>()?;
    let longest = words
        .iter()
        .max_by_key(|w| w.len())
        .expect("query has words");
    let matcher = RegexMatcherBuilder::new()
        .case_insensitive(!case_sensitive)
        .build(&regex::escape(longest))
        .context("building search matcher")?;
    let mut searcher = SearcherBuilder::new()
        .line_number(true)
        .binary_detection(BinaryDetection::quit(b'\x00'))
        .build();

    if limit == 0 {
        return Ok(results);
    }
    'files: for rel in &ordered {
        let mut in_file = 0usize;
        let mut stop = false;
        let sink = sinks::Lossy(|lnum, line| {
            let line = line.trim();
            if !word_res.iter().all(|re| re.is_match(line)) {
                return Ok(true);
            }
            if results.hits.len() >= limit || in_file >= MAX_HITS_PER_FILE {
                results.truncated = true;
                stop = results.hits.len() >= limit;
                return Ok(false);
            }
            let (text, ranges) = excerpt(line, &word_res);
            results.hits.push(SearchHit {
                file: rel.clone(),
                line: lnum.saturating_sub(1) as usize,
                text,
                ranges,
            });
            in_file += 1;
            Ok(true)
        });
        // Unreadable files (permissions, races with deletes) are skipped.
        let _ = searcher.search_path(&matcher, root.join(rel), sink);
        if stop {
            break 'files;
        }
    }
    Ok(results)
}

/// Vault-relative paths of searchable files, sorted.
fn candidate_files(root: &Path) -> anyhow::Result<Vec<String>> {
    let walk_root = root.to_path_buf();
    let walker = ignore::WalkBuilder::new(root)
        .hidden(true)
        .parents(false)
        .require_git(false)
        .follow_links(false)
        .filter_entry(move |entry| {
            if !entry.file_type().is_some_and(|t| t.is_dir()) {
                return true;
            }
            let name = entry.file_name().to_str().unwrap_or_default();
            let rel = rel_of(&walk_root, entry.path()).unwrap_or_default();
            rel.is_empty() || !is_hidden_dir(&rel, name)
        })
        .build();
    let mut out = Vec::new();
    for entry in walker {
        let Ok(entry) = entry else { continue };
        if !entry.file_type().is_some_and(|t| t.is_file()) {
            continue;
        }
        let has_ext = entry
            .path()
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| SEARCH_EXTENSIONS.iter().any(|x| x.eq_ignore_ascii_case(e)));
        if !has_ext {
            continue;
        }
        if let Some(rel) = rel_of(root, entry.path()) {
            out.push(rel);
        }
    }
    out.sort();
    Ok(out)
}

/// Cuts `line` to at most [`MAX_LINE_CHARS`] chars, keeping the first match
/// visible, and returns the merged match ranges within the excerpt.
fn excerpt(line: &str, word_res: &[Regex]) -> (String, Vec<(usize, usize)>) {
    let mut text = truncate_chars(line, MAX_LINE_CHARS);
    if text.len() < line.len() {
        let first = word_res
            .iter()
            .filter_map(|re| re.find(line))
            .map(|m| (m.start(), m.end()))
            .min();
        if let Some((start, end)) = first
            && end > text.len()
        {
            // Start the window ~40 chars before the first match.
            let before = line[..start]
                .chars()
                .rev()
                .take(40)
                .map(char::len_utf8)
                .sum::<usize>();
            let from = start - before;
            text = truncate_chars(&line[from..], MAX_LINE_CHARS);
        }
    }
    let mut ranges: Vec<(usize, usize)> = word_res
        .iter()
        .flat_map(|re| re.find_iter(text).map(|m| (m.start(), m.end())))
        .filter(|(s, e)| s < e)
        .collect();
    ranges.sort_unstable();
    let mut merged: Vec<(usize, usize)> = Vec::with_capacity(ranges.len());
    for (s, e) in ranges {
        match merged.last_mut() {
            Some(last) if s <= last.1 => last.1 = last.1.max(e),
            _ => merged.push((s, e)),
        }
    }
    (text.to_string(), merged)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
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

    fn marked(hit: &SearchHit) -> Vec<&str> {
        hit.ranges.iter().map(|&(s, e)| &hit.text[s..e]).collect()
    }

    #[test]
    fn empty_query_returns_nothing() {
        let v = vault_with(&[("a.md", "anything")]);
        let r = search(v.path(), "   ", 10).unwrap();
        assert!(r.hits.is_empty() && r.file_matches.is_empty() && !r.truncated);
    }

    #[test]
    fn smart_case() {
        let v = vault_with(&[("a.md", "Rollout plan\nrollout notes\nROLLOUT now\n")]);
        let r = search(v.path(), "rollout", 50).unwrap();
        assert_eq!(r.hits.len(), 3);
        assert_eq!(r.hits[0].line, 0);
        assert_eq!(r.hits[2].line, 2);
        assert_eq!(marked(&r.hits[2]), ["ROLLOUT"]);
        let r = search(v.path(), "Rollout", 50).unwrap();
        assert_eq!(r.hits.len(), 1);
        assert_eq!(r.hits[0].text, "Rollout plan");
    }

    #[test]
    fn literal_not_regex() {
        let v = vault_with(&[("a.md", "cost is $5 (approx.)\nprice 5\n")]);
        let r = search(v.path(), "$5 (approx.)", 50).unwrap();
        assert_eq!(r.hits.len(), 1);
        assert_eq!(r.hits[0].line, 0);
    }

    #[test]
    fn multiword_requires_all_words_on_line() {
        let v = vault_with(&[(
            "a.md",
            "  alex owns the rollout  \nrollout only\nalex only\nthe Rollout by Alex\n",
        )]);
        let r = search(v.path(), "rollout alex", 50).unwrap();
        let lines: Vec<_> = r.hits.iter().map(|h| h.line).collect();
        assert_eq!(lines, [0, 3]);
        assert_eq!(r.hits[0].text, "alex owns the rollout");
        assert_eq!(r.hits[0].ranges, [(0, 4), (14, 21)]);
        assert_eq!(marked(&r.hits[1]), ["Rollout", "Alex"]);
    }

    #[test]
    fn ranges_merge_overlaps_and_handle_unicode() {
        let v = vault_with(&[("a.md", "Ärger über ärger\naaaa\n")]);
        let r = search(v.path(), "ärger", 50).unwrap();
        assert_eq!(marked(&r.hits[0]), ["Ärger", "ärger"]);
        let r = search(v.path(), "aa aaa", 50).unwrap();
        assert_eq!(r.hits[0].ranges, [(0, 4)]);
    }

    #[test]
    fn long_lines_are_windowed_around_the_match() {
        let line = format!("{} needle {}", "x".repeat(500), "y".repeat(500));
        let v = vault_with(&[("a.md", &line)]);
        let r = search(v.path(), "needle", 50).unwrap();
        let hit = &r.hits[0];
        assert!(hit.text.chars().count() <= 300);
        assert_eq!(marked(hit), ["needle"]);
    }

    #[test]
    fn limits_and_truncation() {
        let many: String = (0..30).map(|i| format!("match {i}\n")).collect();
        let v = vault_with(&[("a.md", &many), ("b.md", &many), ("c.md", "match\n")]);
        let r = search(v.path(), "match", 100).unwrap();
        assert_eq!(r.hits.len(), 41);
        assert!(r.truncated);
        assert_eq!(r.hits.iter().filter(|h| h.file == "a.md").count(), 20);
        let r = search(v.path(), "match", 25).unwrap();
        assert_eq!(r.hits.len(), 25);
        assert!(r.truncated);
        let r = search(v.path(), "match", 0).unwrap();
        assert!(r.hits.is_empty());
        let r = search(v.path(), "match 3", 100).unwrap();
        assert!(!r.truncated);
        // "match 3" lines: 3, 13, 23 in a and b.
        assert_eq!(r.hits.len(), 6);
        let exact = vault_with(&[("a.md", "match\nmatch\n")]);
        let r = search(exact.path(), "match", 2).unwrap();
        assert_eq!(r.hits.len(), 2);
        assert!(!r.truncated);
    }

    #[test]
    fn filters_files_and_respects_gitignore() {
        let v = vault_with(&[
            ("note.md", "secret word"),
            ("doc.markdown", "secret word"),
            ("plain.txt", "secret word"),
            ("data.json", "secret word"),
            ("bin.md", "secret\0word"),
            (".vault/x.md", "secret word"),
            (".hidden/x.md", "secret word"),
            ("repos/r/README.md", "secret word"),
            ("node_modules/m/x.md", "secret word"),
            ("ignored/x.md", "secret word"),
            ("teams/repos/x.md", "secret word"),
            (".gitignore", "ignored/\n"),
        ]);
        let r = search(v.path(), "secret", 100).unwrap();
        let files: Vec<_> = r.hits.iter().map(|h| h.file.as_str()).collect();
        assert_eq!(
            files,
            ["doc.markdown", "note.md", "plain.txt", "teams/repos/x.md"]
        );
    }

    #[test]
    fn filename_matches_first() {
        let v = vault_with(&[
            ("a.md", "talk to alex"),
            ("people/Alex.md", "# Alex\nalex is great"),
            ("z.md", "alex"),
        ]);
        let r = search(v.path(), "alex", 100).unwrap();
        assert_eq!(r.file_matches, ["people/Alex.md"]);
        let files: Vec<_> = r.hits.iter().map(|h| h.file.as_str()).collect();
        assert_eq!(files, ["people/Alex.md", "people/Alex.md", "a.md", "z.md"]);
        // Path matching needs all words, case-insensitively.
        let r = search(v.path(), "PEOPLE alex", 100).unwrap();
        assert_eq!(r.file_matches, ["people/Alex.md"]);
        assert!(r.hits.is_empty());
        let r = search(v.path(), "people bob", 100).unwrap();
        assert!(r.file_matches.is_empty());
    }

    #[test]
    fn file_matches_capped() {
        let files: Vec<(String, &str)> = (0..25)
            .map(|i| (format!("people/p{i:02}.md"), ""))
            .collect();
        let refs: Vec<(&str, &str)> = files.iter().map(|(p, c)| (p.as_str(), *c)).collect();
        let v = vault_with(&refs);
        let r = search(v.path(), "people", 10).unwrap();
        assert_eq!(r.file_matches.len(), 20);
        assert_eq!(r.file_matches[0], "people/p00.md");
    }
}
