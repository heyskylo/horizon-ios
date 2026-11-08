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
