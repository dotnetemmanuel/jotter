#![warn(clippy::pedantic)]
//! Markdown rendering for jotter: comrak with GFM extensions, syntect code
//! highlighting driven by the active theme, and a source-line to heading-anchor
//! map for scroll synchronization between the editor and the preview.

pub mod frontmatter;
pub mod tags;
pub mod wikilink;

use comrak::nodes::{AstNode, NodeValue};
use comrak::{Anchorizer, Arena, Options};
use jotter_theming::Code;
use syntect::highlighting::{Color, ScopeSelectors, StyleModifier, Theme, ThemeItem, ThemeSet};

pub use wikilink::Wikilink;

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
pub fn render(src: &str, code: &Code, resolver: &dyn wikilink::LinkResolver) -> Rendered {
    let src = wikilink::rewrite(src, resolver);
    let options = base_options();
    let arena = Arena::new();
    let root = comrak::parse_document(&arena, &src, &options);

    let headings = collect_headings(root);

    let adapter = comrak::plugins::syntect::SyntectAdapterBuilder::new()
        .theme_set(theme_set_from_code(code))
        .theme(SYNTECT_THEME_NAME)
        .build();
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

/// The name our custom syntect theme is registered under in its theme set.
const SYNTECT_THEME_NAME: &str = "jotter";

/// Build a one-theme syntect [`ThemeSet`] from the jotter code palette.
fn theme_set_from_code(code: &Code) -> ThemeSet {
    let mut set = ThemeSet::default();
    set.themes
        .insert(SYNTECT_THEME_NAME.to_string(), theme_from_code(code));
    set
}

/// Build a syntect [`Theme`] from the jotter code palette, mapping scopes to
/// palette colors. Malformed hex falls back to a neutral default.
fn theme_from_code(code: &Code) -> Theme {
    let mut theme = Theme {
        name: Some(SYNTECT_THEME_NAME.to_string()),
        ..Theme::default()
    };
    theme.settings.background = Some(parse_hex(&code.background));
    theme.settings.foreground = Some(parse_hex(&code.foreground));

    let mappings = [
        ("keyword", code.keyword.as_str()),
        ("string", code.string.as_str()),
        ("comment", code.comment.as_str()),
        ("entity.name.function", code.function.as_str()),
        ("constant.numeric", code.number.as_str()),
        ("entity.name.type, storage.type", code.type_name.as_str()),
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
