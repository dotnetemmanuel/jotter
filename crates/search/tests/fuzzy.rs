use jotter_search::fuzzy;

fn score(query: &str, candidate: &str) -> i32 {
    fuzzy(query, candidate)
        .unwrap_or_else(|| panic!("{query:?} should match {candidate:?}"))
        .score
}

#[test]
fn matches_a_scattered_subsequence() {
    assert!(fuzzy("p3pl", "notes/phase3-plan.md").is_some());
}

#[test]
fn rejects_characters_out_of_order() {
    assert!(fuzzy("lpa", "plan.md").is_none());
}

#[test]
fn rejects_a_character_that_is_absent() {
    assert!(fuzzy("planz", "plan.md").is_none());
}

#[test]
fn empty_query_matches_anything() {
    let hit = fuzzy("", "plan.md").expect("empty query matches");
    assert_eq!(hit.score, 0);
    assert!(hit.positions.is_empty());
}

#[test]
fn positions_point_at_the_matched_bytes() {
    let hit = fuzzy("pl", "a-plan").expect("match");
    assert_eq!(hit.positions, vec![2, 3]);
}

#[test]
fn positions_land_on_char_boundaries_of_multibyte_text() {
    let hit = fuzzy("re", "resume\u{301}.md").expect("match");
    let text = "resume\u{301}.md";
    for pos in hit.positions {
        assert!(text.is_char_boundary(pos), "byte {pos} splits a char");
    }
}

#[test]
fn word_boundary_beats_a_mid_word_match() {
    assert!(score("pl", "phase-plan") > score("pl", "apples"));
}

#[test]
fn adjacent_characters_beat_scattered_ones() {
    assert!(score("plan", "plan-notes") > score("plan", "p-l-a-n-notes"));
}

#[test]
fn a_match_in_the_filename_beats_the_same_match_in_a_folder() {
    assert!(score("plan", "notes/plan.md") > score("plan", "plan/notes.md"));
}

#[test]
fn shorter_candidates_win_an_otherwise_equal_match() {
    assert!(score("plan", "plan.md") > score("plan", "plan-and-more.md"));
}

#[test]
fn a_lowercase_query_ignores_case() {
    assert!(fuzzy("readme", "README.md").is_some());
}

#[test]
fn an_uppercase_query_character_demands_uppercase() {
    assert!(fuzzy("RE", "readme.md").is_none());
    assert!(fuzzy("RE", "README.md").is_some());
}

#[test]
fn an_exact_prefix_outscores_a_later_match() {
    assert!(score("plan", "plan.md") > score("plan", "my-plan.md"));
}
