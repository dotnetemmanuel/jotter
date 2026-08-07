//! The fonts installed on this machine, as the settings window offers them.
//!
//! Families come from pango, which is what actually renders the text, so a name
//! offered here is a name that will work.

use gtk::prelude::*;

/// Every font family installed, sorted, optionally only the monospaced ones.
///
/// The editor list is monospaced only: a proportional font in a markdown buffer
/// misaligns every table and code block, and is a mistake worth having to work
/// for.
#[must_use]
pub fn families(monospace_only: bool) -> Vec<String> {
    let Some(map) = pango_map() else {
        return Vec::new();
    };
    let mut names: Vec<String> = map
        .list_families()
        .into_iter()
        .filter(|family| !monospace_only || family.is_monospace())
        .map(|family| family.name().to_string())
        .collect();
    names.sort_by_key(|name| name.to_lowercase());
    names.dedup();
    names
}

/// The font map GTK renders with, taken from a throwaway widget's context so it
/// is the same one the app draws with. `None` without a display, as in tests.
fn pango_map() -> Option<gtk::pango::FontMap> {
    gtk::gdk::Display::default()?;
    gtk::Label::new(None).pango_context().font_map()
}

/// The families matching `query`, case-insensitively, in their existing order.
///
/// An empty query keeps everything, so the list starts as the whole set rather
/// than empty.
#[must_use]
pub fn matching(families: &[String], query: &str) -> Vec<String> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return families.to_vec();
    }
    families
        .iter()
        .filter(|name| name.to_lowercase().contains(&query))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::matching;

    fn installed() -> Vec<String> {
        ["CaskaydiaMono Nerd Font", "DejaVu Sans", "Inter", "Iosevka Term"]
            .into_iter()
            .map(str::to_string)
            .collect()
    }

    #[test]
    fn an_empty_query_offers_everything() {
        assert_eq!(matching(&installed(), "").len(), 4);
        assert_eq!(matching(&installed(), "   ").len(), 4);
    }

    #[test]
    fn a_query_matches_anywhere_in_the_name() {
        assert_eq!(matching(&installed(), "mono"), ["CaskaydiaMono Nerd Font"]);
        assert_eq!(matching(&installed(), "sans"), ["DejaVu Sans"]);
    }

    #[test]
    fn matching_ignores_case() {
        assert_eq!(matching(&installed(), "IOSEVKA"), ["Iosevka Term"]);
    }

    #[test]
    fn a_query_matching_nothing_offers_nothing() {
        assert!(matching(&installed(), "comic sans").is_empty());
    }
}
