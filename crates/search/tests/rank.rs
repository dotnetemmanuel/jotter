use jotter_search::rank;

fn paths() -> Vec<String> {
    vec![
        "archive/phase3-notes.md".to_string(),
        "notes/phase3-plan.md".to_string(),
        "inbox/groceries.md".to_string(),
    ]
}

fn ranked(query: &str, items: &[String]) -> Vec<String> {
    rank(query, items, |s: &String| s.as_str())
        .into_iter()
        .map(|(item, _)| item.clone())
        .collect()
}

#[test]
fn drops_items_that_do_not_match() {
    assert_eq!(ranked("phase3", &paths()).len(), 2);
}

#[test]
fn puts_the_best_match_first() {
    assert_eq!(ranked("p3pl", &paths()), vec!["notes/phase3-plan.md"]);
}

#[test]
fn empty_query_keeps_every_item_in_order() {
    assert_eq!(ranked("", &paths()), paths());
}

#[test]
fn equal_scores_keep_the_original_order() {
    let items = vec!["a/plan.md".to_string(), "b/plan.md".to_string()];
    assert_eq!(ranked("plan", &items), items);
}

#[test]
fn carries_the_match_positions_of_each_hit() {
    let items = vec!["plan.md".to_string()];
    let hits = rank("pl", &items, |s: &String| s.as_str());
    assert_eq!(hits[0].1.positions, vec![0, 1]);
}
