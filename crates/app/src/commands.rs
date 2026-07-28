//! Command mode for the picker: the `>` prefix, and the command list behind it.

use crate::picker::Row;

/// Prefix that switches the picker from notes to commands.
pub const PREFIX: char = '>';

/// A command the palette can run, named by the app action it activates.
pub struct Command {
    /// Action name without the `app.` prefix.
    pub action: String,
    /// Human-readable name, the text the query matches against.
    pub title: String,
    /// Accelerator label such as `Ctrl+S`, shown dimmed. Empty when unbound.
    pub accel: String,
}

/// The command part of `query`, or `None` when the picker is in note mode.
pub fn command_query(query: &str) -> Option<&str> {
    Some(query.strip_prefix(PREFIX)?.trim_start())
}

/// True when both queries put the picker in the same mode.
pub fn same_mode(one: &str, other: &str) -> bool {
    command_query(one).is_some() == command_query(other).is_some()
}

/// Rows for the command list, capped at `limit`.
pub fn rows(query: &str, commands: &[Command], limit: usize) -> Vec<Row> {
    jotter_search::rank(query, commands, |command| command.title.as_str())
        .into_iter()
        .take(limit)
        .map(|(command, hit)| Row {
            key: command.action.clone(),
            label: command.title.clone(),
            detail: command.accel.clone(),
            positions: hit.positions,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{Command, command_query, rows};

    fn commands() -> Vec<Command> {
        [
            ("save", "Save note", "Ctrl+S"),
            ("toggle-theme", "Toggle light and dark theme", "Ctrl+T"),
            ("toggle-sidebar", "Toggle sidebar", "Ctrl+B"),
        ]
        .into_iter()
        .map(|(action, title, accel)| Command {
            action: action.to_string(),
            title: title.to_string(),
            accel: accel.to_string(),
        })
        .collect()
    }

    fn keys(query: &str, limit: usize) -> Vec<String> {
        rows(query, &commands(), limit)
            .into_iter()
            .map(|row| row.key)
            .collect()
    }

    #[test]
    fn a_plain_query_is_not_command_mode() {
        assert_eq!(command_query("plan"), None);
    }

    #[test]
    fn the_prefix_alone_is_an_empty_command_query() {
        assert_eq!(command_query(">"), Some(""));
    }

    #[test]
    fn the_prefix_strips_off_the_query() {
        assert_eq!(command_query(">theme"), Some("theme"));
    }

    #[test]
    fn space_after_the_prefix_is_ignored() {
        assert_eq!(command_query("> theme"), Some("theme"));
    }

    #[test]
    fn two_note_queries_share_a_mode() {
        assert!(super::same_mode("", "plan"));
    }

    #[test]
    fn two_command_queries_share_a_mode() {
        assert!(super::same_mode(">", ">theme"));
    }

    #[test]
    fn a_note_query_and_a_command_query_differ() {
        assert!(!super::same_mode("", ">"));
        assert!(!super::same_mode(">theme", "plan"));
    }

    #[test]
    fn an_empty_command_query_lists_every_command() {
        assert_eq!(keys("", 10).len(), 3);
    }

    #[test]
    fn commands_are_matched_by_title() {
        assert_eq!(keys("theme", 10), ["toggle-theme"]);
    }

    #[test]
    fn the_limit_caps_the_row_count() {
        assert_eq!(keys("toggle", 1).len(), 1);
    }

    #[test]
    fn a_row_shows_the_title_and_its_accelerator() {
        let found = rows("save", &commands(), 10);
        assert_eq!(found[0].label, "Save note");
        assert_eq!(found[0].detail, "Ctrl+S");
    }

    #[test]
    fn a_row_highlights_the_matched_characters_of_the_title() {
        let found = rows("save", &commands(), 10);
        assert_eq!(found[0].positions, vec![0, 1, 2, 3]);
    }
}
