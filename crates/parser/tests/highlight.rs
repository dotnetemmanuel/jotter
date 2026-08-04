//! Golden output for the two syntax highlighting paths, editor and preview.
//!
//! Both are driven by the same syntect tokenization, so anything that changes
//! how that tokenization is produced or reused has to leave these byte for byte
//! identical.

use jotter_parser::{codeblock, render};
use jotter_theming::Code;

const FIXTURE: &str = "\
# Highlighting

```rust
// a comment
fn main() {
    let greeting = \"hello\";
    println!(\"{greeting} {}\", 42);
}
```

```python
# a comment
def greet(name):
    return f\"hi {name}\"
```

```csharp
public async Task<int> GoAsync() {
    var x = 1 + 2;
    return await Task.FromResult(x);
}
```

```kotlin
// a comment
fun greet(name: String) = \"hi $name\"
```

```nosuchlang
this stays flat
```

```
no language at all
```
";

fn dark() -> Code {
    Code {
        background: "#1e1e1e".into(),
        foreground: "#d4d4d4".into(),
        keyword: "#c586c0".into(),
        string: "#ce9178".into(),
        comment: "#6a9955".into(),
        function: "#dcdcaa".into(),
        number: "#b5cea8".into(),
        type_name: "#4ec9b0".into(),
        variable: "#9cdcfe".into(),
    }
}

fn light() -> Code {
    Code {
        background: "#ffffff".into(),
        foreground: "#24292e".into(),
        keyword: "#d73a49".into(),
        string: "#032f62".into(),
        comment: "#6a737d".into(),
        function: "#6f42c1".into(),
        number: "#005cc5".into(),
        type_name: "#22863a".into(),
        variable: "#e36209".into(),
    }
}

fn no_links(_: &str) -> Option<String> {
    None
}

/// The spans as text, so a snapshot diff names the code that moved color.
fn spans_as_text(code: &Code) -> String {
    codeblock::color_spans(FIXTURE, code)
        .iter()
        .map(|span| format!("{} {:?}\n", span.color, &FIXTURE[span.range.clone()]))
        .collect()
}

#[test]
fn editor_spans_match_snapshot() {
    insta::assert_snapshot!("editor_spans_dark", spans_as_text(&dark()));
    insta::assert_snapshot!("editor_spans_light", spans_as_text(&light()));
}

#[test]
fn preview_html_matches_snapshot() {
    let html = render(FIXTURE, &dark(), &no_links, &jotter_parser::NoImages).html;
    insta::assert_snapshot!("preview_html_dark", html);
    let html = render(FIXTURE, &light(), &no_links, &jotter_parser::NoImages).html;
    insta::assert_snapshot!("preview_html_light", html);
}

/// The same block, colored with one palette then the other, must take the
/// palette it was given each time rather than the one it was first seen with.
#[test]
fn a_second_palette_recolors_the_same_text() {
    let first = spans_as_text(&dark());
    let second = spans_as_text(&light());
    let first_again = spans_as_text(&dark());

    assert_ne!(first, second);
    assert_eq!(first, first_again);
    assert!(second.contains("#6a737d"), "light comment color missing");
    assert!(!second.contains("#6a9955"), "dark comment color leaked into light");
}

/// The editor path and the preview path agree on which blocks get color.
#[test]
fn both_paths_color_the_same_languages() {
    let html = render(FIXTURE, &dark(), &no_links, &jotter_parser::NoImages).html;
    let spans = codeblock::color_spans(FIXTURE, &dark());
    for color in ["#6a9955", "#ce9178", "#4ec9b0"] {
        assert!(html.contains(color), "{color} missing from preview");
        assert!(spans.iter().any(|span| span.color == color), "{color} missing from editor");
    }
}
