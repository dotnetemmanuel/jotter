//! Wikilink scanning: find every `[[target]]` in a markdown source, with the
//! byte range it occupies, skipping anything inside code.
//!
//! Resolution lives in the app, not here. This module only reports what the
//! source says, so the indexer, the renderer, and the editor click handling all
//! agree on where the links are.

use std::fmt::Write as _;
use std::ops::Range;

use comrak::nodes::NodeValue;
use comrak::{Anchorizer, Arena};

/// One `[[...]]` occurrence in a markdown source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Wikilink {
    /// Byte range of the whole `[[...]]`, including both bracket pairs.
    pub range: Range<usize>,
    /// Link target as written, trimmed: a note stem or a vault-relative path.
    pub target: String,
    /// Heading fragment after `#`, if any.
    pub heading: Option<String>,
    /// Display text after `|`, if any.
    pub alias: Option<String>,
}

impl Wikilink {
    /// The text the preview should show: the alias when present, else the
    /// target with its heading fragment.
    #[must_use]
    pub fn display_text(&self) -> String {
        if let Some(alias) = &self.alias {
            return alias.clone();
        }
        match &self.heading {
            Some(heading) => format!("{} > {heading}", self.target),
            None => self.target.clone(),
        }
    }
}

/// Find every wikilink in `src`, in document order.
///
/// Occurrences inside fenced, indented, or inline code are skipped, as are
/// frontmatter and raw HTML blocks. An embed (`![[x]]`) is not a link.
#[must_use]
pub fn scan(src: &str) -> Vec<Wikilink> {
    scan_all(src).0
}

/// Spans that look like a wikilink but are deliberately not one: embeds
/// (`![[x]]`) and backslash-escaped brackets (`\[[x]]`).
///
/// Code is excluded, since code already has styling of its own to keep. The
/// editor uses this to stop the markdown grammar half-highlighting them.
#[must_use]
pub fn scan_inert(src: &str) -> Vec<Range<usize>> {
    scan_all(src).1
}

/// Byte ranges where a `[[` means nothing: code blocks, frontmatter, raw HTML,
/// and inline code spans.
///
/// Unlike [`scan`], this needs no closing `]]`, so a half-typed link can be
/// tested against it. An unclosed backtick run counts to the end of its line:
/// mid-typing, the safe reading is that it is code.
#[must_use]
pub fn dead_ranges(src: &str) -> Vec<Range<usize>> {
    let mut ranges = opaque_ranges(src);
    let bytes = src.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if let Some(end) = ranges.iter().find(|r| r.contains(&i)).map(|r| r.end) {
            i = end;
            continue;
        }
        if bytes[i] == b'`' {
            let closed = skip_inline_code(bytes, i);
            let end = if closed == i + run_length(bytes, i) && !closes_run(bytes, i) {
                line_end(bytes, i)
            } else {
                closed
            };
            ranges.push(i..end);
            i = end;
        } else {
            i += 1;
        }
    }
    ranges.sort_by_key(|range| range.start);
    ranges
}

/// Whether the backtick run at `start` has a matching closing run.
fn closes_run(bytes: &[u8], start: usize) -> bool {
    let open_len = run_length(bytes, start);
    let mut i = start + open_len;
    while i < bytes.len() {
        if bytes[i] == b'`' {
            let len = run_length(bytes, i);
            if len == open_len {
                return true;
            }
            i += len;
        } else {
            i += 1;
        }
    }
    false
}

/// Offset just past the end of the line containing `from`.
fn line_end(bytes: &[u8], from: usize) -> usize {
    bytes[from..]
        .iter()
        .position(|&b| b == b'\n')
        .map_or(bytes.len(), |offset| from + offset)
}

/// One pass returning both the links and the lookalikes that are not links.
fn scan_all(src: &str) -> (Vec<Wikilink>, Vec<Range<usize>>) {
    let skip = opaque_ranges(src);
    let bytes = src.as_bytes();
    let mut links = Vec::new();
    let mut inert = Vec::new();
    let mut i = 0;

    while i < bytes.len() {
        if let Some(end) = skip.iter().find(|r| r.contains(&i)).map(|r| r.end) {
            i = end;
            continue;
        }
        match bytes[i] {
            // An escape hides the bracket that follows, so \[[x]] is not a link.
            b'\\' => {
                i = match opening_at(bytes, i + 1)
                    .then(|| parse_at(src, i + 1))
                    .flatten()
                {
                    Some(link) => {
                        let end = link.range.end;
                        inert.push(i..end);
                        end
                    }
                    None => i + 2,
                };
            }
            b'`' => i = skip_inline_code(bytes, i),
            b'[' if bytes.get(i + 1) == Some(&b'[') => {
                let embed = i > 0 && bytes[i - 1] == b'!';
                match parse_at(src, i) {
                    Some(link) if embed => {
                        i = link.range.end;
                        inert.push(i - link.range.len() - 1..i);
                    }
                    Some(link) => {
                        i = link.range.end;
                        links.push(link);
                    }
                    None => i += 2,
                }
            }
            _ => i += 1,
        }
    }

    (links, inert)
}

/// Whether a `[[` opens at `at`.
fn opening_at(bytes: &[u8], at: usize) -> bool {
    bytes.get(at) == Some(&b'[') && bytes.get(at + 1) == Some(&b'[')
}

/// Parse a wikilink starting at `open`, where `src[open..]` begins with `[[`.
fn parse_at(src: &str, open: usize) -> Option<Wikilink> {
    let rest = &src[open + 2..];
    let close = rest.find("]]")?;
    let body = &rest[..close];
    // A link never spans lines, and nested brackets mean this is not one.
    if body.contains(['\n', '[', ']']) {
        return None;
    }

    let (link_part, alias) = match body.split_once('|') {
        Some((link, alias)) => (link, non_empty(alias)),
        None => (body, None),
    };
    let (target, heading) = match link_part.split_once('#') {
        Some((target, heading)) => (target, non_empty(heading)),
        None => (link_part, None),
    };
    let target = target.trim();
    if target.is_empty() {
        return None;
    }

    Some(Wikilink {
        range: open..open + 2 + close + 2,
        target: target.to_owned(),
        heading,
        alias,
    })
}

/// Trim a fragment, returning `None` when nothing is left.
fn non_empty(part: &str) -> Option<String> {
    let trimmed = part.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

/// Skip an inline code span opening at `start`, per the `CommonMark` rule that a
/// run of n backticks closes on the next run of exactly n. An unclosed run is
/// literal text, so only the run itself is skipped.
fn skip_inline_code(bytes: &[u8], start: usize) -> usize {
    let open_len = run_length(bytes, start);
    let mut i = start + open_len;
    while i < bytes.len() {
        if bytes[i] == b'`' {
            let len = run_length(bytes, i);
            if len == open_len {
                return i + len;
            }
            i += len;
        } else {
            i += 1;
        }
    }
    start + open_len
}

/// Length of the backtick run starting at `start`.
fn run_length(bytes: &[u8], start: usize) -> usize {
    bytes[start..]
        .iter()
        .take_while(|&&b| b == b'`')
        .count()
        .max(1)
}

/// Byte ranges that a wikilink can never appear in: code blocks, frontmatter,
/// and raw HTML blocks. Taken from comrak's block-level source positions, which
/// is why indented code needs no hand-rolled detection here.
fn opaque_ranges(src: &str) -> Vec<Range<usize>> {
    let arena = Arena::new();
    let root = comrak::parse_document(&arena, src, &crate::base_options());
    let starts = line_starts(src);
    let mut ranges = Vec::new();

    for node in root.descendants() {
        let data = node.data.borrow();
        if !matches!(
            data.value,
            NodeValue::CodeBlock(_) | NodeValue::FrontMatter(_) | NodeValue::HtmlBlock(_)
        ) {
            continue;
        }
        if let Some(range) = byte_range(&data.sourcepos, &starts, src.len()) {
            ranges.push(range);
        }
    }

    ranges
}

/// Byte offset of the first character of each line, indexed by line - 1.
fn line_starts(src: &str) -> Vec<usize> {
    let mut starts = vec![0];
    starts.extend(src.match_indices('\n').map(|(i, _)| i + 1));
    starts
}

/// Convert a comrak source position to a byte range, clamped to the source.
/// Comrak reports 1-based lines with inclusive 1-based byte columns.
fn byte_range(
    pos: &comrak::nodes::Sourcepos,
    starts: &[usize],
    len: usize,
) -> Option<Range<usize>> {
    let start_line = starts.get(pos.start.line.checked_sub(1)?)?;
    let start = (start_line + pos.start.column.saturating_sub(1)).min(len);
    let end_line = starts.get(pos.end.line.checked_sub(1)?)?;
    let end = (end_line + pos.end.column).min(len);
    (start < end).then_some(start..end)
}

/// Answers where a wikilink target points, if anywhere.
///
/// Implemented by the app over its note index. Any `Fn(&str) -> Option<String>`
/// is a resolver too, which keeps tests here free of a database.
pub trait LinkResolver {
    /// Vault-relative path the target resolves to, or `None` when it is broken.
    fn resolve(&self, target: &str) -> Option<String>;
}

impl<F: Fn(&str) -> Option<String>> LinkResolver for F {
    fn resolve(&self, target: &str) -> Option<String> {
        self(target)
    }
}

/// URI scheme for a wikilink that resolved to an existing note.
pub const NOTE_SCHEME: &str = "jotter-note:";
/// URI scheme for a wikilink whose target does not exist yet.
pub const NEW_SCHEME: &str = "jotter-new:";

/// Rewrite every wikilink in `src` to an inline markdown link, leaving the rest
/// of the source untouched. Line numbering is preserved: a replacement never
/// contains a newline, so heading source lines stay valid.
#[must_use]
pub fn rewrite(src: &str, resolver: &dyn LinkResolver) -> String {
    let links = scan(src);
    if links.is_empty() {
        return src.to_owned();
    }

    let mut out = String::with_capacity(src.len());
    let mut cursor = 0;
    for link in links {
        out.push_str(&src[cursor..link.range.start]);
        out.push_str(&markdown_link(&link, resolver));
        cursor = link.range.end;
    }
    out.push_str(&src[cursor..]);
    out
}

/// Render one wikilink as `[text](scheme:destination)`.
fn markdown_link(link: &Wikilink, resolver: &dyn LinkResolver) -> String {
    let href = match resolver.resolve(&link.target) {
        Some(path) => {
            let mut href = format!("{NOTE_SCHEME}{}", percent_encode(&path));
            if let Some(heading) = &link.heading {
                href.push('#');
                href.push_str(&percent_encode(&anchor_slug(heading)));
            }
            href
        }
        None => format!("{NEW_SCHEME}{}", percent_encode(&link.target)),
    };
    format!("[{}]({href})", escape_link_text(&link.display_text()))
}

/// Slug for a heading fragment, matching the ids comrak emits for headings.
#[must_use]
pub fn anchor_slug(heading: &str) -> String {
    Anchorizer::new().anchorize(heading)
}

/// Percent-encode everything outside the unreserved set, keeping `/` readable.
fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' | b'/' => {
                out.push(byte as char);
            }
            _ => write!(out, "%{byte:02X}").expect("writing to a String never fails"),
        }
    }
    out
}

/// Escape the characters that would otherwise be markdown syntax in link text.
fn escape_link_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        if matches!(ch, '\\' | '[' | ']' | '*' | '_' | '`') {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn targets(src: &str) -> Vec<String> {
        scan(src).into_iter().map(|l| l.target).collect()
    }

    #[test]
    fn plain_link_reports_target_and_exact_range() {
        let src = "see [[standup]] today";
        let links = scan(src);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].target, "standup");
        assert_eq!(&src[links[0].range.clone()], "[[standup]]");
        assert!(links[0].alias.is_none());
        assert!(links[0].heading.is_none());
    }

    #[test]
    fn alias_and_heading_are_split_out() {
        let links = scan("[[work/standup#Agenda|today]]");
        assert_eq!(links[0].target, "work/standup");
        assert_eq!(links[0].heading.as_deref(), Some("Agenda"));
        assert_eq!(links[0].alias.as_deref(), Some("today"));
    }

    #[test]
    fn parts_are_trimmed() {
        let links = scan("[[  standup  #  Agenda  |  today  ]]");
        assert_eq!(links[0].target, "standup");
        assert_eq!(links[0].heading.as_deref(), Some("Agenda"));
        assert_eq!(links[0].alias.as_deref(), Some("today"));
    }

    #[test]
    fn display_text_prefers_alias_then_heading() {
        assert_eq!(scan("[[a|b]]")[0].display_text(), "b");
        assert_eq!(scan("[[a#h]]")[0].display_text(), "a > h");
        assert_eq!(scan("[[a]]")[0].display_text(), "a");
    }

    #[test]
    fn several_links_on_one_line_keep_document_order() {
        assert_eq!(targets("[[a]] and [[b]] and [[c]]"), ["a", "b", "c"]);
    }

    #[test]
    fn fenced_code_is_skipped() {
        let src = "before [[real]]\n\n```\n[[fake]]\n```\n\nafter [[also-real]]\n";
        assert_eq!(targets(src), ["real", "also-real"]);
    }

    #[test]
    fn tilde_fenced_code_is_skipped() {
        assert_eq!(targets("~~~\n[[fake]]\n~~~\n\n[[real]]\n"), ["real"]);
    }

    #[test]
    fn indented_code_is_skipped() {
        assert_eq!(targets("text\n\n    [[fake]]\n\ntext [[real]]\n"), ["real"]);
    }

    #[test]
    fn inline_code_is_skipped() {
        assert_eq!(targets("`[[fake]]` but [[real]]"), ["real"]);
    }

    #[test]
    fn multi_backtick_inline_code_is_skipped() {
        assert_eq!(targets("``a ` [[fake]]`` [[real]]"), ["real"]);
    }

    #[test]
    fn unclosed_backtick_does_not_swallow_the_rest() {
        assert_eq!(targets("` unclosed [[real]]"), ["real"]);
    }

    #[test]
    fn frontmatter_is_skipped() {
        assert_eq!(targets("---\ntitle: [[fake]]\n---\n\n[[real]]\n"), ["real"]);
    }

    #[test]
    fn html_block_is_skipped() {
        assert_eq!(targets("<div>\n[[fake]]\n</div>\n\n[[real]]\n"), ["real"]);
    }

    #[test]
    fn escaped_bracket_is_not_a_link() {
        assert!(scan(r"\[[not a link]]").is_empty());
    }

    #[test]
    fn embed_syntax_is_not_a_link() {
        assert!(scan("![[picture]]").is_empty());
    }

    #[test]
    fn inert_spans_cover_the_whole_lookalike() {
        let src = r"a ![[pic]] b \[[esc]] c";
        let inert = scan_inert(src);
        let covered: Vec<&str> = inert.iter().map(|r| &src[r.clone()]).collect();
        assert_eq!(covered, [r"![[pic]]", r"\[[esc]]"]);
    }

    #[test]
    fn real_links_are_not_inert() {
        assert!(scan_inert("[[real]]").is_empty());
    }

    #[test]
    fn code_is_not_reported_as_inert() {
        assert!(scan_inert("`[[x]]`\n\n```\n![[y]]\n```\n").is_empty());
    }

    #[test]
    fn a_link_after_an_escape_still_scans() {
        let links = scan(r"\[[esc]] then [[real]]");
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].target, "real");
    }

    #[test]
    fn unclosed_link_is_ignored() {
        assert!(scan("[[dangling").is_empty());
    }

    #[test]
    fn empty_target_is_not_a_link() {
        assert!(scan("[[]] and [[   ]] and [[|alias]]").is_empty());
    }

    #[test]
    fn link_never_spans_lines() {
        assert!(scan("[[open\nclosed]]").is_empty());
    }

    #[test]
    fn nested_brackets_are_not_a_link() {
        assert!(scan("[[a [b] c]]").is_empty());
    }

    #[test]
    fn markdown_link_is_not_a_wikilink() {
        assert!(scan("[label](target.md)").is_empty());
    }

    #[test]
    fn ranges_are_correct_after_multibyte_text() {
        let src = "café ☕ [[standup]]";
        let links = scan(src);
        assert_eq!(&src[links[0].range.clone()], "[[standup]]");
    }

    #[test]
    fn link_inside_a_list_item_is_found() {
        assert_eq!(targets("- item [[real]]\n- other\n"), ["real"]);
    }

    #[test]
    fn code_block_inside_a_list_item_is_skipped() {
        assert_eq!(
            targets("- item\n\n  ```\n  [[fake]]\n  ```\n\n[[real]]\n"),
            ["real"]
        );
    }

    /// Resolves any target whose stem is "standup", to a path with a space in it.
    fn stub(target: &str) -> Option<String> {
        (target.eq_ignore_ascii_case("standup")).then(|| "work/team standup.md".to_owned())
    }

    #[test]
    fn resolved_link_uses_the_note_scheme_with_encoded_path() {
        assert_eq!(
            rewrite("see [[standup]]", &stub),
            "see [standup](jotter-note:work/team%20standup.md)"
        );
    }

    #[test]
    fn broken_link_uses_the_new_scheme_with_the_raw_target() {
        assert_eq!(
            rewrite("see [[not yet]]", &stub),
            "see [not yet](jotter-new:not%20yet)"
        );
    }

    #[test]
    fn alias_becomes_the_link_text() {
        assert_eq!(
            rewrite("[[standup|today]]", &stub),
            "[today](jotter-note:work/team%20standup.md)"
        );
    }

    #[test]
    fn heading_becomes_an_anchor_slug() {
        assert_eq!(
            rewrite("[[standup#Next Steps]]", &stub),
            "[standup > Next Steps](jotter-note:work/team%20standup.md#next-steps)"
        );
    }

    #[test]
    fn heading_on_a_broken_link_is_dropped() {
        assert_eq!(
            rewrite("[[gone#Head]]", &stub),
            "[gone > Head](jotter-new:gone)"
        );
    }

    #[test]
    fn rewrite_leaves_code_and_other_text_alone() {
        let src = "keep `[[standup]]` and\n\n```\n[[standup]]\n```\n\nplain text\n";
        assert_eq!(rewrite(src, &stub), src);
    }

    #[test]
    fn rewrite_preserves_line_count() {
        let src = "a\n[[standup]]\nb\n[[gone]]\nc\n";
        let out = rewrite(src, &stub);
        assert_eq!(src.lines().count(), out.lines().count());
    }

    #[test]
    fn markdown_syntax_in_the_alias_is_escaped() {
        assert_eq!(
            rewrite("[[standup|a *b* c_d]]", &stub),
            r"[a \*b\* c\_d](jotter-note:work/team%20standup.md)"
        );
    }
}
