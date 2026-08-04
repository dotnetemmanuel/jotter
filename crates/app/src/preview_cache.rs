//! What the preview web view currently holds, and whether it can be reused.
//!
//! The text alone does not identify a rendered page: the same markdown renders
//! differently after a theme switch, after a wikilink starts resolving, or after
//! the open note moves and relative image paths point somewhere else. A
//! generation counter carries all of that, so the page is reusable only while
//! both the text and the generation still match.

use jotter_parser::HeadingAnchor;

/// The source the page the web view holds was rendered from.
pub(crate) struct LoadedPage {
    /// Editor text the page was rendered from.
    pub text: String,
    /// Render generation the page was rendered at.
    pub generation: u64,
    /// Heading anchors of that page, for scrolling without re-rendering.
    pub headings: Vec<HeadingAnchor>,
}

/// Whether `loaded` is exactly what rendering `text` at `generation` would give.
pub(crate) fn is_current(loaded: Option<&LoadedPage>, text: &str, generation: u64) -> bool {
    loaded.is_some_and(|page| page.generation == generation && page.text == text)
}

/// The heading anchors of a page worth reusing, or `None` when it must be
/// rendered again.
pub(crate) fn reusable<'a>(
    loaded: Option<&'a LoadedPage>,
    text: &str,
    generation: u64,
) -> Option<&'a [HeadingAnchor]> {
    loaded
        .filter(|page| is_current(Some(page), text, generation))
        .map(|page| page.headings.as_slice())
}

#[cfg(test)]
mod tests {
    use super::{HeadingAnchor, LoadedPage, is_current};

    fn page(text: &str, generation: u64) -> LoadedPage {
        LoadedPage { text: text.to_owned(), generation, headings: Vec::new() }
    }

    #[test]
    fn nothing_loaded_is_never_current() {
        assert!(!is_current(None, "# hi", 0));
    }

    #[test]
    fn the_same_text_at_the_same_generation_is_current() {
        let loaded = page("# hi", 3);
        assert!(is_current(Some(&loaded), "# hi", 3));
    }

    #[test]
    fn edited_text_is_not_current() {
        let loaded = page("# hi", 3);
        assert!(!is_current(Some(&loaded), "# hi there", 3));
    }

    #[test]
    fn a_bumped_generation_is_not_current() {
        // A theme switch renders the very same text differently.
        let loaded = page("# hi", 3);
        assert!(!is_current(Some(&loaded), "# hi", 4));
    }

    #[test]
    fn an_empty_document_is_still_a_loaded_page() {
        let loaded = page("", 7);
        assert!(is_current(Some(&loaded), "", 7));
    }

    #[test]
    fn only_a_current_page_hands_back_its_headings() {
        let mut loaded = page("# hi", 3);
        loaded.headings.push(HeadingAnchor {
            source_line: 1,
            anchor: "hi".to_owned(),
            level: 1,
        });
        assert_eq!(super::reusable(Some(&loaded), "# hi", 3).map(<[_]>::len), Some(1));
        assert!(super::reusable(Some(&loaded), "# hi", 4).is_none());
        assert!(super::reusable(Some(&loaded), "# bye", 3).is_none());
        assert!(super::reusable(None, "# hi", 3).is_none());
    }
}
