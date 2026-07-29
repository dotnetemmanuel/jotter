//! The state behind the conflict resolver: which files conflict, how each block
//! has been answered, and where the cursor is.
//!
//! Pure, so the counting and the stepping are tested without a window.

use jotter_git::conflict::{Choice, Region, Span, count};

/// One conflicted note, with an answer slot per block.
pub struct File {
    /// Vault-relative path.
    pub path: String,
    /// The note split into text and blocks.
    pub spans: Vec<Span>,
    /// One entry per block, in order.
    pub choices: Vec<Choice>,
}

impl File {
    /// Builds a file with every block unanswered.
    #[must_use]
    pub fn new(path: String, spans: Vec<Span>) -> Self {
        let choices = vec![Choice::Unresolved; count(&spans)];
        Self {
            path,
            spans,
            choices,
        }
    }

    /// How many blocks this note holds.
    #[must_use]
    pub fn blocks(&self) -> usize {
        self.choices.len()
    }

    /// How many have been answered.
    #[must_use]
    pub fn answered(&self) -> usize {
        self.choices
            .iter()
            .filter(|choice| !choice.is_unresolved())
            .count()
    }

    /// The nth block of this note.
    #[must_use]
    pub fn region(&self, block: usize) -> Option<&Region> {
        self.spans
            .iter()
            .filter_map(|span| match span {
                Span::Conflict(region) => Some(region),
                Span::Text(_) => None,
            })
            .nth(block)
    }
}

/// Every conflicted note, and the cursor into them.
pub struct Model {
    /// The conflicted notes, in the order git reported them.
    pub files: Vec<File>,
    /// Index of the note under the cursor.
    pub file: usize,
    /// Index of the block under the cursor, within that note.
    pub block: usize,
}

impl Model {
    /// Builds a model over `files`, cursor on the first block.
    #[must_use]
    pub fn new(files: Vec<File>) -> Self {
        Self {
            files,
            file: 0,
            block: 0,
        }
    }

    /// The note under the cursor.
    #[must_use]
    pub fn current(&self) -> Option<&File> {
        self.files.get(self.file)
    }

    /// The block under the cursor.
    #[must_use]
    pub fn region(&self) -> Option<&Region> {
        self.current()?.region(self.block)
    }

    /// The answer for the block under the cursor.
    #[must_use]
    pub fn choice(&self) -> Choice {
        self.current()
            .and_then(|file| file.choices.get(self.block))
            .cloned()
            .unwrap_or_default()
    }

    /// Answers the block under the cursor.
    pub fn answer(&mut self, choice: Choice) {
        if let Some(file) = self.files.get_mut(self.file)
            && let Some(slot) = file.choices.get_mut(self.block)
        {
            *slot = choice;
        }
    }

    /// Answered and total block counts across every note.
    #[must_use]
    pub fn progress(&self) -> (usize, usize) {
        self.files.iter().fold((0, 0), |(done, total), file| {
            (done + file.answered(), total + file.blocks())
        })
    }

    /// Whether every block everywhere has an answer.
    #[must_use]
    pub fn all_answered(&self) -> bool {
        let (done, total) = self.progress();
        total > 0 && done == total
    }

    /// Every block in the tree as (file, block) pairs, in reading order.
    fn flattened(&self) -> Vec<(usize, usize)> {
        self.files
            .iter()
            .enumerate()
            .flat_map(|(index, file)| (0..file.blocks()).map(move |block| (index, block)))
            .collect()
    }

    /// Position of the cursor among every block, for "conflict 2 of 5".
    #[must_use]
    pub fn position(&self) -> usize {
        self.flattened()
            .iter()
            .position(|(file, block)| *file == self.file && *block == self.block)
            .unwrap_or(0)
    }

    /// Moves the cursor one block forward or back, wrapping at both ends.
    ///
    /// Wraps because the last thing anyone wants after answering the final block
    /// is to be told there is nowhere to go; notes with no blocks are skipped.
    pub fn step(&mut self, forward: bool) {
        let blocks = self.flattened();
        if blocks.is_empty() {
            return;
        }
        let at = self.position();
        let next = if forward {
            (at + 1) % blocks.len()
        } else {
            (at + blocks.len() - 1) % blocks.len()
        };
        (self.file, self.block) = blocks[next];
    }

    /// Jumps to the first block of the next or previous conflicted note.
    pub fn jump_file(&mut self, forward: bool) {
        let files = self.files.len();
        if files == 0 {
            return;
        }
        for offset in 1..=files {
            let index = if forward {
                (self.file + offset) % files
            } else {
                (self.file + files - (offset % files)) % files
            };
            if self.files[index].blocks() > 0 {
                self.file = index;
                self.block = 0;
                return;
            }
        }
    }
}

/// The line above the panes: which block, and in which note.
#[must_use]
pub fn heading(model: &Model) -> String {
    let (_, total) = model.progress();
    let Some(file) = model.current() else {
        return "No conflicts".to_string();
    };
    format!(
        "Conflict {} of {total} \u{b7} {}",
        model.position() + 1,
        file.path
    )
}

/// The progress line: how much of the work is done.
#[must_use]
pub fn progress_text(model: &Model) -> String {
    let (done, total) = model.progress();
    if total > 0 && done == total {
        return "All resolved".to_string();
    }
    format!("{done} of {total} resolved")
}

#[cfg(test)]
mod tests {
    use super::{File, Model, heading, progress_text};
    use jotter_git::conflict::{Choice, parse};

    fn file(path: &str, blocks: usize) -> File {
        let blocks = (0..blocks).fold(String::from("head\n"), |mut text, n| {
            use std::fmt::Write;
            let _ = write!(text, "<<<<<<<\nincoming {n}\n=======\nyours {n}\n>>>>>>>\n");
            text
        });
        File::new(path.to_string(), parse(&blocks).unwrap())
    }

    fn model() -> Model {
        Model::new(vec![file("a.md", 2), file("b.md", 1)])
    }

    #[test]
    fn a_fresh_model_has_answered_nothing() {
        let model = model();
        assert_eq!(model.progress(), (0, 3));
        assert!(!model.all_answered());
        assert_eq!(progress_text(&model), "0 of 3 resolved");
    }

    #[test]
    fn answering_moves_the_count() {
        let mut model = model();
        model.answer(Choice::Yours);
        assert_eq!(model.progress(), (1, 3));
        assert_eq!(progress_text(&model), "1 of 3 resolved");
    }

    #[test]
    fn answering_everything_is_noticed() {
        let mut model = model();
        for _ in 0..3 {
            model.answer(Choice::Incoming);
            model.step(true);
        }
        assert!(model.all_answered());
        assert_eq!(progress_text(&model), "All resolved");
    }

    #[test]
    fn stepping_crosses_from_one_note_into_the_next() {
        let mut model = model();
        model.step(true);
        assert_eq!((model.file, model.block), (0, 1));
        model.step(true);
        assert_eq!((model.file, model.block), (1, 0));
    }

    #[test]
    fn stepping_off_the_end_wraps_to_the_start() {
        let mut model = model();
        model.step(false);
        assert_eq!((model.file, model.block), (1, 0));
        model.step(true);
        assert_eq!((model.file, model.block), (0, 0));
    }

    #[test]
    fn jumping_files_lands_on_the_first_block() {
        let mut model = model();
        model.step(true);
        model.jump_file(true);
        assert_eq!((model.file, model.block), (1, 0));
        model.jump_file(true);
        assert_eq!((model.file, model.block), (0, 0));
    }

    #[test]
    fn a_note_with_no_blocks_is_skipped() {
        let mut model = Model::new(vec![file("a.md", 1), file("empty.md", 0), file("c.md", 1)]);
        model.jump_file(true);
        assert_eq!(model.file, 2);
    }

    #[test]
    fn the_heading_counts_from_one() {
        let mut model = model();
        assert_eq!(heading(&model), "Conflict 1 of 3 \u{b7} a.md");
        model.step(true);
        model.step(true);
        assert_eq!(heading(&model), "Conflict 3 of 3 \u{b7} b.md");
    }

    #[test]
    fn a_file_counts_its_own_answers() {
        let mut model = model();
        model.answer(Choice::Both);
        assert_eq!(model.current().unwrap().answered(), 1);
        assert_eq!(model.current().unwrap().blocks(), 2);
    }

    #[test]
    fn the_region_under_the_cursor_is_the_right_one() {
        let mut model = model();
        model.step(true);
        assert_eq!(model.region().unwrap().yours, ["yours 1"]);
    }
}
