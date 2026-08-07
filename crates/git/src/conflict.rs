//! Turning the markers git leaves in a file into something a person can answer.
//!
//! A conflicted file is parsed into ordered [`Span`]s: runs of untouched text
//! interleaved with [`Region`]s, one per `<<<<<<<` block. Each region is
//! answered on its own, and [`apply`] reassembles the file from the answers.
//! Concatenating the context spans with a chosen side round-trips the file
//! exactly, so resolving never rewrites a line nobody chose to change.
//!
//! The sides are called **incoming** and **yours**, never ours and theirs.
//! During a rebase git checks the upstream out as HEAD and replays your commit
//! on top, so its `--ours` is the remote and `--theirs` is you. Naming the sides
//! after where the work came from removes the trap entirely: jotter only ever
//! rebases, so the first section of a marker block is always the incoming side
//! and the second is always yours.

use thiserror::Error;

/// A conflicted file that could not be read as one.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ParseError {
    /// A block opened with `<<<<<<<` and the file ended before `>>>>>>>`.
    #[error("unterminated conflict block")]
    Unterminated,
}

/// One conflicted block, split into the sides that disagree.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Region {
    /// Lines from the remote, above the `=======`.
    pub incoming: Vec<String>,
    /// The common ancestor, present only when git wrote diff3 markers.
    pub base: Option<Vec<String>>,
    /// Lines you wrote, below the `=======`.
    pub yours: Vec<String>,
}

/// A slice of a parsed file: untouched text, or a block to answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Span {
    /// Text nobody disagreed about, newlines intact.
    Text(String),
    /// A block awaiting a choice.
    Conflict(Region),
}

/// How one region is answered.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Choice {
    /// Not answered yet.
    #[default]
    Unresolved,
    /// Take the remote's lines.
    Incoming,
    /// Take your lines.
    Yours,
    /// Take both, incoming first.
    Both,
    /// Take this text instead, written by hand.
    Custom(String),
}

impl Choice {
    /// Whether this region still needs an answer.
    #[must_use]
    pub fn is_unresolved(&self) -> bool {
        matches!(self, Choice::Unresolved)
    }
}

/// Splits `text` into spans, in order.
///
/// # Errors
/// [`ParseError::Unterminated`] when a block never closes.
pub fn parse(text: &str) -> Result<Vec<Span>, ParseError> {
    if text.is_empty() {
        return Ok(Vec::new());
    }

    let mut spans = Vec::new();
    let mut context = String::new();
    let mut region: Option<Region> = None;
    let mut section = Section::Incoming;

    for raw in text.split_inclusive('\n') {
        let line = raw.strip_suffix('\n').unwrap_or(raw);
        match () {
            () if line.starts_with("<<<<<<<") => {
                if !context.is_empty() {
                    spans.push(Span::Text(std::mem::take(&mut context)));
                }
                region = Some(Region::default());
                section = Section::Incoming;
            }
            () if region.is_some() && line.starts_with("|||||||") => {
                if let Some(region) = region.as_mut() {
                    region.base = Some(Vec::new());
                }
                section = Section::Base;
            }
            () if region.is_some() && line.starts_with("=======") => section = Section::Yours,
            () if region.is_some() && line.starts_with(">>>>>>>") => {
                if let Some(region) = region.take() {
                    spans.push(Span::Conflict(region));
                }
            }
            () => match region.as_mut() {
                Some(region) => region.push(section, line),
                None => context.push_str(raw),
            },
        }
    }

    if region.is_some() {
        return Err(ParseError::Unterminated);
    }
    if !context.is_empty() {
        spans.push(Span::Text(context));
    }
    Ok(spans)
}

/// Which side of a block the parser is reading.
#[derive(Clone, Copy)]
enum Section {
    Incoming,
    Base,
    Yours,
}

impl Region {
    fn push(&mut self, section: Section, line: &str) {
        match section {
            Section::Incoming => self.incoming.push(line.to_string()),
            Section::Base => {
                if let Some(base) = self.base.as_mut() {
                    base.push(line.to_string());
                }
            }
            Section::Yours => self.yours.push(line.to_string()),
        }
    }
}

/// How many blocks `spans` holds.
#[must_use]
pub fn count(spans: &[Span]) -> usize {
    spans
        .iter()
        .filter(|span| matches!(span, Span::Conflict(_)))
        .count()
}

/// Rebuilds the file from `spans` and one choice per block, in order.
///
/// A block with no answer is written back as bare markers, so a half-resolved
/// file can be parsed again and finished later.
#[must_use]
pub fn apply(spans: &[Span], choices: &[Choice]) -> String {
    let mut out = String::new();
    let mut block = 0;

    for span in spans {
        let region = match span {
            Span::Text(text) => {
                out.push_str(text);
                continue;
            }
            Span::Conflict(region) => region,
        };
        let choice = choices.get(block).cloned().unwrap_or_default();
        block += 1;

        match choice {
            Choice::Incoming => push_lines(&mut out, &region.incoming),
            Choice::Yours => push_lines(&mut out, &region.yours),
            Choice::Both => {
                push_lines(&mut out, &region.incoming);
                push_lines(&mut out, &region.yours);
            }
            Choice::Custom(text) => {
                out.push_str(text.strip_suffix('\n').unwrap_or(&text));
                out.push('\n');
            }
            Choice::Unresolved => {
                out.push_str("<<<<<<<\n");
                push_lines(&mut out, &region.incoming);
                if let Some(base) = &region.base {
                    out.push_str("|||||||\n");
                    push_lines(&mut out, base);
                }
                out.push_str("=======\n");
                push_lines(&mut out, &region.yours);
                out.push_str(">>>>>>>\n");
            }
        }
    }
    out
}

fn push_lines(out: &mut String, lines: &[String]) {
    for line in lines {
        out.push_str(line);
        out.push('\n');
    }
}

#[cfg(test)]
mod tests {
    use super::{Choice, ParseError, Region, Span, apply, count, parse};

    const CONFLICTED: &str = "\
# Note

<<<<<<< HEAD
from the remote
=======
what I wrote
>>>>>>> my commit

tail
";

    fn regions(text: &str) -> Vec<Region> {
        parse(text)
            .unwrap()
            .into_iter()
            .filter_map(|span| match span {
                Span::Conflict(region) => Some(region),
                Span::Text(_) => None,
            })
            .collect()
    }

    #[test]
    fn a_file_with_no_conflict_is_one_run_of_text() {
        let spans = parse("just text\nand more\n").unwrap();
        assert_eq!(spans, [Span::Text("just text\nand more\n".to_string())]);
        assert_eq!(count(&spans), 0);
    }

    #[test]
    fn an_empty_file_has_nothing_in_it() {
        assert!(parse("").unwrap().is_empty());
    }

    #[test]
    fn the_two_sides_come_out_in_order() {
        let region = regions(CONFLICTED).remove(0);
        assert_eq!(region.incoming, ["from the remote"]);
        assert_eq!(region.yours, ["what I wrote"]);
        assert!(region.base.is_none());
    }

    #[test]
    fn text_around_a_block_is_kept_whole() {
        let spans = parse(CONFLICTED).unwrap();
        assert_eq!(spans.len(), 3);
        assert_eq!(spans[0], Span::Text("# Note\n\n".to_string()));
        assert_eq!(spans[2], Span::Text("\ntail\n".to_string()));
    }

    #[test]
    fn a_diff3_ancestor_is_kept_when_git_wrote_one() {
        let text = "<<<<<<< HEAD\nnew\n||||||| base\nold\n=======\nmine\n>>>>>>> commit\n";
        let region = regions(text).remove(0);
        assert_eq!(region.base.as_deref(), Some(["old".to_string()].as_slice()));
    }

    #[test]
    fn several_blocks_are_all_found() {
        let text = "a\n<<<<<<<\n1\n=======\n2\n>>>>>>>\nb\n<<<<<<<\n3\n=======\n4\n>>>>>>>\n";
        assert_eq!(count(&parse(text).unwrap()), 2);
    }

    #[test]
    fn a_block_that_never_closes_is_an_error() {
        let text = "<<<<<<< HEAD\nfrom the remote\n=======\nwhat I wrote\n";
        assert_eq!(parse(text), Err(ParseError::Unterminated));
    }

    fn resolved(choice: Choice) -> String {
        apply(&parse(CONFLICTED).unwrap(), &[choice])
    }

    #[test]
    fn taking_the_incoming_side_keeps_only_it() {
        assert_eq!(
            resolved(Choice::Incoming),
            "# Note\n\nfrom the remote\n\ntail\n"
        );
    }

    #[test]
    fn taking_your_side_keeps_only_yours() {
        assert_eq!(resolved(Choice::Yours), "# Note\n\nwhat I wrote\n\ntail\n");
    }

    #[test]
    fn taking_both_puts_the_incoming_side_first() {
        assert_eq!(
            resolved(Choice::Both),
            "# Note\n\nfrom the remote\nwhat I wrote\n\ntail\n"
        );
    }

    #[test]
    fn a_hand_written_answer_replaces_the_block() {
        assert_eq!(
            resolved(Choice::Custom("both, but shorter".to_string())),
            "# Note\n\nboth, but shorter\n\ntail\n"
        );
    }

    #[test]
    fn a_hand_written_answer_does_not_double_its_newline() {
        assert_eq!(
            resolved(Choice::Custom("one line\n".to_string())),
            "# Note\n\none line\n\ntail\n"
        );
    }

    #[test]
    fn an_unanswered_block_survives_a_round_trip() {
        let once = apply(&parse(CONFLICTED).unwrap(), &[]);
        let twice = apply(&parse(&once).unwrap(), &[]);
        assert_eq!(once, twice, "parsing its own output changed it");
        assert!(once.contains("<<<<<<<\nfrom the remote\n=======\nwhat I wrote\n>>>>>>>\n"));
    }

    #[test]
    fn answering_one_block_leaves_the_other_alone() {
        let text = "<<<<<<<\n1\n=======\n2\n>>>>>>>\nmiddle\n<<<<<<<\n3\n=======\n4\n>>>>>>>\n";
        let spans = parse(text).unwrap();
        let out = apply(&spans, &[Choice::Yours]);
        assert!(out.starts_with("2\nmiddle\n"));
        assert!(out.contains("<<<<<<<\n3\n=======\n4\n>>>>>>>\n"));
    }

    #[test]
    fn a_file_without_a_trailing_newline_keeps_it_that_way() {
        let spans = parse("head\n<<<<<<<\n1\n=======\n2\n>>>>>>>\nno newline").unwrap();
        assert_eq!(apply(&spans, &[Choice::Incoming]), "head\n1\nno newline");
    }
}
