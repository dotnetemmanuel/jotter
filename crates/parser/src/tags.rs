//! Inline `#tag` scanning, code-aware like the wikilink scanner.

use crate::wikilink::dead_ranges;

/// Every inline tag in `src`, lowercased, deduplicated, in document order.
///
/// A tag starts at a `#` that follows whitespace or a line start and is followed
/// by a letter, so `# Heading` is a heading and `page#section` is a fragment.
/// Code, frontmatter, and raw HTML are skipped.
#[must_use]
pub fn scan(src: &str) -> Vec<String> {
    let dead = dead_ranges(src);
    let bytes = src.as_bytes();
    let mut found: Vec<String> = Vec::new();
    let mut i = 0;

    while i < bytes.len() {
        if let Some(end) = dead.iter().find(|range| range.contains(&i)).map(|r| r.end) {
            i = end;
            continue;
        }
        if bytes[i] != b'#' || !opens_tag(bytes, i) {
            i += 1;
            continue;
        }
        let start = i + 1;
        let end = start + body_length(&src[start..]);
        // A run of `#` at a line start is a heading marker, not a tag.
        if end > start {
            let tag = src[start..end].to_lowercase();
            if !found.contains(&tag) {
                found.push(tag);
            }
        }
        i = end.max(i + 1);
    }
    found
}

/// Whether the `#` at `at` can open a tag: preceded by nothing or by whitespace,
/// and followed by a letter.
fn opens_tag(bytes: &[u8], at: usize) -> bool {
    let before_is_clear = at == 0 || bytes[at - 1].is_ascii_whitespace();
    let after_is_letter = bytes
        .get(at + 1)
        .is_some_and(|byte| byte.is_ascii_alphabetic() || *byte >= 0x80);
    before_is_clear && after_is_letter
}

/// Length of the tag body: letters, digits, and `-`, `_`, `/`.
fn body_length(rest: &str) -> usize {
    rest.find(|c: char| !(c.is_alphanumeric() || matches!(c, '-' | '_' | '/')))
        .unwrap_or(rest.len())
}
