//! Snapshot test for the plain markdown-to-HTML pipeline on a fixture document.

const FIXTURE: &str = include_str!("fixtures/sample.md");

#[test]
fn markdown_to_html_matches_snapshot() {
    let html = jotter_parser::markdown_to_html(FIXTURE);
    insta::assert_snapshot!(html);
}
