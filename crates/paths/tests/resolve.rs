use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};

use jotter_paths::resolve;

#[test]
fn an_override_wins_over_the_base_directory() {
    let got = resolve(Some(OsStr::new("/override")), Path::new("/base"));
    assert_eq!(got, Path::new("/override"));
}

#[test]
fn no_override_appends_the_app_directory_to_the_base() {
    let got = resolve(None, Path::new("/base"));
    assert_eq!(got, Path::new("/base/jotter"));
}

#[test]
fn an_empty_override_is_ignored() {
    let got = resolve(Some(OsStr::new("")), Path::new("/base"));
    assert_eq!(got, resolve(None, Path::new("/base")));
}

#[test]
fn the_config_and_data_lookups_resolve_independently() {
    let config = resolve(
        Some(OsStr::new("/config-override")),
        Path::new("/config-base"),
    );
    let data = resolve(None, Path::new("/data-base"));
    assert_eq!(config, Path::new("/config-override"));
    assert_eq!(data, Path::new("/data-base/jotter"));
}

#[cfg(unix)]
fn non_utf8_os_string() -> OsString {
    use std::os::unix::ffi::OsStringExt;
    OsString::from_vec(vec![0xff, 0xfe])
}

#[cfg(windows)]
fn non_utf8_os_string() -> OsString {
    use std::os::windows::ffi::OsStringExt;
    // An unpaired surrogate: valid as a Windows environment variable value, invalid as UTF-8 or UTF-16.
    OsString::from_wide(&[0xd800])
}

#[cfg(any(unix, windows))]
#[test]
fn a_non_utf8_override_is_used_verbatim_not_treated_as_absent() {
    let invalid = non_utf8_os_string();
    let got = resolve(Some(&invalid), Path::new("/base"));
    assert_eq!(got, PathBuf::from(&invalid));
}
