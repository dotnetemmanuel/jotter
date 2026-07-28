//! Full-text search: turning typed text into an FTS5 query, and matched notes
//! into the lines worth showing.

use std::ops::Range;

/// A matching line of a note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Snippet {
    /// 0-based line number, for jumping the editor there.
    pub line: i32,
    /// The line itself, trimmed of surrounding whitespace.
    pub text: String,
    /// Byte ranges within `text` that matched, ascending.
    pub spans: Vec<Range<usize>>,
}

/// Builds an FTS5 query from what the user typed.
///
/// Words are joined with `AND`, quoted phrases survive, bare operators pass through, and the
/// word still being typed gets a prefix star so results appear as you go.
pub fn fts_query(input: &str) -> String {
    let tokens = tokenize(input);
    let last = tokens.len().saturating_sub(1);
    let mut out: Vec<String> = Vec::new();
    for (position, token) in tokens.iter().enumerate() {
        match token {
            Token::Operator(text) => out.push(text.clone()),
            Token::Phrase(text) => {
                join(&mut out);
                out.push(format!("\"{text}\""));
            }
            Token::Word(text) => {
                join(&mut out);
                // Only the word still under the caret matches as a prefix.
                let typing = position == last && !input.ends_with(char::is_whitespace);
                out.push(if typing {
                    format!("{text}*")
                } else {
                    text.clone()
                });
            }
        }
    }
    out.join(" ")
}

/// The plain words of `input`, for locating matches in note text.
pub fn terms(input: &str) -> Vec<String> {
    tokenize(input)
        .into_iter()
        .filter_map(|token| match token {
            Token::Word(text) | Token::Phrase(text) => Some(text),
            Token::Operator(_) => None,
        })
        .collect()
}

/// Lines of `text` containing any of `terms`, at most `limit` of them.
pub fn snippets(text: &str, terms: &[String], limit: usize) -> Vec<Snippet> {
    if terms.is_empty() {
        return Vec::new();
    }
    let needles: Vec<String> = terms.iter().map(|term| term.to_lowercase()).collect();
    let mut found = Vec::new();
    for (number, raw) in text.lines().enumerate() {
        if found.len() == limit {
            break;
        }
        let line = raw.trim();
        let haystack = line.to_lowercase();
        let mut spans: Vec<Range<usize>> = Vec::new();
        for needle in &needles {
            let mut from = 0;
            while let Some(offset) = haystack[from..].find(needle.as_str()) {
                let start = from + offset;
                spans.push(start..start + needle.len());
                from = start + needle.len();
            }
        }
        if spans.is_empty() {
            continue;
        }
        spans.sort_by_key(|span| span.start);
        found.push(Snippet {
            line: i32::try_from(number).unwrap_or(i32::MAX),
            text: line.to_string(),
            spans,
        });
    }
    found
}

/// Char offsets inside `spans`, the form the row highlighter takes.
pub fn highlight_positions(text: &str, spans: &[Range<usize>]) -> Vec<usize> {
    text.char_indices()
        .map(|(offset, _)| offset)
        .filter(|offset| spans.iter().any(|span| span.contains(offset)))
        .collect()
}

/// One piece of what the user typed.
enum Token {
    Word(String),
    Phrase(String),
    Operator(String),
}

/// Splits input into quoted phrases, bare operators, and words.
fn tokenize(input: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut rest = input;
    while !rest.is_empty() {
        let trimmed = rest.trim_start();
        if trimmed.is_empty() {
            break;
        }
        if let Some(after_quote) = trimmed.strip_prefix('"') {
            let (phrase, tail) = after_quote.split_once('"').unwrap_or((after_quote, ""));
            tokens.push(Token::Phrase(phrase.to_string()));
            rest = tail;
            continue;
        }
        let end = trimmed.find(char::is_whitespace).unwrap_or(trimmed.len());
        let (word, tail) = trimmed.split_at(end);
        tokens.push(if is_operator(word) {
            Token::Operator(word.to_string())
        } else {
            Token::Word(word.to_string())
        });
        rest = tail;
    }
    tokens
}

/// FTS5 operators the user may type deliberately.
fn is_operator(word: &str) -> bool {
    matches!(word, "AND" | "OR" | "NOT")
}

/// Inserts the implicit AND between two adjacent search terms.
fn join(out: &mut Vec<String>) {
    if out.last().is_some_and(|last| !is_operator(last)) {
        out.push("AND".to_string());
    }
}

#[cfg(test)]
mod tests {
    use std::ops::Range;

    use super::{fts_query, highlight_positions, snippets, terms};

    #[test]
    fn positions_cover_every_char_of_a_span() {
        let spans = [Range { start: 0, end: 3 }];
        assert_eq!(highlight_positions("gtk here", &spans), vec![0, 1, 2]);
    }

    #[test]
    fn positions_skip_text_outside_the_spans() {
        let spans = [Range { start: 2, end: 5 }];
        assert_eq!(highlight_positions("a gtk b", &spans), vec![2, 3, 4]);
    }

    #[test]
    fn positions_land_on_char_boundaries() {
        let text = "café gtk";
        let spans = [Range { start: 5, end: 8 }];
        assert_eq!(highlight_positions(text, &spans), vec![5, 6, 7]);
    }

    #[test]
    fn a_single_word_matches_as_a_prefix() {
        assert_eq!(fts_query("web"), "web*");
    }

    #[test]
    fn words_are_anded_with_only_the_last_as_a_prefix() {
        assert_eq!(fts_query("webkit ren"), "webkit AND ren*");
    }

    #[test]
    fn a_trailing_space_ends_the_word_being_typed() {
        assert_eq!(fts_query("webkit "), "webkit");
    }

    #[test]
    fn a_quoted_phrase_survives_whole() {
        assert_eq!(fts_query("\"oat milk\""), "\"oat milk\"");
    }

    #[test]
    fn a_phrase_and_a_word_combine() {
        assert_eq!(fts_query("\"oat milk\" cof"), "\"oat milk\" AND cof*");
    }

    #[test]
    fn operators_pass_through_untouched() {
        assert_eq!(fts_query("webkit OR gtk"), "webkit OR gtk*");
    }

    #[test]
    fn an_empty_query_stays_empty() {
        assert_eq!(fts_query("   "), "");
    }

    #[test]
    fn terms_drop_operators_and_quotes() {
        assert_eq!(terms("\"oat milk\" OR cof"), ["oat milk", "cof"]);
    }

    #[test]
    fn a_snippet_reports_the_line_and_where_it_matched() {
        let text = "first line\nabout webkit here\nlast";
        let found = snippets(text, &["webkit".to_string()], 3);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].line, 1);
        assert_eq!(found[0].text, "about webkit here");
        assert_eq!(found[0].spans, vec![6..12]);
    }

    #[test]
    fn snippet_matching_ignores_case() {
        let found = snippets("The WebKit engine", &["webkit".to_string()], 3);
        assert_eq!(found[0].spans, vec![4..10]);
    }

    #[test]
    fn a_line_matching_twice_reports_both_spans() {
        let found = snippets("gtk and gtk", &["gtk".to_string()], 3);
        assert_eq!(found[0].spans, vec![0..3, 8..11]);
    }

    #[test]
    fn snippets_stop_at_the_limit() {
        let text = "gtk\ngtk\ngtk\ngtk";
        assert_eq!(snippets(text, &["gtk".to_string()], 2).len(), 2);
    }

    #[test]
    fn a_line_is_trimmed_and_its_spans_follow() {
        let found = snippets("    indented gtk", &["gtk".to_string()], 3);
        assert_eq!(found[0].text, "indented gtk");
        assert_eq!(found[0].spans, vec![9..12]);
    }

    #[test]
    fn text_with_no_match_yields_nothing() {
        assert!(snippets("nothing here", &["gtk".to_string()], 3).is_empty());
    }

    #[test]
    fn no_terms_yields_nothing() {
        assert!(snippets("gtk everywhere", &[], 3).is_empty());
    }
}
