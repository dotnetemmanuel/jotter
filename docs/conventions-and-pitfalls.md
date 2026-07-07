# jotter: conventions and pitfalls

Companion to `implementation-plan.md` and `architecture.md`. This is the guardrail
sheet. Read it before writing code in a new phase. It encodes the coding standards,
the writing rules, the git conventions, and the traps that are known in advance so
we do not step on them twice.

## Toolchain and build

- Rust: latest stable, edition 2024. Target 1.96.1 or newer.
- Manage the toolchain with `rustup` (official Arch extra package, not the AUR).
- Pin with a `rust-toolchain.toml` at the workspace root:
  ```toml
  [toolchain]
  channel = "stable"
  components = ["rustfmt", "clippy"]
  ```
- Every crate manifest sets `edition = "2024"`. Workspace root sets `resolver = "3"`.
- `Cargo.lock` is committed. jotter is an application, so dependency versions are
  pinned for reproducible builds.
- Pin exact dependency versions at first `cargo add`. Do not float majors.

## Coding standards

- Warnings are errors in CI. Add `#![warn(clippy::pedantic)]` at each crate root and
  allow-list only the lints that are genuine noise, with a one-line reason.
- `rustfmt` with defaults, run on save.
- Public APIs carry `///` doc comments. `cargo doc` should read cleanly.
- No panics in library code. Return `Result` everywhere. `unwrap` and `expect` only
  in tests or at a truly infallible spot, and only with a comment saying why.
- `crates/*` use `thiserror` for typed errors. `app` and the binary use `anyhow` at
  the boundary.
- Async only where it earns its keep: git network IO and file watching. The editor
  and the index are synchronous.
- Feature flags stay minimal. If a feature exists, it compiles by default.
- Comments explain the why, not the what.

## Writing rules (apply to code, comments, docs, commits, UI strings)

- No em dashes anywhere. Use a comma, a colon, parentheses, or rephrase.
- Keep comments to one line. Spill to a second line only when one genuinely cannot
  carry the meaning.
- Honest naming. No marketing language in code or UI. Do not use words like
  seamlessly, leverages, or empowers.

## Git conventions

- Commit messages: subject in the imperative, optional body. No single quote
  (apostrophe) character anywhere in the message. Use double quotes when quoting.
  Prefer a heredoc or a `-F` message file so the body never contains a single quote.
- Never append a Co-Authored-By trailer or any generated-by attribution. Commits and
  PRs carry only the human author.
- Suggested commit cadence follows the phase acceptance blocks in the plan, one clean
  commit per completed phase (or per completed sub-unit within a long phase).

## Non-goals for v1 (do not build these, leave seams only)

- No plugin system. Route every user action through the command dispatcher so a
  plugin API can be extracted later, but do not build the API now.
- No graph view. Backlinks are enough for v1.
- No mobile companion, no sync service, no server component.
- No live-preview WYSIWYG. Toggle between edit and preview is the v1 model.
- No split-pane view. Single pane: either editing or previewing, never both.
- No canvas, whiteboard, or spatial views.

## Known pitfalls (each has a rule attached)

### WebKitGTK 6.0 is a hard, independently-versioned dependency
It updates on its own cadence, separate from GTK4. Pin the minimum in the manifest
and document `pacman -S webkitgtk-6.0` in the README.

### Wayland fractional scaling makes WebKit preview text blurry
At 1.25x the preview can render fuzzy. Set the WebView `zoom-level` proportional to
the monitor scale factor. Test on both displays: 1.2x internal and 1.25x external.

### libgit2 SSH depends on libssh2 at build time
The `ssh` feature of `git2` needs `libssh2`. If SSH auth misbehaves on Arch, fall
back to shelling out to the `git` binary via `std::process::Command` for network
ops. Use the same fallback for partial clone, sparse checkout, and awkward rebases.

### notify hits the inotify per-user watch limit on large vaults
inotify has a per-user cap. Large vaults will exceed it. Document the
`fs.inotify.max_user_watches` sysctl bump in the README. Use the debounced watcher
(`notify-debouncer-full`) and apply the ignore list before watching.

### FTS5 rebuild is expensive on first open of a large vault
Indexing can take a while. Do it on a background thread with progress in the status
bar. Never block the UI thread. The UI must be interactive before indexing finishes.

### SourceView undo history fragments on programmatic buffer edits
Any programmatic change to the buffer (auto-formatting, wikilink rewrite helpers,
template insertion) must be wrapped in `buffer.begin_user_action()` and
`buffer.end_user_action()` or the undo history splinters.

### comrak has no wikilinks, and a blind regex corrupts code
Wikilink preprocessing must be code-context aware. Never rewrite `[[target]]` inside
a fenced code block or inline code. Scan with awareness of code spans, not a single
document-wide regex.

## Performance targets

- Cold open in under one second on the target machine.
- Edit-preview toggle feels instant (stack page transition, no resize, no visible
  reflow beyond the parse).
- Reindex on external change is incremental, only the changed files.

## Testing

- `cargo test --workspace` for unit and integration tests.
- `insta` snapshot tests for the markdown-to-HTML pipeline and the three theming
  generator outputs (GTK CSS, SourceView scheme XML, preview CSS).
- UI tests are deferred. Cover logic crates (theming, parser, index, vault) well
  since they hold the risk and are testable without a display.
