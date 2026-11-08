# The UI kit: Horizon's own overlays

Horizon draws things vanilla doesn't have over the game: the controller's gameplay guide, glyphs on
declared buttons, controller notices, splitscreen's prompts and the on-screen keyboard. The UI kit is
the shared code they draw with, so a fix in one place reaches all of them. It lives in
`horizon-client-render`, which never reaches gameplay. The game hands it what it knows (the player's
settings and glyph style), and the kit only draws.

The kit grows a piece at a time, and each piece adds its section here.

| Piece | Where | For |
|---|---|---|
| Glyphs | `crates/horizon-client-render/src/pad_glyph.rs`, `MenuJob` and `GlyphOf` in `crates/horizon-client-input/src/gamepad.rs` | a controller button, asked for by what it does |
| Settings shell | `crates/horizon-client-render/src/settings_shell.rs` | a searchable settings screen: Extras, Graphics, Controller Settings |

## What never changes

Vanilla's screens and display lists are the specification, and the kit never touches them:
`hud_layout::GuiDraw`, `screen_draw::Draw`, the gamemode switcher, the spectator menu and the toasts'
`Slide`. Their parity harnesses (hud, screen_layout, chat_screen) pin them. Use the kit only for what
Horizon adds, and keep it off by default where it would change the vanilla look
([design-system.md](design-system.md)).

## Glyphs

Code asks for a glyph by what the button does, never by a button. The chain runs from the job to the art:

1. **What it does** (`gamepad::GlyphOf`): a gameplay action, `GlyphOf::Bound("key.jump")`, or a menu's fixed
   job, `GlyphOf::Menu(MenuJob::Back)`. Menus aren't rebindable, so `MenuJob::button` is the one map from a
   menu job to its button.
2. **The button**: the player's binding for an action (`PadSettings::binding`), or the menu map. An unbound
   action has no glyph.
3. **The style**: the player's resolved glyph style, `ui.options.pad_style`, set once a frame by
   `PadSettings::glyph_style`. That is the player's choice, else the pad's family, and
   `GlyphStyle::NintendoLetters` on a Nintendo pad going by its letters.
4. **The art**: `pad_glyph::Glyph` holds the button and the style. `draw`, `width`, `rect` and `art` take
   only a `Glyph`, and `Glyph::label` and `Glyph::color` give its text.

**Letter mode is part of the resolved glyph style; callers never handle it.** The game already reads a pad
going by its letters as parts (`PadState::by_letter`): the A on a Switch pad's right is South, which
confirms and jumps. `GlyphStyle::on_pad` turns a part back into the button that plays it for the art and
the label, so South shows that A. `NintendoLetters` is never a setting: the Glyphs choice and
`horizon-controller.txt` keep `nintendo` and `nintendoLetters` as before.

A new screen asks for a glyph like this:

```rust
// Gameplay: `Game::glyph` for anything, `Game::menu_glyph` for a menu job.
if let Some(jump) = self.glyph(GlyphOf::Bound("key.jump")) {
    pad_glyph::draw(batch, atlas, jump, x, y);
}
let back = self.menu_glyph(MenuJob::Back);

// In the render crate, with the style the game passed in (`ui.options.pad_style`):
let keys = [MenuJob::PagePrev, MenuJob::PageNext].map(|job| Glyph::menu(job, style));
```

There is no way to turn a bare `Button` into art or a label outside `pad_glyph`. The one exception is
`Glyph::unheld`: splitscreen's join and lost-controller prompts are answered by a controller nobody holds
yet, whose letter mode isn't known, so they show the buttons by their places
([controllers.md](controllers.md)). A test (`only_the_documented_exception_draws_by_place`) fails if
another file uses it.

## Moving a screen onto the shell

A settings screen on the shell describes its pages, groups and rows, and says what a press does. The
shell (`settings_shell::Shell`) does the rest: drawing, the mouse, one scroll path, the keyboard's focus
and the controller's targets, the search, the strip that describes a row, Undo, and a binding's wait. A
fix there reaches every screen on it. `extras_ui.rs` is the smallest example, `graphics_ui.rs` has
sliders, costs and greyed rows, and `controller_ui.rs` has bindings and lines to read.

1. **Name the rows and the pages.** `type Row` is an id for a row (an enum over the settings), `type
   Page` one for a page (`()` for a screen of one page, which then shows no page list).
2. **Describe them** in `Screen::sheets`, afresh from the settings: a `Sheet` per page, its `Group`s,
   and a `Row` each, with its `Value`: `Switch`, `Choice` (stepped by a press, Shift back, and Left and
   Right), `Slider`, `Binding`, `Read` (a value the focus reaches, which nothing presses), `Press` or
   `Note`. A row says what it does (`about`), what its chosen value does (`chosen`), and why it is greyed
   (`blocked`); the strip shows them and the search finds them.
3. **Say what a press does** in `press`, `slide`, `wheel_step` and, for a binding row's reset button,
   `reset`, as `Output`s: an `Event` for the game, `Push` another page, `Done`. The shell has already
   played the click. `side` gives lines to show under the page list, such as whose settings these are
   (Controller Settings: "Editing: <player>" and the controller the game sees); they are read-only.
4. **Fingerprint what the rows read** in `stamp` (`stamp_of`), besides the presses: the rows are built
   again only when it changes.
5. **Undo**, if the screen has it: `undoes`, and `undo` puts back what the screen changes from the
   settings it opened with. `differs` lets Undo show exactly while something differs, a binding the game
   made meanwhile included.
6. **Host it**: a `Shell` of it on `OptionsUi`, named in `OptionsUi::shell` and `shell_mut`, and opened
   at its page with `Shell::show`. Every input goes through those two, so a screen can't take the wheel
   and miss the stick.
7. **Delete** the screen's own drawing, scrolling, focus, search and click code.

What the old screen's tests checked moves to tests on the `Shell` of the new one: press a row with
`click` at `point_at`, walk with `key`, and read `listed`, `row_labels`, `focused` and `changed`.

### Binding rows

`Value::Binding` is a row that binds a key or a controller button. The screen says what it binds and
what it is bound to now; the shell starts the wait when it is pressed, marks the row ("> A <") while the
host waits, draws the controller button's art (`pad_glyph::Glyph`), says in the strip that it is unbound
or what else its button does, and draws the row's reset.

- `Capture::Key(name)`: a key or a mouse button, as on Key Binds. The host waits (`selected_key`), and
  Escape unbinds.
- `Capture::Pad(action)`: a controller button. The host waits (`selected_pad_action`); the game binds the
  next button pressed, Start cancels, a click or another key cancels, and the keyboard's Escape unbinds.
- `shared`: what else the same button does, which the strip lists and the row marks "[ ]" in yellow.
- `reset`: `Reset::None` (no button), `Reset::Default` (greyed) or `Reset::Changed` (live) for a reset
  button at the row's end; pressed (a click, or Right then Enter or A), it calls `Screen::reset`.

A press gives `Output::Bind(capture)`; the host's `shell_events` starts the wait, and passes what it waits
on back to `Hosted::draw`.

```rust
// A key, as Extras' sort key and Graphics' HDR shortcut have it:
Row::new(row, name, Value::Binding(Binding::key(extras::SORT_KEY, key_name)))

// A controller button, as Controller Settings > Bindings has each action:
let shown = Shown::Pad {
    glyph: Glyph::of(GlyphOf::Bound(action), &s.controller, style),
    cancel: Glyph::menu(MenuJob::Start, style),
};
let binding = Binding {
    capture: Capture::Pad(action),
    shown,
    shared, // the other actions on its button, by name
    reset: if s.controller.is_default(action) { Reset::Default } else { Reset::Changed },
};
Row::new(Item::Bind(action), name, Value::Binding(binding))
```

## Testing

Test what is drawn, not which function was called.

- A glyph test builds the test atlas (`HudAtlas::build` on a missing folder gives Horizon's own art),
  draws into a `HudBatch`, and compares the quads or texture coordinates with the sprite expected.
- Letter mode is checked against input: `pad_glyph`'s test presses each physical button, reads it the way
  the game does (`PadState::by_letter`), and expects that part's glyph to be the pressed button's print.
- In the e2e harness, `horizon-ctl pad family nintendo` makes the pad playing a Nintendo one, and
  `pad connection connected nintendo` raises a Nintendo pad's notice. With `nintendoLetters:true` in
  the seeded `horizon-controller.txt`, that is letter mode with no real pad (`tools/pad_menu_e2e.py`,
  `nintendo_letters`). `pad status` lists each declared glyph's part (`button`), `label` and `sprite`.
- A screen on the shell is tested through its `Shell`: `controller_ui`'s tests press a binding and read
  the row's "> A <", the strip, the art drawn in its row, and Undo putting a binding back.
