# The HUD kit

Horizon draws a few overlays of its own over the game, such as the controller notice and
splitscreen's join strip. Vanilla has no screen for them, so nothing in vanilla tells them where
they may go. The HUD kit is the small set of pieces they share, so they fit around vanilla's HUD
and look alike.

It lives in `crates/horizon-client-render/src/hud_kit/`, below gameplay. The game builds the
inputs once a frame and the overlays draw with them. It is immediate mode, with no state of its
own. Vanilla's display lists (`hud_layout`, `screen_draw`) never use it: they are vanilla's, and
parity-tested against it.

The kit grows a piece at a time, each with something on screen that needs it:

| Piece | What it gives |
|---|---|
| `hud_kit::room` | `Room`: what of a view an overlay may use, clear of vanilla's boss bars |
| `hud_kit::plate` | `Plate`: the box behind a notice's text |
| `hud_kit::tabs` | `collapsed`: a page list in one row, `‹ Name ›` with a pip per page |

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

## Tabs

`hud_kit::tabs::collapsed(atlas, names, selected, area)` lays out a page list in one row: the
selected name between `‹ ›` arrows, cut to the room between them, with a pip per page under the
row, the selected one white. `tabs::draw(batch, atlas, &strip)` paints it.

- `has_prev` and `has_next` say whether there is a page either way; an arrow without one is drawn
  dimmed (`ARROW_OFF`). A screen may change them, as Shaders' search does.
- When the pips don't all fit across the area, the ones shown are a window round the selected
  page's, so they never leave the row's width.
- It only lays out and paints: what a click or a bumper does is the screen's.

### Shaders' screens

Shaders' page list (`shader_ui`) has the pack list, a line naming the picked pack, then the pack's
screens near its main one. In a short view the column has room for only a few of them (four at
427x240 GUI px: an 854x480 window at GUI scale 2, or 720p at its automatic scale), and the rest
used to be left out with nothing to say so. Now, when they don't all fit, they collapse into one
`tabs::collapsed` row. When they fit, the list is as it was.

- Left of the name opens the screen before, the name the one shown, right of it the screen after.
  While searching, before and after are the nearest screens with matches.
- Each part is a `Tab` target for a controller (the name lit while its screen is open), so RB and
  LB step through every screen, and past the last RB goes back to the pack list. The arrows are
  recorded as vanilla's Previous Page and Next Page for the widget tree and the narrator.
- The open screen's row has the list's open look (plate and accent bar), and the focus ring goes
  round the whole row.

Tests: `hud_kit::tabs` (centring, cutting, dimmed ends, the pip window), `shader_ui` (a short view
leaves no screen out and a tall one keeps the list, the arrows walk every screen, search skips
screens without matches, the parts are `Tab` targets), and `tools/pad_menu_e2e.py --only
shader_screens` (RB through every screen of a twelve-screen pack in a real game).
