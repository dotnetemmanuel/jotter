//! Moving a node in the tree, and the wikilink rewrite a move owes its linkers.
//!
//! A move is the same operation as a rename: both change a note's vault-relative
//! path, and both can break the links pointing at it. Bare `[[stem]]` links
//! survive a change of folder but not a change of name; path-form
//! `[[notes/plan]]` links survive neither. This module decides where a drop
//! lands and what each affected link should say afterwards, with no GTK and no
//! filesystem in sight so both halves are testable.

use std::collections::HashMap;
use std::path::Path;

use jotter_index::Index;
use jotter_parser::wikilink;
use jotter_vault::Vault;

use crate::links::{Resolver, format_wikilink, stem_of};
use crate::vault_session;

/// Where dragging `from` onto the folder `into` lands it, vault-relative.
///
/// `None` when the drop would do nothing or cannot work: the root itself, the
/// folder it already sits in, a folder onto itself, or a folder into its own
/// subtree.
#[must_use]
pub fn destination(from: &str, into: &str) -> Option<String> {
    let from = from.trim_end_matches('/');
    let into = into.trim_end_matches('/');
    if from.is_empty() {
        return None;
    }
    if from == into || into.starts_with(&format!("{from}/")) {
        return None;
    }
    let name = from.rsplit('/').next()?;
    let parent = from.strip_suffix(name)?.trim_end_matches('/');
    if parent == into {
        return None;
    }
    Some(crate::tree::join_rel(into, name))
}

/// The target a link written as `written` should say now its note lives at `to`.
///
/// A path-form link stays path-form, since the author spelled the folder out to
/// disambiguate and a move does not change that intent. A bare stem takes
/// `shortest`, the shortest target that still resolves to the note.
#[must_use]
pub fn new_target(written: &str, to: &str, shortest: &str) -> String {
    if written.contains('/') {
        return to.strip_suffix(".md").unwrap_or(to).to_owned();
    }
    shortest.to_owned()
}

/// Rewrites every wikilink in `text` that `retarget` gives a new target for,
/// keeping each link's heading and alias. `None` when nothing changed.
///
/// `retarget` is handed the target as written and answers with what it should
/// say instead, or `None` to leave the link alone.
#[must_use]
pub fn rewrite_links(text: &str, retarget: &dyn Fn(&str) -> Option<String>) -> Option<String> {
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0;
    let mut changed = false;
    for link in wikilink::scan(text) {
        let Some(target) = retarget(&link.target) else {
            continue;
        };
        if target == link.target {
            continue;
        }
        out.push_str(&text[cursor..link.range.start]);
        out.push_str(&format_wikilink(
            &target,
            link.heading.as_deref(),
            link.alias.as_deref(),
        ));
        cursor = link.range.end;
        changed = true;
    }
    if !changed {
        return None;
    }
    out.push_str(&text[cursor..]);
    Some(out)
}

/// What the status bar says once a move has happened.
///
/// A rename and a move are the same operation underneath, but not to read about,
/// so the wording follows whether the folder changed.
#[must_use]
pub fn message(from: &Path, to: &Path, relinked: usize) -> String {
    let name = to.file_name().and_then(|n| n.to_str()).unwrap_or("note");
    let landing = if from.parent() == to.parent() {
        format!("Renamed to {name}")
    } else {
        match to.parent().filter(|dir| !dir.as_os_str().is_empty()) {
            Some(dir) => format!("Moved {name} to {}", dir.display()),
            None => format!("Moved {name} to the vault root"),
        }
    };
    match relinked {
        0 => landing,
        1 => format!("{landing}, relinked 1 note"),
        many => format!("{landing}, relinked {many} notes"),
    }
}

/// What a move owes the notes that link into it, read before anything moves.
pub struct Plan {
    /// Every note the move touches, old index key to new index key.
    moved: HashMap<String, String>,
    /// Notes holding a link into `moved`, by their pre-move index key.
    linkers: Vec<String>,
    /// Link resolution as it stood before the move.
    before: Resolver,
}

/// Works out what moving `from` to `to` will change, before it changes.
///
/// Both halves have to be read up front: afterwards the resolver no longer knows
/// the old paths, and re-resolving the links table clears the rows that named
/// them.
#[must_use]
pub fn plan(vault: &Vault, index: &Index, from: &Path, to: &Path) -> Plan {
    let notes = index.all_notes().unwrap_or_default();
    let before = Resolver::new(notes.iter().map(|note| note.path.clone()));

    let from_key = vault_session::rel_to_key(from);
    let to_key = vault_session::rel_to_key(to);
    let prefix = format!("{from_key}/");
    let mut moved = HashMap::new();
    // A note the index has not reached yet still moves, so name it directly.
    if !crate::tree::is_dir_path(vault.root(), &from_key) {
        moved.insert(from_key, to_key.clone());
    }
    for note in &notes {
        if let Some(tail) = note.path.strip_prefix(&prefix) {
            moved.insert(note.path.clone(), format!("{to_key}/{tail}"));
        }
    }

    let mut linkers: Vec<String> = Vec::new();
    for target in moved.keys().cloned().chain(stem_rivals(&notes, &moved)) {
        for note in index.linking_notes(&target).unwrap_or_default() {
            if !linkers.contains(&note.path) {
                linkers.push(note.path);
            }
        }
    }
    linkers.sort();
    Plan { moved, linkers, before }
}

/// Notes that stay put but share a filename stem with one that is moving.
///
/// A bare `[[stem]]` means "the note called that, wherever it is", and a
/// collision resolves to the first path alphabetically. So a move can hand the
/// stem to a different note, and links naming the one that stayed put change
/// meaning without it ever moving. They have to be looked at too, or the move
/// silently retargets them.
fn stem_rivals(notes: &[jotter_index::Note], moved: &HashMap<String, String>) -> Vec<String> {
    let stems: Vec<&str> = moved.keys().filter_map(|path| stem_of(path)).collect();
    notes
        .iter()
        .filter(|note| !moved.contains_key(&note.path))
        .filter(|note| stem_of(&note.path).is_some_and(|stem| stems.contains(&stem)))
        .map(|note| note.path.clone())
        .collect()
}

/// Points the index rows of the moved notes at their new paths.
pub fn reindex(vault: &Vault, index: &Index, plan: &Plan) {
    for (old, new) in &plan.moved {
        let _ = vault_session::deindex_note(index, Path::new(old));
        let _ = vault_session::reindex_note(vault, index, Path::new(new));
    }
    if let Err(err) = vault_session::resolve_links(index) {
        eprintln!("jotter: could not resolve links: {err}");
    }
}

/// Rewrites the wikilinks that named a moved note, returning the notes changed.
///
/// Runs after [`reindex`], so the shortest target it can offer accounts for the
/// notes at their new paths.
pub fn relink(vault: &Vault, index: &Index, plan: &Plan) -> Vec<String> {
    let notes = index.all_notes().unwrap_or_default();
    let after = Resolver::new(notes.into_iter().map(|note| note.path));
    let mut rewritten = Vec::new();
    for linker in &plan.linkers {
        // A linker inside a moved folder is itself somewhere else by now.
        let path = plan.moved.get(linker).unwrap_or(linker);
        let Ok(text) = vault.read_note(Path::new(path)) else {
            continue;
        };
        let retarget = |written: &str| {
            let meant = plan.before.lookup(written)?;
            let lands = match plan.moved.get(&meant) {
                // The note it names moved, so the link follows it.
                Some(moved) => moved,
                // It did not move, so the link only needs pinning down when the
                // move handed its stem to somebody else.
                None if after.lookup(written).as_deref() != Some(meant.as_str()) => &meant,
                None => return None,
            };
            Some(new_target(written, lands, &after.shortest_target(lands)))
        };
        let Some(out) = rewrite_links(&text, &retarget) else {
            continue;
        };
        if let Err(err) = vault.write_note(Path::new(path), &out) {
            eprintln!("jotter: could not rewrite links in {path}: {err}");
            continue;
        }
        let _ = vault_session::reindex_note(vault, index, Path::new(path));
        rewritten.push(path.clone());
    }
    if !rewritten.is_empty()
        && let Err(err) = vault_session::resolve_links(index)
    {
        eprintln!("jotter: could not resolve links: {err}");
    }
    rewritten
}

#[cfg(test)]
mod tests {
    use super::{destination, new_target, rewrite_links};

    #[test]
    fn a_note_moves_into_a_folder() {
        assert_eq!(destination("plan.md", "work").as_deref(), Some("work/plan.md"));
    }

    #[test]
    fn a_note_moves_out_to_the_root() {
        assert_eq!(destination("work/plan.md", "").as_deref(), Some("plan.md"));
    }

    #[test]
    fn a_note_moves_between_folders() {
        assert_eq!(
            destination("work/plan.md", "archive/2024").as_deref(),
            Some("archive/2024/plan.md")
        );
    }

    #[test]
    fn a_folder_moves_with_its_name() {
        assert_eq!(destination("work", "archive").as_deref(), Some("archive/work"));
    }

    #[test]
    fn dropping_where_it_already_lives_does_nothing() {
        assert_eq!(destination("work/plan.md", "work"), None);
        assert_eq!(destination("plan.md", ""), None);
    }

    #[test]
    fn a_folder_cannot_land_on_itself() {
        assert_eq!(destination("work", "work"), None);
    }

    #[test]
    fn a_folder_cannot_land_inside_itself() {
        assert_eq!(destination("work", "work/deep"), None);
    }

    #[test]
    fn a_sibling_prefix_is_not_a_subtree() {
        assert_eq!(destination("work", "workshop").as_deref(), Some("workshop/work"));
    }

    #[test]
    fn the_root_cannot_be_moved() {
        assert_eq!(destination("", "work"), None);
    }

    #[test]
    fn a_path_form_link_stays_path_form() {
        assert_eq!(new_target("notes/plan", "work/plan.md", "plan"), "work/plan");
    }

    #[test]
    fn a_bare_link_takes_the_shortest_target() {
        assert_eq!(new_target("plan", "work/plan.md", "plan"), "plan");
        assert_eq!(new_target("plan", "work/plan.md", "work/plan"), "work/plan");
    }

    fn to_work(written: &str) -> Option<String> {
        (written == "plan").then(|| "work/plan".to_owned())
    }

    #[test]
    fn a_matching_link_is_rewritten() {
        assert_eq!(
            rewrite_links("see [[plan]] today", &to_work).as_deref(),
            Some("see [[work/plan]] today")
        );
    }

    #[test]
    fn a_rewrite_keeps_the_heading_and_the_alias() {
        assert_eq!(
            rewrite_links("[[plan#Next Steps|the plan]]", &to_work).as_deref(),
            Some("[[work/plan#Next Steps|the plan]]")
        );
    }

    #[test]
    fn every_occurrence_is_rewritten() {
        assert_eq!(
            rewrite_links("[[plan]] and [[plan]]", &to_work).as_deref(),
            Some("[[work/plan]] and [[work/plan]]")
        );
    }

    #[test]
    fn other_links_are_left_alone() {
        assert_eq!(rewrite_links("[[other]] and [[plan]]", &to_work).as_deref(),
            Some("[[other]] and [[work/plan]]"));
    }

    #[test]
    fn nothing_to_do_reports_no_change() {
        assert_eq!(rewrite_links("[[other]]", &to_work), None);
        assert_eq!(rewrite_links("no links here", &to_work), None);
    }

    #[test]
    fn a_target_that_would_not_change_is_not_a_change() {
        assert_eq!(rewrite_links("[[plan]]", &|t| Some(t.to_owned())), None);
    }

    #[test]
    fn a_link_inside_code_is_not_a_link() {
        let src = "```\n[[plan]]\n```\n";
        assert_eq!(rewrite_links(src, &to_work), None);
    }

    #[test]
    fn an_embed_is_not_rewritten() {
        assert_eq!(rewrite_links("![[plan]]", &to_work), None);
    }

    fn message(from: &str, to: &str, relinked: usize) -> String {
        super::message(std::path::Path::new(from), std::path::Path::new(to), relinked)
    }

    #[test]
    fn a_rename_reads_as_a_rename() {
        assert_eq!(message("a/plan.md", "a/roadmap.md", 0), "Renamed to roadmap.md");
    }

    #[test]
    fn a_move_names_the_folder_it_landed_in() {
        assert_eq!(message("plan.md", "a/b/plan.md", 0), "Moved plan.md to a/b");
    }

    #[test]
    fn a_move_to_the_root_says_so() {
        assert_eq!(message("a/plan.md", "plan.md", 0), "Moved plan.md to the vault root");
    }

    #[test]
    fn the_relink_count_is_appended_and_reads_singular_at_one() {
        assert_eq!(message("a/plan.md", "a/roadmap.md", 1), "Renamed to roadmap.md, relinked 1 note");
        assert_eq!(message("a/plan.md", "a/roadmap.md", 4), "Renamed to roadmap.md, relinked 4 notes");
    }

    mod over_a_vault {
        use std::path::Path;

        use jotter_index::Index;
        use jotter_vault::Vault;
        use tempfile::TempDir;

        use crate::vault_session;

        /// A vault holding `files`, fully indexed, with an index handle to match.
        fn vault(files: &[(&str, &str)]) -> (TempDir, Vault, Index) {
            let tmp = TempDir::new().unwrap();
            let root = tmp.path();
            for (path, text) in files {
                let abs = root.join(path);
                std::fs::create_dir_all(abs.parent().unwrap()).unwrap();
                std::fs::write(abs, text).unwrap();
            }
            vault_session::reconcile_in_thread(root, &|_| {}).unwrap();
            let index = vault_session::open_index(root).unwrap();
            let opened = Vault::open(root).unwrap();
            (tmp, opened, index)
        }

        /// Runs the whole move: plan, rename on disk, reindex, relink.
        fn move_it(vault: &Vault, index: &Index, from: &str, to: &str) -> Vec<String> {
            let (from, to) = (Path::new(from), Path::new(to));
            let plan = super::super::plan(vault, index, from, to);
            vault.rename_note(from, to).unwrap();
            super::super::reindex(vault, index, &plan);
            super::super::relink(vault, index, &plan)
        }

        fn text(vault: &Vault, path: &str) -> String {
            vault.read_note(Path::new(path)).unwrap()
        }

        #[test]
        fn a_bare_link_is_left_alone_by_a_change_of_folder() {
            let (_tmp, vault, index) = vault(&[
                ("plan.md", "# Plan\n"),
                ("index.md", "see [[plan]] and [[plan|the plan]]\n"),
            ]);

            let rewritten = move_it(&vault, &index, "plan.md", "work/plan.md");

            // A bare stem still resolves after a change of folder, so nothing moved.
            assert!(rewritten.is_empty());
            assert_eq!(text(&vault, "index.md"), "see [[plan]] and [[plan|the plan]]\n");
        }

        #[test]
        fn a_path_form_link_is_rewritten() {
            let (_tmp, vault, index) = vault(&[
                ("notes/plan.md", "# Plan\n"),
                ("index.md", "see [[notes/plan]]\n"),
            ]);

            let rewritten = move_it(&vault, &index, "notes/plan.md", "work/plan.md");

            assert_eq!(rewritten, ["index.md"]);
            assert_eq!(text(&vault, "index.md"), "see [[work/plan]]\n");
        }

        #[test]
        fn a_rename_mends_the_bare_links_it_would_have_broken() {
            let (_tmp, vault, index) = vault(&[
                ("plan.md", "# Plan\n"),
                ("index.md", "see [[plan#Today|today]]\n"),
            ]);

            let rewritten = move_it(&vault, &index, "plan.md", "roadmap.md");

            assert_eq!(rewritten, ["index.md"]);
            assert_eq!(text(&vault, "index.md"), "see [[roadmap#Today|today]]\n");
        }

        #[test]
        fn a_rename_into_a_stem_collision_falls_back_to_the_path() {
            let (_tmp, vault, index) = vault(&[
                ("a/roadmap.md", "# Other\n"),
                ("b/plan.md", "# Plan\n"),
                ("index.md", "see [[plan]]\n"),
            ]);

            move_it(&vault, &index, "b/plan.md", "b/roadmap.md");

            // Two notes named roadmap now, and a/roadmap wins the bare stem.
            assert_eq!(text(&vault, "index.md"), "see [[b/roadmap]]\n");
        }

        #[test]
        fn moving_a_folder_rewrites_the_links_naming_what_is_inside_it() {
            let (_tmp, vault, index) = vault(&[
                ("notes/plan.md", "# Plan\n"),
                ("notes/retro.md", "# Retro\n"),
                ("index.md", "see [[notes/plan]] and [[notes/retro]]\n"),
            ]);

            let rewritten = move_it(&vault, &index, "notes", "archive/notes");

            assert_eq!(rewritten, ["index.md"]);
            assert_eq!(
                text(&vault, "index.md"),
                "see [[archive/notes/plan]] and [[archive/notes/retro]]\n"
            );
        }

        #[test]
        fn a_link_inside_the_moved_folder_is_rewritten_at_its_new_home() {
            let (_tmp, vault, index) = vault(&[
                ("notes/plan.md", "# Plan\n"),
                ("notes/retro.md", "see [[notes/plan]]\n"),
            ]);

            let rewritten = move_it(&vault, &index, "notes", "archive");

            assert_eq!(rewritten, ["archive/retro.md"]);
            assert_eq!(text(&vault, "archive/retro.md"), "see [[archive/plan]]\n");
        }

        #[test]
        fn the_index_follows_the_move_and_the_links_table_with_it() {
            let (_tmp, vault, index) = vault(&[
                ("notes/plan.md", "# Plan\n"),
                ("index.md", "see [[notes/plan]]\n"),
            ]);

            move_it(&vault, &index, "notes/plan.md", "work/plan.md");

            assert!(index.note_by_path("notes/plan.md").unwrap().is_none());
            assert!(index.note_by_path("work/plan.md").unwrap().is_some());
            let linkers = index.linking_notes("work/plan.md").unwrap();
            assert_eq!(linkers.len(), 1);
            assert_eq!(linkers[0].path, "index.md");
        }

        #[test]
        fn a_move_that_steals_a_stem_pins_the_link_to_the_note_it_meant() {
            // [[plan]] means x/plan.md, the first of the two paths alphabetically.
            let (_tmp, vault, index) = vault(&[
                ("x/plan.md", "# X\n"),
                ("y/plan.md", "# Y\n"),
                ("index.md", "see [[plan]]\n"),
            ]);

            // Moving the other one to a/ hands it the stem, so the link has to
            // spell out the note it always meant even though that note stayed put.
            move_it(&vault, &index, "y/plan.md", "a/plan.md");

            assert_eq!(text(&vault, "index.md"), "see [[x/plan]]\n");
        }

        #[test]
        fn a_visit_to_the_root_leaves_a_path_form_link_bare() {
            let (_tmp, vault, index) = vault(&[
                ("notes/plan.md", "# Plan\n"),
                ("index.md", "see [[notes/plan]]\n"),
            ]);

            // At the root the shortest path form is the stem, so there is nothing
            // else the link could say.
            move_it(&vault, &index, "notes/plan.md", "plan.md");
            assert_eq!(text(&vault, "index.md"), "see [[plan]]\n");

            // Which means a later move into a folder finds a bare link, and a bare
            // link that still resolves is left alone: the folder is not re-added.
            let rewritten = move_it(&vault, &index, "plan.md", "work/plan.md");
            assert!(rewritten.is_empty());
            assert_eq!(text(&vault, "index.md"), "see [[plan]]\n");
        }

        #[test]
        fn a_link_in_code_survives_the_move_untouched() {
            let (_tmp, vault, index) = vault(&[
                ("notes/plan.md", "# Plan\n"),
                ("index.md", "`[[notes/plan]]` and [[notes/plan]]\n"),
            ]);

            move_it(&vault, &index, "notes/plan.md", "work/plan.md");

            assert_eq!(
                text(&vault, "index.md"),
                "`[[notes/plan]]` and [[work/plan]]\n"
            );
        }
    }
}

