#![warn(clippy::pedantic)]
//! Fuzzy subsequence matching for jotter pickers: scoring plus the matched byte
//! positions, with no GTK, database, or filesystem dependency.

/// Points for matching one query character.
const SCORE_MATCH: i32 = 16;
/// Bonus for matching the very first character of the candidate.
const BONUS_FIRST_CHAR: i32 = 40;
/// Bonus for matching at a word start: after a separator, or a camel-case hump.
const BONUS_BOUNDARY: i32 = 24;
/// Bonus for a character matched directly after the previous one.
const BONUS_CONSECUTIVE: i32 = 30;
/// Bonus for matching in the filename rather than the directory part.
const BONUS_FILENAME: i32 = 10;
/// Cost of opening a gap between two matched characters.
const GAP_START: i32 = -5;
/// Cost of each further skipped character inside a gap.
const GAP_EXTENSION: i32 = -2;

/// Characters that make the next character a word start.
const SEPARATORS: [char; 7] = ['/', '\\', '-', '_', '.', ' ', ':'];

/// Sentinel for "no alignment reaches here", kept far from overflow.
const NEG: i32 = i32::MIN / 4;

/// A successful match: how good it is, and which bytes of the candidate matched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    /// Higher is better. Only comparable between candidates scored by one query.
    pub score: i32,
    /// Byte offsets into the candidate, ascending, each on a char boundary.
    pub positions: Vec<usize>,
}

/// Scores `candidate` against `query`, or `None` when the query is not a
/// subsequence of it.
///
/// An all-lowercase query matches case-insensitively; any uppercase character in
/// the query makes the whole match case-sensitive.
#[must_use]
pub fn fuzzy(query: &str, candidate: &str) -> Option<Match> {
    let sensitive = query.chars().any(char::is_uppercase);
    let needle: Vec<char> = query.chars().map(|c| fold(c, sensitive)).collect();
    if needle.is_empty() {
        return Some(Match {
            score: 0,
            positions: Vec::new(),
        });
    }

    let hay: Vec<(usize, char)> = candidate.char_indices().collect();
    if needle.len() > hay.len() || !is_subsequence(&needle, &hay, sensitive) {
        return None;
    }

    let bonuses = position_bonuses(&hay);
    let (scores, parents) = align(&needle, &hay, sensitive, &bonuses);

    let last = (needle.len() - 1) * hay.len();
    let (end, best) = (0..hay.len())
        .map(|j| (j, scores[last + j]))
        .max_by_key(|&(_, score)| score)?;
    if best <= NEG {
        return None;
    }

    let length = i32::try_from(hay.len()).unwrap_or(i32::MAX);
    Some(Match {
        score: best - length,
        positions: trace(&hay, &parents, end, needle.len()),
    })
}

/// Ranks `items` against `query`, best first, dropping the ones that do not match.
///
/// `text` picks the string each item is matched on; equal scores keep the input order.
pub fn rank<'a, T>(query: &str, items: &'a [T], text: impl Fn(&T) -> &str) -> Vec<(&'a T, Match)> {
    let mut hits: Vec<(&T, Match)> = items
        .iter()
        .filter_map(|item| fuzzy(query, text(item)).map(|hit| (item, hit)))
        .collect();
    hits.sort_by_key(|(_, hit)| std::cmp::Reverse(hit.score));
    hits
}

/// Fills the score and parent matrices for every (query char, candidate char) pair.
fn align(
    needle: &[char],
    hay: &[(usize, char)],
    sensitive: bool,
    bonuses: &[i32],
) -> (Vec<i32>, Vec<usize>) {
    let width = hay.len();
    let mut scores = vec![NEG; needle.len() * width];
    let mut parents = vec![usize::MAX; needle.len() * width];

    for (i, &want) in needle.iter().enumerate() {
        // Best alignment of the previous row that reaches this column across a gap.
        let mut carry = NEG;
        let mut carry_from = usize::MAX;
        for j in 0..width {
            if i > 0 && j >= 2 {
                let opened = add(scores[(i - 1) * width + (j - 2)], GAP_START);
                let extended = add(carry, GAP_EXTENSION);
                if opened >= extended {
                    carry = opened;
                    carry_from = j - 2;
                } else {
                    carry = extended;
                }
            }
            if fold(hay[j].1, sensitive) != want {
                continue;
            }
            let base = SCORE_MATCH + bonuses[j];
            if i == 0 {
                scores[j] = base;
                continue;
            }
            let consecutive = if j == 0 {
                NEG
            } else {
                add(scores[(i - 1) * width + (j - 1)], BONUS_CONSECUTIVE)
            };
            let (best, from) = if consecutive >= carry {
                (consecutive, j.wrapping_sub(1))
            } else {
                (carry, carry_from)
            };
            if best <= NEG {
                continue;
            }
            scores[i * width + j] = base + best;
            parents[i * width + j] = from;
        }
    }
    (scores, parents)
}

/// Walks the parent chain back from the last matched column into byte offsets.
fn trace(hay: &[(usize, char)], parents: &[usize], end: usize, rows: usize) -> Vec<usize> {
    let width = hay.len();
    let mut positions = Vec::with_capacity(rows);
    let mut column = end;
    for row in (0..rows).rev() {
        positions.push(hay[column].0);
        if row == 0 {
            break;
        }
        column = parents[row * width + column];
    }
    positions.reverse();
    positions
}

/// Bonus each candidate position earns from its neighbours, before matching.
fn position_bonuses(hay: &[(usize, char)]) -> Vec<i32> {
    let filename_start = hay
        .iter()
        .rposition(|&(_, c)| c == '/' || c == '\\')
        .map_or(0, |slash| slash + 1);

    hay.iter()
        .enumerate()
        .map(|(j, &(_, current))| {
            let boundary = if j == 0 {
                BONUS_FIRST_CHAR
            } else {
                let previous = hay[j - 1].1;
                let camel = !previous.is_uppercase() && current.is_uppercase();
                if SEPARATORS.contains(&previous) || camel {
                    BONUS_BOUNDARY
                } else {
                    0
                }
            };
            let filename = if j >= filename_start {
                BONUS_FILENAME
            } else {
                0
            };
            boundary + filename
        })
        .collect()
}

/// Cheap rejection before the quadratic pass.
fn is_subsequence(needle: &[char], hay: &[(usize, char)], sensitive: bool) -> bool {
    let mut wanted = needle.iter();
    let mut next = wanted.next();
    for &(_, c) in hay {
        if next == Some(&fold(c, sensitive)) {
            next = wanted.next();
        }
    }
    next.is_none()
}

fn fold(c: char, sensitive: bool) -> char {
    if sensitive { c } else { c.to_ascii_lowercase() }
}

/// Adds to a score unless it is the "unreachable" sentinel.
fn add(score: i32, delta: i32) -> i32 {
    if score <= NEG { NEG } else { score + delta }
}
