//! Turns the vault note list into picker rows: recents when the query is empty,
//! fuzzy matches over path and title otherwise.

use crate::picker::Row;

/// A note the switcher can offer.
pub struct Candidate {
    /// Vault-relative path, the text shown and matched first.
    pub path: String,
    /// Display title, matched as a fallback and shown when it adds information.
    pub title: String,
}

/// Rows to show for `query`, capped at `limit`.
pub fn rows(query: &str, notes: &[Candidate], recents: &[String], limit: usize) -> Vec<Row> {
    if query.is_empty() {
        return recents
            .iter()
            .filter_map(|path| notes.iter().find(|note| &note.path == path))
            .take(limit)
            .map(|note| row(note, Vec::new()))
            .collect();
    }

    let mut scored: Vec<(i32, &Candidate, Vec<usize>)> = notes
        .iter()
        .filter_map(|note| {
            let on_path = jotter_search::fuzzy(query, &note.path);
            let on_title = jotter_search::fuzzy(query, &note.title);
            let score = match (&on_path, &on_title) {
                (Some(path), Some(title)) => path.score.max(title.score),
                (Some(hit), None) | (None, Some(hit)) => hit.score,
                (None, None) => return None,
            };
            let positions = on_path.map(|hit| hit.positions).unwrap_or_default();
            Some((score, note, positions))
        })
        .collect();
    scored.sort_by_key(|(score, _, _)| std::cmp::Reverse(*score));
    scored
        .into_iter()
        .take(limit)
        .map(|(_, note, positions)| row(note, positions))
        .collect()
}

/// One row for `note`, dropping a title that only repeats the filename stem.
fn row(note: &Candidate, positions: Vec<usize>) -> Row {
    let stem = note
        .path
        .rsplit('/')
        .next()
        .and_then(|name| name.strip_suffix(".md"))
        .unwrap_or(&note.path);
    let detail = if note.title.eq_ignore_ascii_case(stem) {
        String::new()
    } else {
        note.title.clone()
    };
    Row {
        key: note.path.clone(),
        label: note.path.clone(),
        detail,
        positions,
    }
}

#[cfg(test)]
mod tests {
    use super::{Candidate, rows};

    fn notes() -> Vec<Candidate> {
        [
            ("notes/phase3-plan.md", "Phase 3 plan"),
            ("archive/phase3-notes.md", "Old notes"),
            ("inbox/groceries.md", "groceries"),
        ]
        .into_iter()
        .map(|(path, title)| Candidate {
            path: path.to_string(),
            title: title.to_string(),
        })
        .collect()
    }

    fn keys(query: &str, recents: &[String], limit: usize) -> Vec<String> {
        rows(query, &notes(), recents, limit)
            .into_iter()
            .map(|row| row.key)
            .collect()
    }

    #[test]
    fn an_empty_query_lists_recents_in_order() {
        let recents = vec!["inbox/groceries.md".to_string(), "notes/phase3-plan.md".to_string()];
        assert_eq!(keys("", &recents, 10), recents);
    }

    #[test]
    fn an_empty_query_drops_a_recent_that_no_longer_exists() {
        let recents = vec!["gone.md".to_string(), "inbox/groceries.md".to_string()];
        assert_eq!(keys("", &recents, 10), ["inbox/groceries.md"]);
    }

    #[test]
    fn an_empty_query_with_no_recents_lists_nothing() {
        assert!(keys("", &[], 10).is_empty());
    }

    #[test]
    fn a_query_ranks_the_best_match_first() {
        assert_eq!(keys("p3pl", &[], 10), ["notes/phase3-plan.md"]);
    }

    #[test]
    fn a_query_ignores_recents() {
        let recents = vec!["inbox/groceries.md".to_string()];
        assert_eq!(keys("phase3", &recents, 10).len(), 2);
    }

    #[test]
    fn a_title_match_outranks_a_scattered_path_match() {
        assert_eq!(keys("old", &[], 10)[0], "archive/phase3-notes.md");
    }

    #[test]
    fn a_note_whose_title_alone_matches_is_offered() {
        assert_eq!(keys("oldnotes", &[], 10), ["archive/phase3-notes.md"]);
    }

    #[test]
    fn the_limit_caps_the_row_count() {
        assert_eq!(keys("md", &[], 1).len(), 1);
    }

    #[test]
    fn the_row_shows_the_path_and_highlights_the_matched_bytes() {
        let found = rows("plan", &notes(), &[], 10);
        assert_eq!(found[0].label, "notes/phase3-plan.md");
        assert_eq!(found[0].positions, vec![13, 14, 15, 16]);
    }

    #[test]
    fn the_detail_carries_the_title_when_it_differs_from_the_stem() {
        let found = rows("phase3-plan", &notes(), &[], 10);
        assert_eq!(found[0].detail, "Phase 3 plan");
    }

    #[test]
    fn the_detail_is_empty_when_the_title_only_repeats_the_stem() {
        let found = rows("groceries", &notes(), &[], 10);
        assert_eq!(found[0].detail, "");
    }
}
