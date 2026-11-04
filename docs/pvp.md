# PvP & HUD

Settings PvP players expect from a client, on **Options → Horizon Settings... → PvP & HUD...**. Every one is
off by default: a fresh install looks exactly like vanilla 26.3, and turning one off restores vanilla.

## Fair play

Nothing here tells the player more than vanilla does or plays for them, so the game stays fine on servers
that ban unfair clients. That rules out, on purpose: a reach display, entities on the minimap, freelook,
auto-clicking or any other automatic action, seeing through walls, and hitboxes beyond vanilla's F3+B.
Each feature is built from what it visibly does, never from another client's or mod's code.

## Where it lives

| Part | Code |
|---|---|
| Settings, parsing, the page's rows | `crates/horizon-client-render/src/pvp.rs` (stored in `horizon-extras.txt`, see [extras.md](extras.md)) |
| The page | `options_ui.rs` (`Page::Pvp`), the rows of `pvp::rows()` drawn like the Extras page |
| Game glue | `crates/horizon-client/src/game/pvp.rs` |

## Crosshair (`crosshair`, default off)

- Shape (cross, T, X, circle, square, dot only), arm size 1-12, gap 0-8, thickness 1-4 GUI pixels, a
  colour (Inverted, as vanilla's crosshair blends, or a solid one), a black outline and a centre dot.
- Its first settings draw vanilla's crosshair: a 15-pixel cross, one pixel thick, inverting what is under it.
- Colour on entities: another colour while the crosshair is on a living entity, the same target vanilla's
  attack indicator reads (`crosshairPickEntity`).
- The attack indicator, the F3 axes and the spectator rules stay vanilla's; only the sprite is replaced.
- Cost: the pixels are worked out once per settings change (`CrosshairCache`); a frame adds a few quads.

## Block outline (`outline`, default off)

- Colour (solid), opacity 10-100%, thickness 1-4 times vanilla's width, and an optional fill that tints the
  targeted block's faces at 25%. Turned on, its first settings are vanilla's: black at 40%, vanilla's width.
- With Accessibility's high contrast outline, the colour replaces the bright stroke and the black one stays.
- The fill draws each box of the block's shape (`getShape`, the boxes the outline's edges come from) as
  translucent faces after translucent terrain, 0.002 out from the block. Only while the outline shows.
- Cost: the stroke is vanilla's draw with another colour and width; the fill adds at most a few dozen quads.

## Hit colour (`hit`, default off)

- The colour hurt mobs and players flash, and how much of it covers them (0-100%). Its first settings are
  vanilla's: `OverlayTexture`'s red over 30%.
- The texel rides in the frame's uniforms (`colored_count.y`, 0 for vanilla's), so changing it costs nothing.
- Shader packs keep vanilla's red (`pack_entities::entity_color`).
