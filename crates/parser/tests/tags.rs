use jotter_parser::tags::scan;

#[test]
fn an_inline_tag_is_found() {
    assert_eq!(scan("about #project today"), ["project"]);
}

#[test]
fn a_tag_at_the_start_of_a_line_is_found() {
    assert_eq!(scan("intro\n#project and more"), ["project"]);
}

#[test]
fn a_heading_is_not_a_tag() {
    assert_eq!(scan("# Heading\n\nbody"), Vec::<String>::new());
}

#[test]
fn every_heading_level_is_ignored() {
    assert_eq!(scan("### Deep heading"), Vec::<String>::new());
}

#[test]
fn a_url_fragment_is_not_a_tag() {
    assert_eq!(scan("see https://x.dev/page#section"), Vec::<String>::new());
}

#[test]
fn a_bare_hash_is_not_a_tag() {
    assert_eq!(scan("issue # 4 and #"), Vec::<String>::new());
}

#[test]
fn a_tag_may_not_start_with_a_digit() {
    assert_eq!(scan("#2026 and #q1"), ["q1"]);
}

#[test]
fn dashes_underscores_and_slashes_are_part_of_a_tag() {
    assert_eq!(
        scan("#phase-1 #my_tag #work/urgent"),
        ["phase-1", "my_tag", "work/urgent"]
    );
}

#[test]
fn punctuation_ends_a_tag() {
    assert_eq!(scan("#project, #demo."), ["project", "demo"]);
}

#[test]
fn a_tag_inside_inline_code_is_ignored() {
    assert_eq!(scan("use `#project` here"), Vec::<String>::new());
}

#[test]
fn a_tag_inside_a_fenced_block_is_ignored() {
    assert_eq!(scan("```\n#project\n```\n"), Vec::<String>::new());
}

#[test]
fn a_tag_in_frontmatter_is_ignored_by_the_scanner() {
    assert_eq!(scan("---\ntags: [demo]\n---\n\n#project"), ["project"]);
}

#[test]
fn duplicates_collapse_and_order_is_kept() {
    assert_eq!(scan("#a then #b then #a"), ["a", "b"]);
}

#[test]
fn tags_are_lowercased() {
    assert_eq!(scan("#Project and #project"), ["project"]);
}
