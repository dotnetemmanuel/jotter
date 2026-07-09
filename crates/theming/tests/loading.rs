//! Loading and resolution error paths.

use jotter_theming::{Mode, ThemeError, ThemeFile};

fn retro82() -> ThemeFile {
    let src = jotter_theming::bundled::BUNDLED
        .iter()
        .find(|b| b.id == "retro82")
        .expect("retro82 is bundled")
        .source;
    ThemeFile::from_jsonc(src).expect("bundled retro82 parses")
}

#[test]
fn unknown_top_level_field_is_ignored() {
    let src = jotter_theming::bundled::BUNDLED[0].source.replacen(
        "\"id\": \"retro82\",",
        "\"id\": \"retro82\",\n  \"future_field\": 42,",
        1,
    );
    assert!(ThemeFile::from_jsonc(&src).is_ok());
}

#[test]
fn undefined_ref_names_field_path() {
    let mut f = retro82();
    f.chrome.background = "$does_not_exist".into();
    let err = f.resolve(Mode::Dark).unwrap_err();
    let ThemeError::UndefinedRef { field, token } = err else {
        panic!("expected UndefinedRef, got {err:?}");
    };
    assert_eq!(field, "chrome.background");
    assert_eq!(token, "does_not_exist");
}

#[test]
fn palette_reference_is_rejected() {
    let mut f = retro82();
    f.palette.dark.insert("base".into(), "$surface".into());
    let err = f.resolve(Mode::Dark).unwrap_err();
    assert!(
        matches!(err, ThemeError::PaletteRefNotAllowed { .. }),
        "got {err:?}"
    );
}

#[test]
fn invalid_palette_color_is_rejected() {
    let mut f = retro82();
    f.palette.dark.insert("base".into(), "not-a-color".into());
    let err = f.resolve(Mode::Dark).unwrap_err();
    assert!(
        matches!(err, ThemeError::InvalidPaletteColor { .. }),
        "got {err:?}"
    );
}

#[test]
fn both_modes_resolve_for_every_bundled_theme() {
    for b in jotter_theming::bundled::BUNDLED {
        let f = ThemeFile::from_jsonc(b.source).expect("bundled theme parses");
        f.resolve(Mode::Dark).expect("dark resolves");
        f.resolve(Mode::Light).expect("light resolves");
    }
}
