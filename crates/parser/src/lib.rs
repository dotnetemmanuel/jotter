#![warn(clippy::pedantic)]
//! Markdown rendering for jotter: comrak with GFM extensions, syntect code
//! highlighting driven by the active theme, and a source-line to heading-anchor
//! map for scroll synchronization between the editor and the preview.

pub mod codeblock;
pub mod frontmatter;
pub mod tags;
pub mod wikilink;

use comrak::nodes::{AstNode, NodeValue};
use comrak::{Anchorizer, Arena, Options};
use jotter_theming::Code;
use syntect::highlighting::{Color, ScopeSelectors, StyleModifier, Theme, ThemeItem};

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

    let adapter = CodeAdapter { theme: theme_from_code(code) };
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

        let mut lines = syntect::easy::HighlightLines::new(syntax, &self.theme);
        for line in code.split_inclusive('\n') {
            let Ok(styled) = lines.highlight_line(line, codeblock::syntaxes()) else {
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
    let mut theme = Theme { name: Some("jotter".to_string()), ..Theme::default() };
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
        ("entity.name.function, variable.function, support.function", code.function.as_str()),
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
