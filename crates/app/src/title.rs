//! Pure note-title extraction: frontmatter `title`, else first H1, else filename stem.

use std::path::Path;

/// Derives a display title from note text and its vault-relative path.
///
/// Resolution order: a `title` in the frontmatter block, else the first `# H1`
/// line, else the filename stem. Always returns something usable.
#[must_use]
pub fn extract_title(text: &str, rel_path: &Path) -> String {
    if let Some(title) = jotter_parser::frontmatter::parse(text).title {
        return title;
    }
    if let Some(h1) = first_h1(text) {
        return h1;
    }
    stem(rel_path)
}

/// The filename stem (no extension), or the last component, or a fallback.
fn stem(rel_path: &Path) -> String {
    rel_path
        .file_stem()
        .or_else(|| rel_path.file_name())
        .and_then(|s| s.to_str())
        .map_or_else(|| "untitled".to_owned(), str::to_owned)
}

/// The text of the first ATX `# ` heading, trimmed, if any.
fn first_h1(text: &str) -> Option<String> {
    for line in text.lines() {
        if let Some(rest) = line.trim_start().strip_prefix("# ") {
            let title = rest.trim();
            if !title.is_empty() {
                return Some(title.to_owned());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::extract_title;
    use std::path::Path;

    #[test]
    fn frontmatter_title_wins() {
        let text = "---\ntitle: My Note\ntags: [a]\n---\n# Heading\nbody";
        assert_eq!(extract_title(text, Path::new("x.md")), "My Note");
    }

    #[test]
    fn frontmatter_title_is_unquoted() {
        let text = "---\ntitle: \"Quoted Title\"\n---\n";
        assert_eq!(extract_title(text, Path::new("x.md")), "Quoted Title");
        let single = "---\ntitle: 'Single'\n---\n";
        assert_eq!(extract_title(single, Path::new("x.md")), "Single");
    }

    #[test]
    fn h1_used_when_no_frontmatter_title() {
        let text = "# The Heading\n\nbody text";
        assert_eq!(extract_title(text, Path::new("x.md")), "The Heading");
    }

    #[test]
    fn h1_used_when_frontmatter_has_no_title() {
        let text = "---\ntags: [a, b]\n---\n# Real Heading\n";
        assert_eq!(extract_title(text, Path::new("x.md")), "Real Heading");
    }

    #[test]
    fn filename_stem_is_last_resort() {
        let text = "no heading here, just prose";
        assert_eq!(
            extract_title(text, Path::new("sub/My File.md")),
            "My File"
        );
    }

    #[test]
    fn empty_frontmatter_title_falls_through() {
        let text = "---\ntitle:\n---\n# Fallback\n";
        assert_eq!(extract_title(text, Path::new("x.md")), "Fallback");
    }

    #[test]
    fn hash_without_space_is_not_h1() {
        let text = "#nospace\nbody";
        assert_eq!(extract_title(text, Path::new("note.md")), "note");
    }
}
