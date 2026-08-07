//! End-to-end wikilink rendering: source in, anchor tags out.

use jotter_parser::render;
use jotter_theming::Code;

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

/// Resolves "standup" only, so one link renders resolved and one broken.
fn stub(target: &str) -> Option<String> {
    (target == "standup").then(|| "work/standup.md".to_owned())
}

#[test]
fn resolved_and_broken_links_survive_to_html() {
    let html = render(
        "See [[standup]] and [[missing]].\n",
        &test_code(),
        &stub,
        &jotter_parser::NoImages,
    )
    .html;
    assert!(
        html.contains(r#"href="jotter-note:work/standup.md""#),
        "custom scheme must not be filtered out: {html}"
    );
    assert!(
        html.contains(r#"href="jotter-new:missing""#),
        "broken link must keep the new-note scheme: {html}"
    );
}

#[test]
fn links_in_code_are_left_as_text() {
    let html = render(
        "```\n[[standup]]\n```\n",
        &test_code(),
        &stub,
        &jotter_parser::NoImages,
    )
    .html;
    assert!(!html.contains("jotter-note:"), "code must not be linkified");
    assert!(
        html.contains("[[standup]]"),
        "code must keep its text: {html}"
    );
}

#[test]
fn heading_lines_are_unaffected_by_rewriting() {
    let src = "# Title\n\n[[standup]]\n\n## Later\n";
    let rendered = render(src, &test_code(), &stub, &jotter_parser::NoImages);
    assert_eq!(rendered.headings[0].source_line, 1);
    assert_eq!(rendered.headings[1].source_line, 5);
}

#[test]
fn dead_ranges_cover_a_fenced_code_block() {
    let src = "text\n\n```\n[[plan\n```\n";
    let dead = jotter_parser::wikilink::dead_ranges(src);
    let open = src.find("[[").expect("bracket");
    assert!(dead.iter().any(|range| range.contains(&open)));
}

#[test]
fn dead_ranges_cover_frontmatter() {
    let src = "---\ntitle: Old notes\n[[plan\n---\n\nbody\n";
    let dead = jotter_parser::wikilink::dead_ranges(src);
    let open = src.find("[[").expect("bracket");
    assert!(dead.iter().any(|range| range.contains(&open)));
}

#[test]
fn dead_ranges_cover_an_inline_code_span() {
    let src = "see `[[plan` here";
    let dead = jotter_parser::wikilink::dead_ranges(src);
    let open = src.find("[[").expect("bracket");
    assert!(dead.iter().any(|range| range.contains(&open)));
}

#[test]
fn dead_ranges_cover_an_unclosed_inline_run_to_end_of_line() {
    let src = "see `[[pl\nnext line";
    let dead = jotter_parser::wikilink::dead_ranges(src);
    let open = src.find("[[").expect("bracket");
    let next = src.find("next").expect("second line");
    assert!(dead.iter().any(|range| range.contains(&open)));
    assert!(!dead.iter().any(|range| range.contains(&next)));
}

#[test]
fn dead_ranges_leave_ordinary_prose_alone() {
    let src = "just [[plan]] in prose";
    let open = src.find("[[").expect("bracket");
    let dead = jotter_parser::wikilink::dead_ranges(src);
    assert!(!dead.iter().any(|range| range.contains(&open)));
}
