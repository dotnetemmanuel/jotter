# TUI appearance: a second style beside classic

Date: 2026-07-31
Status: agreed, ready for an implementation plan

## The goal

jotter has one look: flat surfaces, small corners, thick structural lines, a
sans UI font. That look is now named **classic**. This adds a second one,
**TUI**, modelled on Cairn: a terminal aesthetic drawn in the same theme colors,
switched from a row in the settings window.

Nothing disappears. The rail, the sidebar, the status bar, the headerbar and the
logo all stay exactly where they are. What changes is how they are dressed.

## What TUI means here

Two halves, both required:

1. **The dress.** A different GTK stylesheet: square corners, hairline frames,
   a monospace UI font, flat inverted blocks in place of raised surfaces.
2. **The idioms.** The parts of a terminal look that are made of characters
   rather than styling: `▸`/`▾` folder markers, a `>` cursor on the current row,
   `[ Bracketed ]` buttons, section headers as a title with a rule running to the
   panel edge, bracketed status segments.

The dress alone reads as "classic with the roundness turned off", which is why
the idioms are in scope.

## Decisions

| Question | Decision |
|---|---|
| Structure color (frames, rules, panel titles) | The theme's `focus` token. `event-horizon` dark is already Cairn's scheme: focus `#26bbd9` cyan, accent `#ee64ac` pink, danger `#e95678` coral. |
| Selected row | Inverted block in `accent`, plus a `>` cursor. |
| UI font | Pinned to the theme's declared `editor_font` (CaskaydiaMono Nerd Font), ignoring the user's editor-font override, so the rail's Nerd Font glyphs always resolve. |
| UI font size | Unchanged behavior: the shared `font_size`, so Ctrl+scroll scales the chrome too. |
| Footer | The existing status bar, restyled. No key-hint line: `?` already opens the keysheet. |
| Headerbar | The otter stays at its current size. The bar's bottom line becomes a `focus` hairline. |
| Editor and preview | Untouched. The editor is already monospaced with a themed scheme, and the preview font stays as it is. |
| Switching | Fully live, like every other row in the settings window. |
| Theme files | No changes. Any future theme works in both styles for free. |

## Architecture

### crates/theming

- `model.rs` gains `pub enum Style { Classic, Tui }` with `as_str`, and the
  resolved `Theme` gains a `style: Style` field defaulting to `Classic`.
  `ThemeFile` is untouched: the style is a user choice, not a theme property.
- `generate/gtk_css.rs` keeps today's stylesheet, renamed internally to
  `classic_css`. `to_gtk_css` dispatches on `self.style`.
- `generate/tui_css.rs` is new and holds the TUI stylesheet, split into a chrome
  half and a parts half the same way classic is, so neither file is a wall of
  text.
- `to_preview_css` and `to_sourceview_scheme_xml` are not touched.

### crates/app

- `config::Appearance` gains `style: Option<String>` (`"classic"` | `"tui"`),
  absent meaning classic, matching the other optional fields.
- `appearance.rs` gains `style_of(&Appearance) -> Style` and
  `style_name(Style) -> &'static str`, and `resolve` stamps the style onto the
  theme it returns.
- `appearance::apply` sets `typography.ui_font` from the theme's declared
  `editor_font` when the style is TUI, **before** the user's `editor_font`
  override is applied. Order matters: applying the override first would leak the
  user's chosen editor font into the chrome.
- `lib.rs`: `apply_theme` gains one call that pushes the new style into the
  idiom-bearing widgets, placed after `*state.theme.borrow_mut() = next` so the
  widgets read the style that is now current.

Every existing repaint path (startup, `Ctrl+T`, a theme swap, a font-size
change) already funnels through `apply_theme`, so the style rides along with no
further plumbing.

## The dress

Applies throughout the TUI stylesheet:

- Corner radius forced to 0 and border width to 1, ignoring `chrome.radius` and
  `chrome.border_width`.
- Panels become framed regions rather than raised surfaces: a `focus` hairline
  down the sidebar's right edge, along the top of the status bar and the
  backlinks strip, and under the headerbar. The paned separator is the same
  hairline.
- Selected rows are square inverted blocks in `accent`, full-bleed to the panel
  edge rather than inset by a margin. Hover is a flat square `overlay` block.
- Buttons lose their frames: `focus` text, hover inverts the whole block.
- Entries lose the underline for a full 1px `focus` box.
- Popovers and the picker are boxes drawn over the background: `background`
  fill, 1px `focus` frame, no raised `surface`.
- Scrollbars go thin and square, `muted` on `overlay`.
- Tooltips square with a 1px `focus` frame.

## The idioms

- **Tree.** `TreeExpander::set_hide_expander(true)` in TUI, plus a leading gutter
  label per row rendered as cursor and marker: `▾` open folder, `▸` closed
  folder, blank for a file, prefixed with `>` when the row is selected. The
  cursor follows the `ListItem`'s own `selected` notify, so it stays correct
  without rebinding the whole list. In classic the gutter label is hidden and the
  expander is shown, so the two styles share one factory.
- **Section headers.** The vault name, the backlinks header, and the search, tags
  and git panel headings become an uppercase label in `focus` with a hairline
  rule running out to the panel edge.
- **Rail.** The same two Nerd Font glyphs, which are already terminal-native. The
  active marker moves from an underline under the glyph to a full-height `accent`
  bar down the left edge, and the column gains a right hairline.
- **Buttons.** Buttons whose face is a label render it as `[ Label ]`: the
  Light/Dark and Classic/TUI toggles, the conflict actions, the size steps.
  Icon-only buttons (the settings close, the back arrow, the status items) keep
  their glyph, and the theme buttons keep their color swatch.
- **Status bar.** Mono, a `focus` hairline along the top, items joined with
  ` · `, and the right-hand items as bracketed segments: `[ 15px ]`, the git
  segment with its existing glyphs, broken links as `[ ! 3 ]` in `danger`.
- **Picker, palette, switcher.** A square `focus` frame, a
  `─ Command palette ─────` title line, a `>` prompt label ahead of the filter
  entry, and the cursor plus inverted block on the selected row. Search results,
  tag rows and backlinks rows get the same cursor treatment.
- **Completion popup.** Square frame and inverted selection, no content change.
- **Conflict view.** The tinted incoming/yours/resolution blocks become square
  hairline-framed regions with `─ INCOMING ───` headers.
- **Keysheet and settings window.** The dress, plus the bracketed buttons. No
  other content change.

Each idiom-bearing widget exposes a `set_style(Style)` that is idempotent and
safe to call with the style it already has, since `apply_theme` runs on every
repaint, not only on a style change.

## The settings row

A new first row in the settings grid, above Theme: **Style**, with a grouped
`Classic` / `TUI` toggle pair built exactly like the existing Light/Dark pair. It
reports through a new `settings::Change::Style` and saves to config on the spot,
like every other row in that window. First because it is the widest-reaching of
the three visual axes; named Style rather than Appearance because every row in
that window is appearance.

The settings window itself restyles live along with the main window, since it
takes the same display-level provider.

## Testing

Unit tests:

- `crates/theming`: the TUI stylesheet carries no rounded corners, draws its
  frames from `focus`, and sets the mono UI font. The existing generator tests
  keep guarding the classic output, which must not change.
- `crates/app/src/appearance.rs`: the style round-trips through the config; an
  unknown style falls back to classic rather than failing; TUI pins `ui_font` to
  the theme's own editor font even when the user has overridden the editor font;
  classic leaves `ui_font` alone.

Manual pass, in the headless cage as usual, never on the desktop: both styles
across both themes and both modes, screenshots of each, plus a live flip with the
tree expanded, a note open, the palette up and the settings window open. The
commit waits for sign-off on those screenshots.

## Out of scope

- Any change to the rendered preview or the source editor.
- A key-hint footer line.
- Per-theme TUI color overrides. If a future theme wants different structure
  colors it can move its `focus` token.
- A keyboard shortcut or palette command for the switch. It lives in settings
  only until there is a reason for more.
