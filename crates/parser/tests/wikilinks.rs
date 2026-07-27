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
    let html = render("See [[standup]] and [[missing]].\n", &test_code(), &stub).html;
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
    let html = render("```\n[[standup]]\n```\n", &test_code(), &stub).html;
    assert!(!html.contains("jotter-note:"), "code must not be linkified");
    assert!(html.contains("[[standup]]"), "code must keep its text: {html}");
}

#[test]
fn heading_lines_are_unaffected_by_rewriting() {
    let src = "# Title\n\n[[standup]]\n\n## Later\n";
    let rendered = render(src, &test_code(), &stub);
    assert_eq!(rendered.headings[0].source_line, 1);
    assert_eq!(rendered.headings[1].source_line, 5);
}
