use jotter_parser::frontmatter;

#[test]
fn a_note_without_frontmatter_yields_nothing() {
    let found = frontmatter::parse("# Just a heading\n\nbody\n");
    assert_eq!(found.title, None);
    assert!(found.tags.is_empty());
    assert!(found.raw.is_none());
}

#[test]
fn a_yaml_title_is_read() {
    let found = frontmatter::parse("---\ntitle: Old notes\n---\n\nbody\n");
    assert_eq!(found.title.as_deref(), Some("Old notes"));
}

#[test]
fn a_quoted_title_loses_its_quotes() {
    let found = frontmatter::parse("---\ntitle: \"Old notes\"\n---\n");
    assert_eq!(found.title.as_deref(), Some("Old notes"));
}

#[test]
fn an_empty_title_counts_as_absent() {
    let found = frontmatter::parse("---\ntitle:\n---\n");
    assert_eq!(found.title, None);
}

#[test]
fn a_tag_list_is_read() {
    let found = frontmatter::parse("---\ntags: [demo, phase-1]\n---\n");
    assert_eq!(found.tags, ["demo", "phase-1"]);
}

#[test]
fn a_block_tag_list_is_read() {
    let found = frontmatter::parse("---\ntags:\n  - demo\n  - phase-1\n---\n");
    assert_eq!(found.tags, ["demo", "phase-1"]);
}

#[test]
fn a_single_tag_string_is_read() {
    let found = frontmatter::parse("---\ntags: demo\n---\n");
    assert_eq!(found.tags, ["demo"]);
}

#[test]
fn a_leading_hash_is_stripped_from_a_tag() {
    let found = frontmatter::parse("---\ntags: [\"#demo\"]\n---\n");
    assert_eq!(found.tags, ["demo"]);
}

#[test]
fn aliases_are_read() {
    let found = frontmatter::parse("---\naliases: [old, older]\n---\n");
    assert_eq!(found.aliases, ["old", "older"]);
}

#[test]
fn timestamps_are_read_as_written() {
    let found = frontmatter::parse("---\ncreated: 2026-07-01\nupdated: 2026-07-28\n---\n");
    assert_eq!(found.created.as_deref(), Some("2026-07-01"));
    assert_eq!(found.updated.as_deref(), Some("2026-07-28"));
}

#[test]
fn the_raw_block_is_kept() {
    let found = frontmatter::parse("---\ntitle: A\nweight: 3\n---\n\nbody\n");
    let raw = found.raw.expect("raw block");
    assert!(raw.contains("weight"));
}

#[test]
fn frontmatter_must_open_the_document() {
    let found = frontmatter::parse("intro\n\n---\ntitle: Nope\n---\n");
    assert_eq!(found.title, None);
    assert!(found.raw.is_none());
}

#[test]
fn malformed_frontmatter_is_not_fatal() {
    let found = frontmatter::parse("---\ntitle: [unclosed\n---\n\nbody\n");
    assert_eq!(found.title, None);
}

#[test]
fn a_toml_block_is_read() {
    let found = frontmatter::parse("+++\ntitle = \"Old notes\"\n+++\n\nbody\n");
    assert_eq!(found.title.as_deref(), Some("Old notes"));
}
