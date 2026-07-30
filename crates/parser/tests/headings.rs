//! Integration test for the heading anchor map produced by `render`.

use jotter_parser::render;

/// Resolves nothing: these fixtures carry no wikilinks.
fn no_links(_: &str) -> Option<String> {
    None
}
use jotter_theming::Code;

const FIXTURE: &str = include_str!("fixtures/sample.md");

/// A minimal code palette for exercising the render path.
fn test_code() -> Code {
    Code {
        background: "#1e1e1e".into(),
        foreground: "#d4d4d4".into(),
        keyword: "#c586c0".into(),
        string: "#ce9178".into(),
        comment: "#6a9955".into(),
        function: "#dcdcaa".into(),
        number: "#b5cea8".into(),
        type_name: "#4ec9b0".into(),
        variable: "#9cdcfe".into(),
    }
}

#[test]
fn headings_map_source_lines_levels_and_anchors() {
    let rendered = render(FIXTURE, &test_code(), &no_links);

    let headings = &rendered.headings;
    assert_eq!(headings.len(), 3, "three headings expected");

    // Frontmatter occupies lines 1..=4; the H1 is on the original line 6.
    assert_eq!(headings[0].source_line, 6);
    assert_eq!(headings[0].level, 1);
    assert_eq!(headings[0].anchor, "sample-note");

    // Two "Details" headings exercise the dedup suffix.
    assert_eq!(headings[1].level, 2);
    assert_eq!(headings[1].anchor, "details");
    assert_eq!(headings[2].level, 2);
    assert_eq!(headings[2].anchor, "details-1");

    // Source lines must be strictly increasing and 1-based.
    for window in headings.windows(2) {
        assert!(window[0].source_line < window[1].source_line);
    }

    // Every anchor must appear as an id attribute in the HTML.
    for heading in headings {
        assert!(!heading.anchor.is_empty(), "anchor must be non-empty");
        let needle = format!("id=\"{}\"", heading.anchor);
        assert!(
            rendered.html.contains(&needle),
            "expected {needle} in rendered html"
        );
    }
}

#[test]
fn render_highlights_fenced_code_with_theme_background() {
    let rendered = render(FIXTURE, &test_code(), &no_links);
    // The custom syntect theme background should appear on the code block.
    assert!(
        rendered.html.contains("background-color:#1e1e1e"),
        "expected themed code background in html"
    );
}

#[test]
fn the_preview_colors_csharp_and_kotlin_blocks() {
    // The stock syntect adapter knew neither token, so both fell back to plain
    // text in the preview while the editor colored them.
    let src = "```csharp\n// note\nvar x = 42;\n```\n\n```kotlin\n// note\nval y = 7\n```\n";
    let rendered = render(src, &test_code(), &no_links);
    let comments = rendered.html.matches("color:#6a9955").count();
    assert_eq!(comments, 2, "one colored comment per block:\n{}", rendered.html);
    assert!(rendered.html.contains("color:#b5cea8"), "numbers colored");
}

#[test]
fn an_unknown_fence_language_renders_escaped_plain_text() {
    let src = "```mermaid\ngraph TD; A-->B & \"q\"\n```\n";
    let rendered = render(src, &test_code(), &no_links);
    assert!(rendered.html.contains("A--&gt;B &amp; &quot;q&quot;"));
    assert!(!rendered.html.contains("<span style"));
}
