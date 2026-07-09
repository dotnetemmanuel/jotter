//! JSONC parsing: strip `//` line and `/* */` block comments, then deserialize.
//!
//! Comment stripping is string-aware, so a `//` or `/*` inside a JSON string
//! value is left untouched.

use crate::error::ThemeError;
use crate::model::ThemeFile;

impl ThemeFile {
    /// Parse a theme from JSONC source (JSON with `//` and `/* */` comments).
    ///
    /// # Errors
    /// Returns [`ThemeError::Parse`] when the stripped source is not valid JSON
    /// or does not match the theme schema.
    pub fn from_jsonc(source: &str) -> Result<Self, ThemeError> {
        let stripped = strip_comments(source);
        Ok(serde_json::from_str(&stripped)?)
    }
}

/// Remove `//` line comments and `/* */` block comments, preserving any that
/// appear inside string literals.
fn strip_comments(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    let mut in_string = false;
    let mut escaped = false;

    while let Some(c) = chars.next() {
        if in_string {
            out.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }

        match c {
            '"' => {
                in_string = true;
                out.push(c);
            }
            '/' if chars.peek() == Some(&'/') => {
                while let Some(&n) = chars.peek() {
                    if n == '\n' {
                        break;
                    }
                    chars.next();
                }
            }
            '/' if chars.peek() == Some(&'*') => {
                chars.next();
                let mut prev = '\0';
                for n in chars.by_ref() {
                    if prev == '*' && n == '/' {
                        break;
                    }
                    prev = n;
                }
            }
            _ => out.push(c),
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::strip_comments;

    #[test]
    fn strips_line_and_block_comments() {
        let src = "{ // head\n  \"a\": 1, /* mid */ \"b\": 2 }";
        assert_eq!(
            strip_comments(src).replace(['\n', ' '], ""),
            "{\"a\":1,\"b\":2}"
        );
    }

    #[test]
    fn keeps_comment_markers_inside_strings() {
        let src = r#"{ "url": "http://x.io", "p": "a/*b*/c" }"#;
        let out = strip_comments(src);
        assert!(out.contains("http://x.io"));
        assert!(out.contains("a/*b*/c"));
    }
}
