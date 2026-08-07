#![warn(clippy::pedantic)]
//! Markdown rendering for jotter: comrak with GFM extensions, syntect code
//! highlighting driven by the active theme, and a source-line to heading-anchor
//! map for scroll synchronization between the editor and the preview.

pub mod codeblock;
pub mod frontmatter;
pub mod tags;
pub mod wikilink;

use std::fmt::Write as _;

use comrak::nodes::{AstNode, NodeValue};
use comrak::{Anchorizer, Arena, Options};
use jotter_theming::Code;
use syntect::highlighting::{Color, ScopeSelectors, StyleModifier, Theme, ThemeItem};

pub use wikilink::Wikilink;

/// Finds where an image `src` written in a note actually lives on disk.
///
/// The parser never touches the filesystem, exactly as wikilink resolution is
/// left to a [`wikilink::LinkResolver`]. `None` leaves the src as written.
pub trait ImageLocator {
    /// Absolute path for `src`, or `None` to leave it alone.
    fn locate(&self, src: &str) -> Option<std::path::PathBuf>;
}

/// An [`ImageLocator`] that resolves against one folder and looks no further,
/// which is all a single opened file can do.
pub struct RelativeTo(pub std::path::PathBuf);

impl ImageLocator for RelativeTo {
    fn locate(&self, src: &str) -> Option<std::path::PathBuf> {
        Some(self.0.join(src))
    }
}

/// An [`ImageLocator`] that finds nothing, for rendering with no note behind it.
pub struct NoImages;

impl ImageLocator for NoImages {
    fn locate(&self, _: &str) -> Option<std::path::PathBuf> {
        None
    }
}

/// A rendered document: HTML plus a source-line to heading-anchor map.
pub struct Rendered {
    /// The rendered HTML fragment.
    pub html: String,
    /// Every heading in document order, mapping source line to anchor id.
    pub headings: Vec<HeadingAnchor>,
}

/// One heading, mapping its source line to the anchor id emitted in the HTML.
pub struct HeadingAnchor {
    /// 1-based line of the heading in the ORIGINAL source (frontmatter counts).
    pub source_line: usize,
    /// Id attribute set on the heading element in the HTML.
    pub anchor: String,
    /// Heading level, 1..=6.
    pub level: u8,
}

/// Build the comrak options shared by both render paths: GFM extensions on,
/// heading ids with an empty prefix, and YAML frontmatter stripped so heading
/// source line numbers stay aligned with the original file lines.
fn base_options() -> Options<'static> {
    let mut options = Options::default();
    options.extension.table = true;
    options.extension.strikethrough = true;
    options.extension.tasklist = true;
    options.extension.autolink = true;
    options.extension.footnotes = true;
    options.extension.header_id_prefix = Some(String::new());
    options.extension.front_matter_delimiter = Some("---".into());
    // Raw HTML in a note is rendered rather than escaped: people write tables
    // and the odd <br> by hand and expect to see them. The preview runs with
    // markup JavaScript disabled, so a <script> in a note still does nothing.
    options.render.r#unsafe = true;
    options
}

/// Render markdown to an HTML fragment with a heading anchor map.
///
/// Wikilinks are rewritten to links through `resolver` first, then fenced code
/// blocks are highlighted with a syntect theme derived from `code`.
#[must_use]
pub fn render(
    src: &str,
    code: &Code,
    resolver: &dyn wikilink::LinkResolver,
    images: &dyn ImageLocator,
) -> Rendered {
    let src = wikilink::rewrite(src, resolver);
    let options = base_options();
    let arena = Arena::new();
    let root = comrak::parse_document(&arena, &src, &options);

    absolutize_images(root, images);

    let headings = collect_headings(root);

    let adapter = CodeAdapter {
        theme: theme_from_code(code),
    };
    let mut plugins = comrak::options::Plugins::default();
    plugins.render.codefence_syntax_highlighter = Some(&adapter);

    let mut html = String::new();
    // format_document_with_plugins only fails if the Write sink fails; a String never does.
    let _ = comrak::html::format_document_with_plugins(root, &options, &mut html, &plugins);

    Rendered { html, headings }
}

/// Render markdown to an HTML fragment only, with no code coloring or anchor map.
#[must_use]
pub fn markdown_to_html(src: &str) -> String {
    let options = base_options();
    comrak::markdown_to_html(src, &options)
}

/// Rewrite relative image sources to absolute `file://` URIs against `dir`.
///
/// The preview loads each render from a file in the cache directory, so a
/// relative `src` would otherwise resolve against that directory rather than the
/// note. Only the in-memory document is touched: the note on disk keeps the
/// relative path it was written with, so other editors still read it.
fn absolutize_images<'a>(root: &'a AstNode<'a>, images: &dyn ImageLocator) {
    for node in root.descendants() {
        let mut data = node.data.borrow_mut();
        match &mut data.value {
            NodeValue::Image(image) => {
                if let Some(absolute) = absolute_file_uri(&image.url, images) {
                    image.url = absolute;
                }
            }
            // An <img> written by hand carries its src in raw HTML, which the AST
            // keeps as one opaque literal, so the attribute is rewritten in place.
            NodeValue::HtmlBlock(block) => {
                if let Some(rewritten) = rewrite_html_srcs(&block.literal, images) {
                    block.literal = rewritten;
                }
            }
            NodeValue::HtmlInline(literal) => {
                if let Some(rewritten) = rewrite_html_srcs(literal, images) {
                    *literal = rewritten;
                }
            }
            _ => {}
        }
    }
}

/// Rewrite every relative `src` attribute in a raw HTML literal, or `None` when
/// nothing needed changing.
///
/// Deliberately a scan for one attribute rather than HTML parsing: the literal is
/// whatever the author typed, and only `src` decides where an image loads from.
fn rewrite_html_srcs(html: &str, images: &dyn ImageLocator) -> Option<String> {
    let bytes = html.as_bytes();
    let mut out = String::with_capacity(html.len());
    let mut cursor = 0;
    let mut changed = false;

    for (at, _) in html.match_indices("src") {
        if at < cursor {
            continue;
        }
        // Must be its own attribute: preceded by space, and not srcset or data-src.
        let before_ok = at > 0 && bytes[at - 1].is_ascii_whitespace();
        let after = bytes.get(at + 3).copied();
        if !before_ok || after.is_none_or(|b| b.is_ascii_alphanumeric() || b == b'-') {
            continue;
        }
        let mut i = at + 3;
        while bytes.get(i).is_some_and(u8::is_ascii_whitespace) {
            i += 1;
        }
        if bytes.get(i) != Some(&b'=') {
            continue;
        }
        i += 1;
        while bytes.get(i).is_some_and(u8::is_ascii_whitespace) {
            i += 1;
        }
        let (value, start, end) = match bytes.get(i) {
            Some(&quote @ (b'"' | b'\'')) => {
                let from = i + 1;
                let to = html[from..].find(quote as char)? + from;
                (&html[from..to], from, to)
            }
            Some(_) => {
                let from = i;
                let to = html[from..]
                    .find(|c: char| c.is_whitespace() || c == '>')
                    .map_or(html.len(), |n| n + from);
                (&html[from..to], from, to)
            }
            None => continue,
        };
        let Some(absolute) = absolute_file_uri(value, images) else {
            continue;
        };
        out.push_str(&html[cursor..start]);
        out.push_str(&absolute);
        cursor = end;
        changed = true;
    }
    if !changed {
        return None;
    }
    out.push_str(&html[cursor..]);
    Some(out)
}

/// `dir`-relative `url` as a `file://` URI, or `None` to leave it alone.
///
/// Anything already carrying a scheme is left as written, which keeps remote
/// images and `data:` URIs working.
fn absolute_file_uri(url: &str, images: &dyn ImageLocator) -> Option<String> {
    if url.is_empty() || url.starts_with('#') || has_scheme(url) {
        return None;
    }
    let joined = images.locate(url)?;
    let mut uri = String::from("file://");
    for byte in joined.to_string_lossy().bytes() {
        match byte {
            b'/' | b'-' | b'_' | b'.' | b'~' | b'0'..=b'9' | b'A'..=b'Z' | b'a'..=b'z' => {
                uri.push(byte as char);
            }
            other => {
                // Infallible: writing to a String never fails.
                let _ = write!(uri, "%{other:02X}");
            }
        }
    }
    Some(uri)
}

/// Whether `url` begins with a URI scheme such as `https:` or `data:`.
fn has_scheme(url: &str) -> bool {
    let scheme: String = url
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
        .collect();
    !scheme.is_empty() && url[scheme.len()..].starts_with(':')
}

/// Walk the AST in document order and collect one entry per heading. The slug
/// is computed with a single [`Anchorizer`] so its dedup suffixes match the ids
/// comrak emits in the HTML.
fn collect_headings<'a>(root: &'a AstNode<'a>) -> Vec<HeadingAnchor> {
    let mut anchorizer = Anchorizer::new();
    let mut headings = Vec::new();

    for node in root.descendants() {
        let data = node.data.borrow();
        if let NodeValue::Heading(heading) = &data.value {
            let source_line = data.sourcepos.start.line;
            let text = heading_plain_text(node);
            let anchor = anchorizer.anchorize(&text);
            headings.push(HeadingAnchor {
                source_line,
                anchor,
                level: heading.level,
            });
        }
    }

    headings
}

/// Concatenate the plain text of a heading: its Text and inline Code leaves.
fn heading_plain_text<'a>(node: &'a AstNode<'a>) -> String {
    let mut out = String::new();
    for descendant in node.descendants() {
        match &descendant.data.borrow().value {
            NodeValue::Text(text) => out.push_str(text),
            NodeValue::Code(code) => out.push_str(&code.literal),
            _ => {}
        }
    }
    out
}

/// The comrak highlighter for the preview, sharing the editor's syntax lookup.
///
/// The stock syntect adapter matches fence tokens only the way syntect spells
/// them, so `csharp` and `kotlin` fell back to plain text in the preview while
/// the editor colored them. One [`codeblock::lookup`] for both panes means a
/// block is either colored in both or flat in both.
struct CodeAdapter {
    /// The palette-derived theme, the same one [`codeblock::color_spans`] uses.
    theme: Theme,
}

impl comrak::adapters::SyntaxHighlighterAdapter for CodeAdapter {
    fn write_highlighted(
        &self,
        output: &mut dyn std::fmt::Write,
        lang: Option<&str>,
        code: &str,
    ) -> std::fmt::Result {
        let syntax = lang
            .filter(|lang| !lang.is_empty())
            .map(|lang| lang.split([' ', '\t', ',']).next().unwrap_or(lang))
            .and_then(|lang| codeblock::lookup(&lang.to_lowercase()));
        let Some(syntax) = syntax else {
            return write!(output, "{}", escape_html(code));
        };

        let mut lines = codeblock::BlockHighlighter::new(syntax, &self.theme, code);
        for line in code.split_inclusive('\n') {
            let Some(styled) = lines.highlight_line(line) else {
                write!(output, "{}", escape_html(line))?;
                continue;
            };
            let html = syntect::html::styled_line_to_highlighted_html(
                &styled,
                syntect::html::IncludeBackground::No,
            )
            .map_err(|_| std::fmt::Error)?;
            output.write_str(&html)?;
        }
        Ok(())
    }

    fn write_pre_tag(
        &self,
        output: &mut dyn std::fmt::Write,
        attributes: std::collections::HashMap<&'static str, std::borrow::Cow<'_, str>>,
    ) -> std::fmt::Result {
        let background = self.theme.settings.background.unwrap_or(FALLBACK_COLOR);
        write!(
            output,
            "<pre style=\"background-color:#{:02x}{:02x}{:02x};\"",
            background.r, background.g, background.b
        )?;
        write_attributes(output, &attributes)?;
        output.write_str(">")
    }

    fn write_code_tag(
        &self,
        output: &mut dyn std::fmt::Write,
        attributes: std::collections::HashMap<&'static str, std::borrow::Cow<'_, str>>,
    ) -> std::fmt::Result {
        output.write_str("<code")?;
        write_attributes(output, &attributes)?;
        output.write_str(">")
    }
}

/// Write comrak's tag attributes, values escaped.
fn write_attributes(
    output: &mut dyn std::fmt::Write,
    attributes: &std::collections::HashMap<&'static str, std::borrow::Cow<'_, str>>,
) -> std::fmt::Result {
    for (name, value) in attributes {
        write!(output, " {name}=\"{}\"", escape_html(value))?;
    }
    Ok(())
}

/// Escape text for HTML body or attribute position.
fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Build a syntect [`Theme`] from the jotter code palette, mapping scopes to
/// palette colors. Malformed hex falls back to a neutral default.
pub(crate) fn theme_from_code(code: &Code) -> Theme {
    let mut theme = Theme {
        name: Some("jotter".to_string()),
        ..Theme::default()
    };
    theme.settings.background = Some(parse_hex(&code.background));
    theme.settings.foreground = Some(parse_hex(&code.foreground));

    // A deeper selector outscores a shallower one, so `variable.function` takes
    // calls away from the plain `variable` rule, and `keyword.operator` quiets
    // `=` and `+` back to the foreground: an operator on every line is noise,
    // and the color reads better spent on calls and types.
    let mappings = [
        ("keyword", code.keyword.as_str()),
        ("keyword.operator", code.foreground.as_str()),
        ("storage.modifier", code.keyword.as_str()),
        ("string", code.string.as_str()),
        ("constant.character.escape", code.number.as_str()),
        ("comment", code.comment.as_str()),
        (
            "entity.name.function, variable.function, support.function",
            code.function.as_str(),
        ),
        ("constant.numeric, constant.language", code.number.as_str()),
        (
            "entity.name.type, entity.name.class, storage.type, support.type, support.class",
            code.type_name.as_str(),
        ),
        (
            "variable.annotation, entity.other.attribute-name, entity.other.inherited-class",
            code.type_name.as_str(),
        ),
        ("variable", code.variable.as_str()),
    ];

    for (selector, hex) in mappings {
        if let Some(item) = theme_item(selector, hex) {
            theme.scopes.push(item);
        }
    }

    theme
}

/// Build a foreground-only [`ThemeItem`] for a scope selector, or `None` if the
/// selector fails to parse (it never should for our fixed literals).
fn theme_item(selector: &str, hex: &str) -> Option<ThemeItem> {
    let scope: ScopeSelectors = selector.parse().ok()?;
    Some(ThemeItem {
        scope,
        style: StyleModifier {
            foreground: Some(parse_hex(hex)),
            background: None,
            font_style: None,
        },
    })
}

/// A neutral fallback used when a color string cannot be parsed.
const FALLBACK_COLOR: Color = Color {
    r: 0x80,
    g: 0x80,
    b: 0x80,
    a: 0xff,
};

/// Parse a `#RRGGBB` string to a syntect [`Color`]. Any malformed input returns
/// [`FALLBACK_COLOR`] rather than panicking.
fn parse_hex(hex: &str) -> Color {
    let digits = hex.strip_prefix('#').unwrap_or(hex);
    if digits.len() != 6 {
        return FALLBACK_COLOR;
    }
    let component = |range: std::ops::Range<usize>| u8::from_str_radix(&digits[range], 16).ok();
    match (component(0..2), component(2..4), component(4..6)) {
        (Some(r), Some(g), Some(b)) => Color { r, g, b, a: 0xff },
        _ => FALLBACK_COLOR,
    }
}

#[cfg(test)]
mod image_paths {
    use super::absolute_file_uri;
    use std::path::Path;

    fn at(url: &str) -> Option<String> {
        absolute_file_uri(
            url,
            &super::RelativeTo(Path::new("/vault/notes").to_path_buf()),
        )
    }

    #[test]
    fn a_same_folder_image_becomes_absolute() {
        assert_eq!(
            at("beside.jpg").as_deref(),
            Some("file:///vault/notes/beside.jpg")
        );
    }

    #[test]
    fn a_subfolder_image_keeps_its_subfolder() {
        assert_eq!(
            at("pics/otter.jpg").as_deref(),
            Some("file:///vault/notes/pics/otter.jpg")
        );
    }

    #[test]
    fn a_parent_relative_image_resolves_upward() {
        assert_eq!(
            at("../top.png").as_deref(),
            Some("file:///vault/notes/../top.png")
        );
    }

    #[test]
    fn an_absolute_path_replaces_the_base() {
        assert_eq!(
            at("/srv/shared/x.png").as_deref(),
            Some("file:///srv/shared/x.png")
        );
    }

    #[test]
    fn remote_and_data_urls_are_left_alone() {
        for url in [
            "https://example.com/a.png",
            "http://x/a.png",
            "data:image/png;base64,AAA",
        ] {
            assert_eq!(at(url), None, "{url} should be untouched");
        }
    }

    #[test]
    fn spaces_and_specials_are_percent_encoded() {
        assert_eq!(
            at("my pic.jpg").as_deref(),
            Some("file:///vault/notes/my%20pic.jpg")
        );
        assert_eq!(
            at("caf\u{e9}.png").as_deref(),
            Some("file:///vault/notes/caf%C3%A9.png")
        );
    }

    #[test]
    fn an_anchor_or_empty_url_is_left_alone() {
        assert_eq!(at("#section"), None);
        assert_eq!(at(""), None);
    }

    #[test]
    fn a_windows_style_drive_is_treated_as_a_scheme_not_a_path() {
        // Guard on has_scheme: a single letter followed by a colon reads as a
        // scheme, so it is left alone rather than mangled into a file URI.
        assert_eq!(at("c:/pics/x.png"), None);
    }
}

#[cfg(test)]
mod html_image_paths {
    use super::rewrite_html_srcs;
    use std::path::Path;

    fn at(html: &str) -> Option<String> {
        rewrite_html_srcs(
            html,
            &super::RelativeTo(Path::new("/vault/CV").to_path_buf()),
        )
    }

    #[test]
    fn a_hand_written_img_gets_an_absolute_src() {
        // The shape a CV actually uses: inside a table, self-closing, with attributes.
        let html = "<td><img src=\"profile.jpg\" width=\"100\" /></td>";
        assert_eq!(
            at(html).as_deref(),
            Some("<td><img src=\"file:///vault/CV/profile.jpg\" width=\"100\" /></td>")
        );
    }

    #[test]
    fn single_quoted_and_unquoted_srcs_work() {
        assert!(
            at("<img src='a.png'>")
                .unwrap()
                .contains("file:///vault/CV/a.png")
        );
        assert!(
            at("<img src=a.png>")
                .unwrap()
                .contains("file:///vault/CV/a.png")
        );
    }

    #[test]
    fn several_images_in_one_block_are_all_rewritten() {
        let out = at("<img src=\"github.png\"><img src=\"linkedin.png\">").unwrap();
        assert!(out.contains("file:///vault/CV/github.png"));
        assert!(out.contains("file:///vault/CV/linkedin.png"));
    }

    #[test]
    fn remote_srcs_and_unrelated_attributes_are_left_alone() {
        assert_eq!(at("<img src=\"https://x/a.png\">"), None);
        // srcset and data-src must not be mistaken for src.
        assert_eq!(at("<img srcset=\"a.png 1x\">"), None);
        assert_eq!(at("<img data-src=\"a.png\">"), None);
    }

    #[test]
    fn html_with_no_src_is_untouched() {
        assert_eq!(at("<div style=\"color: black\">hi</div>"), None);
    }

    #[test]
    fn other_attributes_survive_around_the_rewrite() {
        let out = at("<img alt=\"me\" src=\"a.png\" style=\"width:1px\">").unwrap();
        assert!(out.contains("alt=\"me\""), "{out}");
        assert!(out.contains("style=\"width:1px\""), "{out}");
    }
}
