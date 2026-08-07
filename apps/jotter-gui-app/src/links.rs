//! Wikilink target resolution over the notes the index knows about.
//!
//! A target is either a vault-relative path (`work/standup`) or a bare filename
//! stem matched anywhere in the vault (`standup`). Matching is case-insensitive
//! and a `.md` suffix is optional. When two notes share a stem the
//! lexicographically first path wins, so the same link always resolves the same
//! way regardless of index order.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use jotter_parser::wikilink::LinkResolver;

/// Vault-relative note paths, indexed for wikilink lookup.
#[derive(Debug, Default)]
pub struct Resolver {
    /// Lowercased stem to the sorted paths that carry it.
    by_stem: HashMap<String, Vec<String>>,
    /// Lowercased path to the path as stored.
    by_path: HashMap<String, String>,
}

impl Resolver {
    /// Build a resolver from every note path in the vault.
    pub fn new(paths: impl IntoIterator<Item = String>) -> Self {
        let mut resolver = Self::default();
        for path in paths {
            if let Some(stem) = stem_of(&path) {
                resolver
                    .by_stem
                    .entry(stem.to_lowercase())
                    .or_default()
                    .push(path.clone());
            }
            resolver.by_path.insert(path.to_lowercase(), path);
        }
        for paths in resolver.by_stem.values_mut() {
            paths.sort();
        }
        resolver
    }

    /// The path `target` points at, or `None` when no note matches.
    #[must_use]
    pub fn lookup(&self, target: &str) -> Option<String> {
        let normalized = normalize_target(target);
        if normalized.is_empty() {
            return None;
        }
        if normalized.contains('/') {
            return self.by_path.get(&format!("{normalized}.md")).cloned();
        }
        self.by_stem
            .get(&normalized)
            .and_then(|paths| paths.first())
            .cloned()
    }

    /// Existing notes a broken `target` was plausibly a typo for, best first.
    ///
    /// Matching ignores case and the separators people mistype (spaces, dashes,
    /// underscores), then allows a few character edits scaled to target length.
    #[must_use]
    pub fn suggestions(&self, target: &str, limit: usize) -> Vec<String> {
        let needle = squash(&normalize_target(target));
        if needle.is_empty() {
            return Vec::new();
        }
        let budget = edit_budget(needle.len());

        let mut scored: Vec<(usize, &String)> = self
            .by_path
            .values()
            .filter_map(|path| {
                let stripped = normalize_target(path);
                let by_stem = stem_of(path).map(|stem| distance(&needle, &squash(stem)));
                let by_path = distance(&needle, &squash(&stripped));
                let best = by_stem.map_or(by_path, |d| d.min(by_path));
                (best <= budget).then_some((best, path))
            })
            .collect();

        scored.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(b.1)));
        scored
            .into_iter()
            .take(limit)
            .map(|(_, p)| p.clone())
            .collect()
    }

    /// The shortest target that unambiguously points at `path`.
    ///
    /// A bare stem when it resolves back to the same note, else the full path,
    /// so a rewritten link stays as readable as the vault allows.
    #[must_use]
    pub fn shortest_target(&self, path: &str) -> String {
        let stripped = path.strip_suffix(".md").unwrap_or(path);
        match stem_of(path) {
            Some(stem) if self.lookup(stem).as_deref() == Some(path) => stem.to_owned(),
            _ => stripped.to_owned(),
        }
    }
}

/// Render a wikilink back to source text, keeping its heading and alias.
#[must_use]
pub fn format_wikilink(target: &str, heading: Option<&str>, alias: Option<&str>) -> String {
    let mut out = format!("[[{target}");
    if let Some(heading) = heading {
        out.push('#');
        out.push_str(heading);
    }
    if let Some(alias) = alias {
        out.push('|');
        out.push_str(alias);
    }
    out.push_str("]]");
    out
}

impl LinkResolver for Resolver {
    fn resolve(&self, target: &str) -> Option<String> {
        self.lookup(target)
    }
}

/// What a clicked link in the preview refers to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkTarget {
    /// A wikilink that resolved to a note, with an optional heading anchor.
    Note {
        /// Vault-relative path of the note to open.
        path: PathBuf,
        /// Heading anchor to scroll to, if the link named one.
        anchor: Option<String>,
    },
    /// A wikilink whose target does not exist yet, carrying the raw target.
    New(String),
    /// Anything else: an ordinary web link, for the system browser.
    External(String),
}

/// Classify a uri handed back by the preview when the reader clicks a link.
#[must_use]
pub fn parse_uri(uri: &str) -> LinkTarget {
    if let Some(rest) = uri.strip_prefix(jotter_parser::wikilink::NOTE_SCHEME) {
        let (path, anchor) = match rest.split_once('#') {
            Some((path, anchor)) => (path, Some(percent_decode(anchor))),
            None => (rest, None),
        };
        return LinkTarget::Note {
            path: PathBuf::from(percent_decode(path)),
            anchor: anchor.filter(|a| !a.is_empty()),
        };
    }
    if let Some(target) = uri.strip_prefix(jotter_parser::wikilink::NEW_SCHEME) {
        return LinkTarget::New(percent_decode(target));
    }
    LinkTarget::External(uri.to_owned())
}

/// Decode percent escapes, leaving malformed sequences as written.
fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(hex) = text.get(i + 1..i + 3)
            && let Ok(byte) = u8::from_str_radix(hex, 16)
        {
            out.push(byte);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Where a note for a broken `target` should be created, vault-relative.
///
/// A target with a slash names a path from the vault root; a bare target lands
/// beside the note that links to it.
#[must_use]
pub fn new_note_path(target: &str, source: Option<&Path>) -> PathBuf {
    let normalized = normalize_target(target);
    let file = format!("{normalized}.md");
    if normalized.contains('/') {
        return PathBuf::from(file);
    }
    match source
        .and_then(Path::parent)
        .filter(|p| !p.as_os_str().is_empty())
    {
        Some(dir) => dir.join(file),
        None => PathBuf::from(file),
    }
}

/// Lowercase a target and strip the decorations that do not affect matching.
fn normalize_target(target: &str) -> String {
    let cleaned = target.trim().replace('\\', "/");
    let cleaned = cleaned.strip_prefix("./").unwrap_or(&cleaned);
    let cleaned = cleaned
        .strip_suffix(".md")
        .or_else(|| cleaned.strip_suffix(".MD"))
        .unwrap_or(cleaned);
    cleaned.trim_matches('/').to_lowercase()
}

/// The filename stem of a vault-relative path.
pub fn stem_of(path: &str) -> Option<&str> {
    let file = path.rsplit('/').next()?;
    Some(file.strip_suffix(".md").unwrap_or(file))
}

/// Drop the separators that typos move around, so "meeting-notes" and
/// "meeting notes" compare equal.
fn squash(text: &str) -> String {
    text.chars()
        .filter(|c| !matches!(c, ' ' | '-' | '_'))
        .flat_map(char::to_lowercase)
        .collect()
}

/// How many character edits still count as "probably the same word".
fn edit_budget(len: usize) -> usize {
    match len {
        0..=3 => 1,
        4..=8 => 2,
        _ => 3,
    }
}

/// Levenshtein distance between two strings, over chars.
fn distance(a: &str, b: &str) -> usize {
    let b_chars: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b_chars.len()).collect();
    let mut row = vec![0; b_chars.len() + 1];

    for (i, a_ch) in a.chars().enumerate() {
        row[0] = i + 1;
        for (j, &b_ch) in b_chars.iter().enumerate() {
            let cost = usize::from(a_ch != b_ch);
            row[j + 1] = (prev[j] + cost).min(prev[j + 1] + 1).min(row[j] + 1);
        }
        std::mem::swap(&mut prev, &mut row);
    }

    prev[b_chars.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resolver() -> Resolver {
        Resolver::new(
            [
                "work/standup.md",
                "personal/standup.md",
                "Meeting Notes.md",
                "archive/2024/retro.md",
            ]
            .into_iter()
            .map(str::to_owned),
        )
    }

    #[test]
    fn bare_stem_matches_anywhere_in_the_vault() {
        assert_eq!(
            resolver().lookup("retro").as_deref(),
            Some("archive/2024/retro.md")
        );
    }

    #[test]
    fn stem_matching_ignores_case() {
        assert_eq!(
            resolver().lookup("meeting notes").as_deref(),
            Some("Meeting Notes.md")
        );
    }

    #[test]
    fn colliding_stems_resolve_to_the_first_path() {
        assert_eq!(
            resolver().lookup("standup").as_deref(),
            Some("personal/standup.md")
        );
    }

    #[test]
    fn a_path_target_disambiguates() {
        assert_eq!(
            resolver().lookup("work/standup").as_deref(),
            Some("work/standup.md")
        );
    }

    #[test]
    fn the_md_suffix_is_optional() {
        assert_eq!(
            resolver().lookup("work/standup.md").as_deref(),
            Some("work/standup.md")
        );
        assert_eq!(
            resolver().lookup("retro.md").as_deref(),
            Some("archive/2024/retro.md")
        );
    }

    #[test]
    fn unknown_and_empty_targets_do_not_resolve() {
        assert!(resolver().lookup("nope").is_none());
        assert!(resolver().lookup("   ").is_none());
        assert!(resolver().lookup("work/nope").is_none());
    }

    #[test]
    fn suggestions_catch_a_one_character_typo() {
        assert_eq!(
            resolver().suggestions("standp", 5),
            ["personal/standup.md", "work/standup.md"]
        );
    }

    #[test]
    fn suggestions_ignore_separator_style() {
        assert_eq!(
            resolver().suggestions("meeting-notes", 5),
            ["Meeting Notes.md"]
        );
    }

    #[test]
    fn suggestions_are_empty_when_nothing_is_close() {
        assert!(resolver().suggestions("completely different", 5).is_empty());
    }

    #[test]
    fn suggestions_respect_the_limit() {
        assert_eq!(resolver().suggestions("standup", 1).len(), 1);
    }

    #[test]
    fn shortest_target_is_the_stem_when_unambiguous() {
        assert_eq!(resolver().shortest_target("archive/2024/retro.md"), "retro");
    }

    #[test]
    fn shortest_target_falls_back_to_the_path_on_a_collision() {
        assert_eq!(
            resolver().shortest_target("work/standup.md"),
            "work/standup"
        );
    }

    #[test]
    fn format_wikilink_keeps_heading_and_alias() {
        assert_eq!(format_wikilink("a", None, None), "[[a]]");
        assert_eq!(format_wikilink("a", Some("h"), None), "[[a#h]]");
        assert_eq!(format_wikilink("a", None, Some("x")), "[[a|x]]");
        assert_eq!(format_wikilink("a", Some("h"), Some("x")), "[[a#h|x]]");
    }

    #[test]
    fn note_uri_decodes_path_and_anchor() {
        assert_eq!(
            parse_uri("jotter-note:work/team%20standup.md#next-steps"),
            LinkTarget::Note {
                path: PathBuf::from("work/team standup.md"),
                anchor: Some("next-steps".to_owned()),
            }
        );
    }

    #[test]
    fn note_uri_without_an_anchor_has_none() {
        assert_eq!(
            parse_uri("jotter-note:a.md"),
            LinkTarget::Note {
                path: PathBuf::from("a.md"),
                anchor: None
            }
        );
    }

    #[test]
    fn new_uri_carries_the_raw_target() {
        assert_eq!(
            parse_uri("jotter-new:not%20yet%20written"),
            LinkTarget::New("not yet written".to_owned())
        );
    }

    #[test]
    fn other_schemes_are_external() {
        assert_eq!(
            parse_uri("https://example.com/a?b=c"),
            LinkTarget::External("https://example.com/a?b=c".to_owned())
        );
    }

    #[test]
    fn malformed_escapes_survive_decoding() {
        assert_eq!(
            parse_uri("jotter-new:100%"),
            LinkTarget::New("100%".to_owned())
        );
        assert_eq!(
            parse_uri("jotter-new:a%zz"),
            LinkTarget::New("a%zz".to_owned())
        );
    }

    #[test]
    fn multibyte_escapes_decode_to_utf8() {
        assert_eq!(
            parse_uri("jotter-new:caf%C3%A9"),
            LinkTarget::New("café".to_owned())
        );
    }

    #[test]
    fn new_note_lands_beside_the_source_note() {
        let path = new_note_path("ideas", Some(Path::new("work/standup.md")));
        assert_eq!(path, Path::new("work/ideas.md"));
    }

    #[test]
    fn new_note_with_a_path_target_is_vault_relative() {
        let path = new_note_path("notes/ideas", Some(Path::new("work/standup.md")));
        assert_eq!(path, Path::new("notes/ideas.md"));
    }

    #[test]
    fn new_note_without_a_source_lands_at_the_root() {
        assert_eq!(new_note_path("ideas", None), Path::new("ideas.md"));
        assert_eq!(
            new_note_path("ideas", Some(Path::new("top.md"))),
            Path::new("ideas.md")
        );
    }
}
