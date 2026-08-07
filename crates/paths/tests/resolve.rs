use std::path::Path;

use jotter_paths::resolve;

#[test]
fn an_override_wins_over_the_base_directory() {
    let got = resolve(Some("/override"), Path::new("/base"));
    assert_eq!(got, Path::new("/override"));
}

#[test]
fn no_override_appends_the_app_directory_to_the_base() {
    let got = resolve(None, Path::new("/base"));
    assert_eq!(got, Path::new("/base/jotter"));
}

#[test]
fn an_empty_override_is_ignored() {
    let got = resolve(Some(""), Path::new("/base"));
    assert_eq!(got, resolve(None, Path::new("/base")));
}

#[test]
fn the_config_and_data_lookups_resolve_independently() {
    let config = resolve(Some("/config-override"), Path::new("/config-base"));
    let data = resolve(None, Path::new("/data-base"));
    assert_eq!(config, Path::new("/config-override"));
    assert_eq!(data, Path::new("/data-base/jotter"));
}
