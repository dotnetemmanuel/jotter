//! Syntax colors for fenced code blocks, as byte spans over the raw source.
//!
//! The preview gets its code colors from syntect via comrak; the editor cannot,
//! because the `GtkSourceView` markdown grammar paints a whole fenced block with
//! one style and knows nothing of the language inside. This module runs the same
//! syntect theme over the same blocks and reports where each color goes, so the
//! editor can lay tags over its buffer and the two panes agree.
//!
//! Fences are found with a line scanner rather than the markdown AST so an
//! unclosed fence, which is what a block looks like the whole time it is being
//! typed, still gets its colors.

use std::ops::Range;
use std::sync::OnceLock;

use syntect::easy::HighlightLines;
use syntect::parsing::{SyntaxSet, syntax_definition::SyntaxDefinition};

use jotter_theming::Code;

/// One run of same-colored text inside a fenced code block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColorSpan {
    /// Byte range in the source the color applies to.
    pub range: Range<usize>,
    /// The color, as `#rrggbb`.
    pub color: String,
}

/// The syntax definitions, loaded once: the set is large and never changes.
///
/// The syntect defaults plus what jotter vaults actually hold that they lack;
/// today that is Kotlin, vendored under `resources/syntaxes/`.
pub(crate) fn syntaxes() -> &'static SyntaxSet {
    static SET: OnceLock<SyntaxSet> = OnceLock::new();
    SET.get_or_init(|| {
        let mut builder = SyntaxSet::load_defaults_newlines().into_builder();
        let kotlin = include_str!("../../../resources/syntaxes/Kotlin.sublime-syntax");
        match SyntaxDefinition::load_from_str(kotlin, true, None) {
            Ok(syntax) => builder.add(syntax),
            Err(err) => eprintln!("jotter: bundled Kotlin syntax failed to load: {err}"),
        }
        builder.build()
    })
}

/// The syntax a fence token names, through the spellings people actually write.
///
/// syntect matches file extensions and exact names, so `cs` works and `csharp`
/// does not; the aliases bridge the difference. `None` means the block stays one
/// color, which is the honest answer for a language nobody can highlight.
pub(crate) fn lookup(token: &str) -> Option<&'static syntect::parsing::SyntaxReference> {
    let token = match token {
        "csharp" | "c#" => "cs",
        "shell" => "sh",
        other => other,
    };
    syntaxes().find_syntax_by_token(token)
}

/// Color spans for every fenced code block in `src`, in document order.
///
/// Text in the palette's default foreground is not reported: the editor already
/// paints it, and skipping it keeps the tag count down. A fence with no language,
/// or one syntect does not know, yields nothing and stays one color.
#[must_use]
pub fn color_spans(src: &str, code: &Code) -> Vec<ColorSpan> {
    let theme = crate::theme_from_code(code);
    let foreground = code.foreground.trim().to_lowercase();
    let mut spans = Vec::new();

    for block in fenced_blocks(src) {
        let Some(syntax) = lookup(&block.language) else {
            continue;
        };
        let mut lines = HighlightLines::new(syntax, &theme);
        let mut cursor = block.content.start;
        // Line by line, as syntect wants, with offsets kept against the source.
        for line in src[block.content.clone()].split_inclusive('\n') {
            let Ok(styled) = lines.highlight_line(line, syntaxes()) else {
                cursor += line.len();
                continue;
            };
            for (style, text) in styled {
                let range = cursor..cursor + text.len();
                cursor = range.end;
                let color = hex_of(style.foreground);
                if color == foreground || text.trim().is_empty() {
                    continue;
                }
                // Adjacent same-color runs collapse into one span.
                if let Some(ColorSpan { range: last, color: last_color }) = spans.last_mut()
                    && last.end == range.start
                    && *last_color == color
                {
                    last.end = range.end;
                    continue;
                }
                spans.push(ColorSpan { range, color });
            }
        }
    }
    spans
}

/// A fenced code block: the language named on the fence, and its content bytes.
struct Fenced {
    /// First token of the fence info string, lowercased.
    language: String,
    /// The content between the fences, excluding both fence lines.
    content: Range<usize>,
}

/// Every fenced block in `src` that names a language, including an unclosed one.
///
/// A fence opens with three or more backticks or tildes (up to three spaces of
/// indent) and closes with a line of at least as many of the same marker. An
/// open fence with no close runs to the end of the source.
fn fenced_blocks(src: &str) -> Vec<Fenced> {
    let mut blocks = Vec::new();
    let mut open: Option<(char, usize, String, usize)> = None;

    let mut offset = 0;
    for line in src.split_inclusive('\n') {
        let start = offset;
        offset += line.len();
        let trimmed = line.trim_start_matches(' ');
        // More than three spaces of indent is an indented code block, not a fence.
        if line.len() - trimmed.len() > 3 {
            continue;
        }

        match &open {
            None => {
                if let Some((marker, count, info)) = fence_open(trimmed) {
                    open = Some((marker, count, info, offset));
                }
            }
            Some((marker, count, language, content_start)) => {
                if fence_close(trimmed, *marker, *count) {
                    blocks.push(Fenced {
                        language: language.clone(),
                        content: *content_start..start,
                    });
                    open = None;
                }
            }
        }
    }
    if let Some((_, _, language, content_start)) = open {
        blocks.push(Fenced { language, content: content_start..src.len() });
    }
    blocks.retain(|block| !block.language.is_empty());
    blocks
}

/// The marker, its run length, and the language token of an opening fence line.
fn fence_open(line: &str) -> Option<(char, usize, String)> {
    let marker @ ('`' | '~') = line.chars().next()? else {
        return None;
    };
    let count = line.chars().take_while(|&c| c == marker).count();
    if count < 3 {
        return None;
    }
    let info = line[count..].trim();
    // A backtick fence whose info contains a backtick is not a fence (CommonMark).
    if marker == '`' && info.contains('`') {
        return None;
    }
    let language = info
        .split([' ', '\t', ','])
        .next()
        .unwrap_or_default()
        .to_lowercase();
    Some((marker, count, language))
}

/// True if `line` closes a fence opened with `count` of `marker`.
fn fence_close(line: &str, marker: char, count: usize) -> bool {
    let run = line.chars().take_while(|&c| c == marker).count();
    run >= count && line[run..].trim().is_empty()
}

/// Format a syntect color as the `#rrggbb` the theme palette speaks.
fn hex_of(color: syntect::highlighting::Color) -> String {
    format!("#{:02x}{:02x}{:02x}", color.r, color.g, color.b)
}

#[cfg(test)]
pub(super) mod tests {
    use super::{ColorSpan, color_spans, fenced_blocks};
    use jotter_theming::Code;

    pub(super) fn palette() -> Code {
        Code {
            background: "#101010".into(),
            foreground: "#cccccc".into(),
            keyword: "#ff0000".into(),
            string: "#00ff00".into(),
            comment: "#888888".into(),
            function: "#0000ff".into(),
            number: "#ffff00".into(),
            type_name: "#ff00ff".into(),
            variable: "#cccccc".into(),
        }
    }

    /// The colors landing on `snippet`, resolved back to source text.
    fn colored<'a>(src: &'a str, spans: &[ColorSpan], color: &str) -> Vec<&'a str> {
        spans
            .iter()
            .filter(|span| span.color == color)
            .map(|span| &src[span.range.clone()])
            .collect()
    }

    #[test]
    fn a_rust_block_gets_keyword_comment_and_number_colors() {
        let src = "# Head\n\n```rust\n// note\nlet x = 42;\n```\n";
        let spans = color_spans(src, &palette());
        assert!(colored(src, &spans, "#888888").concat().contains("// note"));
        assert!(colored(src, &spans, "#ff00ff").concat().contains("let"));
        assert_eq!(colored(src, &spans, "#ffff00"), ["42"]);
    }

    #[test]
    fn spans_stay_inside_the_fence_content() {
        let src = "before\n```python\nx = 1\n```\nafter [[link]]\n";
        let content = src.find("x = 1").unwrap()..src.find("\n```\naf").unwrap() + 1;
        for span in color_spans(src, &palette()) {
            assert!(span.range.start >= content.start && span.range.end <= content.end);
        }
    }

    #[test]
    fn default_foreground_text_is_not_reported() {
        let src = "```rust\nlet x = 42;\n```\n";
        let spans = color_spans(src, &palette());
        assert!(spans.iter().all(|span| span.color != "#cccccc"));
    }

    #[test]
    fn a_fence_with_no_language_yields_nothing() {
        assert!(color_spans("```\nplain text\n```\n", &palette()).is_empty());
    }

    #[test]
    fn an_unknown_language_yields_nothing() {
        assert!(color_spans("```nosuchlang\nwords\n```\n", &palette()).is_empty());
    }

    #[test]
    fn an_unclosed_fence_highlights_to_the_end() {
        let src = "```rust\n// typing away";
        let spans = color_spans(src, &palette());
        assert!(colored(src, &spans, "#888888").concat().contains("// typing away"));
    }

    #[test]
    fn two_blocks_are_both_colored() {
        let src = "```rust\n// one\n```\n\n```python\n# two\n```\n";
        let spans = color_spans(src, &palette());
        let comments = colored(src, &spans, "#888888").concat();
        assert!(comments.contains("// one") && comments.contains("# two"));
    }

    #[test]
    fn adjacent_same_color_runs_merge() {
        let src = "```rust\nlet mut x = 1;\n```\n";
        let spans = color_spans(src, &palette());
        for pair in spans.windows(2) {
            assert!(
                !(pair[0].color == pair[1].color && pair[0].range.end == pair[1].range.start),
                "unmerged neighbors: {pair:?}"
            );
        }
    }

    #[test]
    fn tilde_fences_and_info_extras_work() {
        let src = "~~~rust,ignore\n// tilde\n~~~\n";
        let spans = color_spans(src, &palette());
        assert!(colored(src, &spans, "#888888").concat().contains("// tilde"));
    }

    #[test]
    fn a_backtick_run_inside_a_line_is_not_a_fence() {
        assert!(fenced_blocks("some ```rust inline\n").is_empty());
        assert!(fenced_blocks("    ```rust\nindented too far\n").is_empty());
    }

    #[test]
    fn the_closing_fence_must_match_the_marker() {
        let blocks = fenced_blocks("```rust\ncode\n~~~\nmore\n```\n");
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].language, "rust");
    }
}


#[cfg(test)]
mod vault_languages {
    //! The languages real vaults lean on, which the syntect defaults spell
    //! differently (`cs`, not `csharp`) or lack outright (Kotlin, vendored).

    use super::{color_spans, lookup};

    #[test]
    fn the_common_fence_tokens_resolve() {
        for token in [
            "kotlin", "kt", "csharp", "c#", "cs", "bash", "sh", "shell", "json", "xml",
            "python", "sql", "yaml", "js", "rust", "gradle",
        ] {
            assert!(lookup(token).is_some(), "{token} does not resolve");
        }
    }

    #[test]
    fn unknown_stays_unknown() {
        assert!(lookup("mermaid").is_none());
        assert!(lookup("nosuchlang").is_none());
    }

    #[test]
    fn a_kotlin_block_gets_colors() {
        let src = "```kotlin\n// note\nfun greet(name: String) = \"hi $name\"\n```\n";
        let spans = color_spans(src, &super::tests::palette());
        assert!(!spans.is_empty());
        let comment = spans
            .iter()
            .find(|span| &src[span.range.clone()] == "// note")
            .expect("comment span");
        assert_eq!(comment.color, "#888888");
    }

    #[test]
    fn a_csharp_block_gets_colors() {
        let src = "```csharp\n// note\nvar x = 42;\n```\n";
        let spans = color_spans(src, &super::tests::palette());
        assert!(spans.iter().any(|span| span.color == "#888888"));
        assert!(spans.iter().any(|span| span.color == "#ffff00"));
    }
    #[test]
    fn method_calls_take_the_function_color() {
        let src = "```csharp\nvar t = await NewTeacherAsync();\nAssert.Equal(a, b);\n```\n";
        let spans = color_spans(src, &super::tests::palette());
        let calls: Vec<&str> = spans
            .iter()
            .filter(|span| span.color == "#0000ff")
            .map(|span| &src[span.range.clone()])
            .collect();
        assert_eq!(calls, ["NewTeacherAsync", "Equal"]);
    }

    #[test]
    fn operators_stay_quiet_and_types_do_not() {
        let src = "```csharp\npublic async Task Go() { var x = 1 + 2; }\n```\n";
        let spans = color_spans(src, &super::tests::palette());
        let texts = |color: &str| -> Vec<&str> {
            spans
                .iter()
                .filter(|span| span.color == color)
                .map(|span| &src[span.range.clone()])
                .collect()
        };
        // Task is support.type; public and async are storage modifiers.
        assert!(texts("#ff00ff").contains(&"Task"));
        assert!(texts("#ff0000").concat().contains("public"));
        // The operators carry no color at all: nothing reports = or +.
        assert!(spans.iter().all(|span| {
            let text = &src[span.range.clone()];
            text != "=" && text != "+"
        }));
    }
}

