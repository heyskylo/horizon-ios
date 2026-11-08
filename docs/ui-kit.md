# The HUD kit

Horizon draws a few overlays of its own over the game, such as the controller notice and
splitscreen's join strip. Vanilla has no screen for them, so nothing in vanilla tells them where
they may go. The HUD kit is the small set of pieces they share, so they fit around vanilla's HUD
and look alike.

It lives in `crates/horizon-client-render/src/hud_kit/`, below gameplay. The game builds the
inputs once a frame and the overlays draw with them. It is immediate mode, with no state of its
own. Vanilla's display lists (`hud_layout`, `screen_draw`) never use it: they are vanilla's, and
parity-tested against it.

The kit grows a piece at a time, each with an overlay that needs it. Today it has four:

| Piece | What it gives |
|---|---|
| `hud_kit::room` | `Room`: what of a view an overlay may use, clear of vanilla's boss bars |
| `hud_kit::plate` | `Plate`: the box behind a notice's text |
| `pad_glyph`, with `GlyphOf` and `MenuJob` in `gamepad.rs` | `Glyph`: a controller button, asked for by what it does, in the player's `Glyphs` |
| `settings_shell` | `Shell`: a searchable settings screen (Extras, Graphics, Controller Settings) a `Screen` describes; it draws and drives it, binding rows included |

All sizes are GUI px from the view's top left, and an area is `[x, y, w, h]`.

## Room and plates

### Room

A `Room` is a view's size plus the keep-outs its HUD shows this frame. Each keep-out is a `Keep`
and an area. Today there is one kind:

| `Keep` | Where it comes from |
|---|---|
| `BossBars` | `hud_overlays::boss_bars_bottom(h, n)`: vanilla's `BossHealthOverlay` layout, y from 12 in steps of 19 until a third of the view's height; as wide as the 182 px bar or its widest name |

`Game::hud_room` (`crates/horizon-client/src/game/hud_room.rs`) gathers what is showing.
Nothing shows while F1 or Replay Studio hides the HUD, or off a world. `hud` empties `ui.room` at
the start of every frame, and `hud_layers` fills it in before the world's HUD draws, so every
overlay in the frame reads the same room and never a stale one.

A centred overlay asks `room.below_top(wanted)`. It keeps `wanted` unless a keep-out reaches past
it (`top() > wanted`, a real overlap; touching isn't one), and otherwise goes 2 px (`GAP`) below
that keep-out. The controller notice and splitscreen's join strip want y 18:

- with no boss bars, or one (it ends at y 17), they stay at 18
- with two they go to 38, with three to 57
- a notice under the join strip follows it, 38 px lower

`room.usable()` is the view below the top keep-outs, with the same gap.

```rust
use horizon_client::hud_kit::room::{Keep, Room};

let room = Room::new(320.0, 240.0).with(Keep::BossBars, [69.0, 0.0, 182.0, 36.0]);
assert_eq!(room.below_top(18.0), 38.0); // below two bars
```

The other HUD boxes (the minimap, the PvP & HUD widgets, mods' corner panels) don't use the room
yet. Chat, the hotbar, effect icons and subtitles aren't keep-outs yet either. #10125 asks for
these push-aside points for mods.

### Plates

`hud_kit::plate::draw(batch, atlas, plate, area)` fills an area with a plate's look, then draws
its 1 px edge inside the area.

| `Plate` | Look | Used by |
|---|---|---|
| `Notice` | dark fill (alpha 210), faint white edge | controller notices, splitscreen's prompts |

It keeps the colours these overlays had before the kit, so moving them onto it changes nothing on
screen.

### Tests

- `hud_overlays` tests pin `boss_bars_bottom` to vanilla's numbers and to `boss_overlay`'s own
  sprites.
- `hud_kit::room` tests cover an empty room and when an overlay moves.
- `game/split/hud.rs` tests place the join strip and the notice with 0 to 3 bars.

## Glyphs

Code asks for a glyph by what the button does, never by a button. The chain runs from the job to the art:

1. **What it does** (`gamepad::GlyphOf`): a gameplay action, `GlyphOf::Bound("key.jump")`, or a menu's fixed
   job, `GlyphOf::Menu(MenuJob::Back)`. Menus aren't rebindable, so `MenuJob::button` is the one map from a
   menu job to its button. It is the map for glyphs and for the on-screen keyboard's input. Menu input in
   `pad_menu.rs` still names its buttons; moving it onto `MenuJob` is a follow-up.
2. **The button**: the player's binding for an action (`PadSettings::binding`), or the menu map. An unbound
   action has no glyph, and in a debug build an action the pad doesn't know trips an assert.
3. **The glyphs** (`pad_glyph::Glyphs`): the family whose art shows, and whether a Nintendo pad goes by its
   letters. The game sets the player's once a frame, `ui.options.pad_glyphs = Glyphs::of(settings, pad)`.
   `Glyphs` is opaque. It is made only by `Glyphs::of(&PadSettings, family)`, from the player's choice,
   else the pad's family, and the letter setting, or by `Glyphs::unheld`, the exception below.
4. **The art**: `pad_glyph::Glyph` holds the part and the `Glyphs`. `draw`, `width`, `rect` and `art` take
   only a `Glyph`, and `Glyph::label` and `Glyph::color` give its text.

**Letter mode is part of the resolved glyphs; callers never handle it.** The game already reads a pad
going by its letters as parts (`PadState::by_letter`): the A on a Switch pad's right is South, which
confirms and jumps. Inside `pad_glyph`, a `Glyph` turns its part back into the button that plays it for the
art and the label, so South shows that A. `GlyphStyle` is only the setting: the Glyphs choice and
`horizon-controller.txt` keep `nintendo` and `nintendoLetters` as before.

What the types guarantee: no code outside `pad_glyph` can turn a bare `Button` into art or a label, and none
can make `Glyphs` without the `PadSettings` its letters come from. What they don't: a caller holding the
settings could still pass another pad's family, so gameplay reads the player's through `Game::glyph` and
`Game::menu_glyph`.

A new screen asks for a glyph like this:

```rust
// Gameplay: `Game::glyph` for anything, `Game::menu_glyph` for a menu job.
if let Some(jump) = self.glyph(GlyphOf::Bound("key.jump")) {
    pad_glyph::draw(batch, atlas, jump, x, y);
}
let back = self.menu_glyph(MenuJob::Back);

// In the render crate, with the glyphs the game passed in (`ui.options.pad_glyphs`):
let keys = [MenuJob::PagePrev, MenuJob::PageNext].map(|job| Glyph::menu(job, glyphs));
```

The one exception is `Glyphs::unheld(&settings, family)`, for splitscreen's join and lost-controller
prompts. They are answered by a controller nobody holds yet, whose letters aren't read yet, so they show its
family's buttons by their places ([controllers.md](controllers.md)). A join prompt uses the joining pad's
family.

### Glyph tests

- A glyph test builds the test atlas (`HudAtlas::build` on a missing folder gives Horizon's own art),
  draws into a `HudBatch`, and compares the quads or texture coordinates with the sprite expected.
- Letter mode is checked against input: `pad_glyph`'s test builds the glyphs from real `PadSettings`,
  presses each physical button, reads it the way the game does (`PadState::by_letter`), and expects that
  part's glyph to be the pressed button's print.
- In the e2e harness, `horizon-ctl pad family nintendo` makes the pad playing a Nintendo one, and
  `pad connection connected nintendo` raises a Nintendo pad's notice. With `nintendoLetters:true` in
  the seeded `horizon-controller.txt`, that is letter mode with no real pad (`tools/pad_menu_e2e.py`,
  `nintendo_letters`). `pad status` lists each declared glyph's part (`button`), `label` and `sprite`.

## The settings shell

### Moving a screen onto the shell

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

// A controller button, as Controller Settings > Bindings has each action, in the player's
// `Glyphs` the game hands the screen (`ui.options.pad_glyphs`):
let shown = Shown::Pad {
    glyph: Glyph::of(GlyphOf::Bound(action), &s.controller, glyphs),
    cancel: Glyph::menu(MenuJob::Start, glyphs),
};
let binding = Binding {
    capture: Capture::Pad(action),
    shown,
    shared, // the other actions on its button, by name
    reset: if s.controller.is_default(action) { Reset::Default } else { Reset::Changed },
};
Row::new(Item::Bind(action), name, Value::Binding(binding))
```

### Shell tests

- A screen on the shell is tested through its `Shell`: `controller_ui`'s tests press a binding and read
  the row's "> A <", the strip, the art drawn in its row, and Undo putting a binding back.
