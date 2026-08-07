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

use std::cell::RefCell;
use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;
use std::sync::OnceLock;

use syntect::highlighting::{HighlightIterator, HighlightState, Highlighter, Style, Theme};
use syntect::parsing::{
    ParseState, ScopeStack, ScopeStackOp, SyntaxReference, SyntaxSet,
    syntax_definition::SyntaxDefinition,
};

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

/// The scope changes syntect found on one line, or `None` if it failed to parse.
type LineOps = Option<Vec<(usize, ScopeStackOp)>>;

/// A block is the syntax it is read with plus the exact text it holds.
type BlockKey = (usize, String);

/// One block's scopes, how many there are, and when they were last wanted.
struct Entry {
    used: u64,
    weight: usize,
    ops: Rc<Vec<LineOps>>,
}

/// Most of the cost of highlighting is finding the scopes, and those depend only
/// on the text; only the last step, scope to color, depends on the palette. So
/// the scopes are kept and a theme switch pays for the coloring alone. The
/// editor and the preview highlight the same blocks and share these entries.
struct Tokenized {
    blocks: HashMap<BlockKey, Entry>,
    weight: usize,
    clock: u64,
}

/// Scopes held before the least recently used blocks are dropped.
#[cfg(not(test))]
const CACHE_OPS: usize = 250_000;
/// Small enough for a test to fill it in a moment.
#[cfg(test)]
const CACHE_OPS: usize = 2_000;

thread_local! {
    static TOKENIZED: RefCell<Tokenized> =
        RefCell::new(Tokenized { blocks: HashMap::new(), weight: 0, clock: 0 });
}

/// Position in the shared set, which identifies a syntax where its name may not.
fn syntax_id(syntax: &SyntaxReference) -> Option<usize> {
    syntaxes()
        .syntaxes()
        .iter()
        .position(|known| std::ptr::eq(known, syntax))
}

/// Run syntect's parser over `code`, one entry per line.
fn parse(syntax: &SyntaxReference, code: &str) -> Vec<LineOps> {
    count_tokenization();
    let mut state = ParseState::new(syntax);
    code.split_inclusive('\n')
        .map(|line| state.parse_line(line, syntaxes()).ok())
        .collect()
}

/// The scopes of `code` under `syntax`, parsed once and kept for later palettes.
fn tokenize(syntax: &SyntaxReference, code: &str) -> Rc<Vec<LineOps>> {
    let Some(id) = syntax_id(syntax) else {
        return Rc::new(parse(syntax, code));
    };
    let key = (id, code.to_string());

    TOKENIZED.with_borrow_mut(|cache| {
        cache.clock += 1;
        let used = cache.clock;
        if let Some(entry) = cache.blocks.get_mut(&key) {
            entry.used = used;
            return Rc::clone(&entry.ops);
        }
        let ops = Rc::new(parse(syntax, code));
        let weight = ops.iter().flatten().map(Vec::len).sum::<usize>();
        cache.blocks.insert(
            key,
            Entry {
                used,
                weight,
                ops: Rc::clone(&ops),
            },
        );
        cache.weight += weight;
        while cache.weight > CACHE_OPS {
            let Some(oldest) = cache
                .blocks
                .iter()
                .min_by_key(|(_, entry)| entry.used)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            if let Some(dropped) = cache.blocks.remove(&oldest) {
                cache.weight -= dropped.weight;
            }
        }
        ops
    })
}

/// One code block's scopes, ready to be colored by whichever palette asks.
///
/// Stands in for `syntect::easy::HighlightLines`, which fuses parsing and
/// coloring and so cannot reuse the parse across palettes.
pub(crate) struct BlockHighlighter<'a> {
    highlighter: Highlighter<'a>,
    state: HighlightState,
    ops: Rc<Vec<LineOps>>,
    line: usize,
}

impl<'a> BlockHighlighter<'a> {
    pub(crate) fn new(syntax: &SyntaxReference, theme: &'a Theme, code: &str) -> Self {
        let highlighter = Highlighter::new(theme);
        let state = HighlightState::new(&highlighter, ScopeStack::new());
        Self {
            highlighter,
            state,
            ops: tokenize(syntax, code),
            line: 0,
        }
    }

    /// The colored runs of the next line, or `None` if it did not parse.
    ///
    /// Lines must arrive in the order they were given to [`Self::new`].
    pub(crate) fn highlight_line<'t>(&mut self, text: &'t str) -> Option<Vec<(Style, &'t str)>> {
        let line = self.line;
        self.line += 1;
        let ops = self.ops.get(line)?.as_ref()?;
        Some(HighlightIterator::new(&mut self.state, ops, text, &self.highlighter).collect())
    }
}

#[cfg(test)]
thread_local! {
    static TOKENIZATIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
fn count_tokenization() {
    TOKENIZATIONS.with(|count| count.set(count.get() + 1));
}

#[cfg(not(test))]
fn count_tokenization() {}

/// How many blocks this thread has parsed, for the tests that guard the cache.
#[cfg(test)]
pub(crate) fn tokenizations() -> usize {
    TOKENIZATIONS.with(std::cell::Cell::get)
}

#[cfg(test)]
fn cached_ops() -> usize {
    TOKENIZED.with_borrow(|cache| cache.weight)
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
        let content = &src[block.content.clone()];
        let mut lines = BlockHighlighter::new(syntax, &theme, content);
        let mut cursor = block.content.start;
        // Line by line, as syntect wants, with offsets kept against the source.
        for line in content.split_inclusive('\n') {
            let Some(styled) = lines.highlight_line(line) else {
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
                if let Some(ColorSpan {
                    range: last,
                    color: last_color,
                }) = spans.last_mut()
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
        blocks.push(Fenced {
            language,
            content: content_start..src.len(),
        });
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
    use super::{ColorSpan, color_spans, fenced_blocks, tokenizations};
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
        assert!(
            colored(src, &spans, "#888888")
                .concat()
                .contains("// typing away")
        );
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
        assert!(
            colored(src, &spans, "#888888")
                .concat()
                .contains("// tilde")
        );
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

    /// A second palette differs in every color, so a stale reuse shows up.
    fn other_palette() -> Code {
        Code {
            background: "#ffffff".into(),
            foreground: "#111111".into(),
            keyword: "#aa0000".into(),
            string: "#00aa00".into(),
            comment: "#333333".into(),
            function: "#0000aa".into(),
            number: "#aaaa00".into(),
            type_name: "#aa00aa".into(),
            variable: "#111111".into(),
        }
    }

    /// Same text, same colors, but resolved from the palette it was given.
    fn recolored(src: &str) -> (Vec<ColorSpan>, Vec<ColorSpan>) {
        (
            color_spans(src, &palette()),
            color_spans(src, &other_palette()),
        )
    }

    #[test]
    fn a_second_palette_recolors_rather_than_reusing_the_first() {
        let src = "```rust\n// note\nlet x = 42;\n```\n";
        let (first, second) = recolored(src);
        assert!(
            second.iter().any(|span| span.color == "#333333"),
            "{second:?}"
        );
        assert!(
            second.iter().all(|span| span.color != "#888888"),
            "{second:?}"
        );
        let ranges: Vec<_> = first.iter().map(|span| span.range.clone()).collect();
        let same: Vec<_> = second.iter().map(|span| span.range.clone()).collect();
        assert_eq!(ranges, same);
        assert_eq!(color_spans(src, &palette()), first);
    }

    #[test]
    fn a_block_is_tokenized_once_however_many_palettes_color_it() {
        let src = "```rust\n// note\nlet x = 42;\n```\n";
        drop(color_spans(src, &palette()));
        let after_first = tokenizations();
        drop(color_spans(src, &other_palette()));
        drop(color_spans(src, &palette()));
        assert_eq!(tokenizations(), after_first);
    }

    #[test]
    fn the_editor_and_the_preview_share_one_tokenization() {
        let src = "```rust\n// note\nlet x = 42;\n```\n";
        drop(color_spans(src, &palette()));
        let after_editor = tokenizations();
        let no_links = |_: &str| None;
        drop(crate::render(
            src,
            &other_palette(),
            &no_links,
            &crate::NoImages,
        ));
        assert_eq!(tokenizations(), after_editor);
    }

    #[test]
    fn the_same_text_in_two_languages_is_tokenized_apart() {
        let sql = color_spans("```sql\nselect 1\n```\n", &palette());
        let python = color_spans("```python\nselect 1\n```\n", &palette());
        assert_ne!(sql, python);
    }

    #[test]
    fn the_cache_drops_the_oldest_block_rather_than_growing() {
        let block =
            |n: usize| format!("```rust\nlet value_{n} = first(1) + second(2) * third(3);\n```\n");
        for n in 0..200 {
            drop(color_spans(&block(n), &palette()));
            assert!(
                super::cached_ops() <= super::CACHE_OPS,
                "{} ops held",
                super::cached_ops()
            );
        }

        let before = tokenizations();
        drop(color_spans(&block(199), &palette()));
        assert_eq!(tokenizations(), before, "the newest block was dropped");
        drop(color_spans(&block(0), &palette()));
        assert_eq!(tokenizations(), before + 1, "the oldest block was kept");
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
            "kotlin", "kt", "csharp", "c#", "cs", "bash", "sh", "shell", "json", "xml", "python",
            "sql", "yaml", "js", "rust", "gradle",
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
