# The HUD kit

Horizon draws a few overlays of its own over the game, such as the controller notice and
splitscreen's join strip. Vanilla has no screen for them, so nothing in vanilla tells them where
they may go. The HUD kit is the small set of pieces they share, so they fit around vanilla's HUD
and look alike.

It lives in `crates/horizon-client-render/src/hud_kit/`, below gameplay. The game builds the
inputs once a frame and the overlays draw with them. It is immediate mode, with no state of its
own. Vanilla's display lists (`hud_layout`, `screen_draw`) never use it: they are vanilla's, and
parity-tested against it.

The kit grows a piece at a time, each with an overlay that needs it. Today it has three:

| Piece | What it gives |
|---|---|
| `hud_kit::room` | `Room`: what of a view an overlay may use, clear of vanilla's boss bars |
| `hud_kit::plate` | `Plate`: the box behind a notice's text |
| `pad_glyph`, with `GlyphOf` and `MenuJob` in `gamepad.rs` | `Glyph`: a controller button, asked for by what it does, in the player's `Glyphs` |

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
