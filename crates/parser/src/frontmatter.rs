//! Frontmatter parsing: the leading YAML, TOML, or JSON block of a note.
//!
//! Only the keys jotter acts on are lifted out; the whole block is kept raw so
//! nothing a user wrote is lost.

use std::collections::HashMap;

use gray_matter::engine::{JSON, TOML, YAML};
use gray_matter::{Matter, Pod};

/// What a note's frontmatter block says.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Frontmatter {
    /// Display title, overriding the first H1 and the filename.
    pub title: Option<String>,
    /// Tags declared in the block, without a leading `#`.
    pub tags: Vec<String>,
    /// Alternative names for the note. Parsed and stored, not yet resolved.
    pub aliases: Vec<String>,
    /// Creation timestamp exactly as written.
    pub created: Option<String>,
    /// Update timestamp exactly as written.
    pub updated: Option<String>,
    /// The block as written, for round-tripping keys jotter does not know.
    pub raw: Option<String>,
}

/// Parses the frontmatter block of `src`, if it has one.
///
/// A block only counts when it opens the document. Malformed frontmatter yields
/// defaults rather than an error: a half-typed block is normal while editing.
#[must_use]
pub fn parse(src: &str) -> Frontmatter {
    let Some((data, raw)) = parse_block(src) else {
        return Frontmatter::default();
    };

    Frontmatter {
        title: string_at(&data, "title"),
        tags: list_at(&data, "tags")
            .iter()
            .map(|tag| strip_hash(tag))
            .collect(),
        aliases: list_at(&data, "aliases"),
        created: string_at(&data, "created"),
        updated: string_at(&data, "updated"),
        raw: Some(raw),
    }
}

/// Runs the engine matching the opening delimiter, returning the keys and the
/// raw block.
fn parse_block(src: &str) -> Option<(HashMap<String, Pod>, String)> {
    if src.starts_with("+++") {
        let mut matter = Matter::<TOML>::new();
        matter.delimiter = "+++".to_string();
        return entity(matter.parse::<Pod>(src).ok()?);
    }
    if !src.starts_with("---") {
        return None;
    }
    let yaml = Matter::<YAML>::new();
    if let Some(found) = yaml.parse::<Pod>(src).ok().and_then(entity) {
        return Some(found);
    }
    // A `---` block holding an object is JSON, which the YAML engine rejects.
    entity(Matter::<JSON>::new().parse::<Pod>(src).ok()?)
}

/// The keys and raw text of a parsed entity, or `None` when the block was empty.
///
/// Indexing a `Pod` by a missing key panics, so the keys become a map up front.
fn entity(parsed: gray_matter::ParsedEntity<Pod>) -> Option<(HashMap<String, Pod>, String)> {
    let data = parsed.data?.as_hashmap().ok()?;
    Some((data, parsed.matter))
}

/// A trimmed string value at `key`, or `None` when absent or empty.
fn string_at(data: &HashMap<String, Pod>, key: &str) -> Option<String> {
    let value = data.get(key)?.as_string().ok()?;
    let trimmed = value.trim();
    (!trimmed.is_empty() && trimmed != "~").then(|| trimmed.to_string())
}

/// Values at `key`, accepting either a list or a single string.
fn list_at(data: &HashMap<String, Pod>, key: &str) -> Vec<String> {
    let Some(value) = data.get(key) else {
        return Vec::new();
    };
    if let Ok(items) = value.as_vec() {
        return items
            .iter()
            .filter_map(|item| item.as_string().ok())
            .map(|item| item.trim().to_string())
            .filter(|item| !item.is_empty())
            .collect();
    }
    string_at(data, key).into_iter().collect()
}

/// Tags may be written with or without the `#` they carry inline.
fn strip_hash(tag: &str) -> String {
    tag.strip_prefix('#').unwrap_or(tag).to_string()
}
