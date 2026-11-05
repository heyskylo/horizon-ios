# PvP & HUD

Settings PvP players expect from a client, on **Options → Horizon Settings... → PvP & HUD...**. Every one is
off by default: a fresh install looks exactly like vanilla 26.3, and turning one off restores vanilla.

## Fair play

Nothing here tells the player more than vanilla does or plays for them, so the game stays fine on servers
that ban unfair clients. That rules out, on purpose: a reach display, entities on the minimap, freelook,
auto-clicking or any other automatic action, seeing through walls, and hitboxes beyond vanilla's F3+B.
Three options do see further than vanilla would at that moment, and some strict servers ban fog or light
changes: clear water and clear lava (below), and a fixed time of day, whose sky light brightens a night.
They are off by default; check a server's rules before turning them on there.
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

## HUD widgets (each default off)

Boxes in the screen corners, placed like the minimap and mods' boxes (`extras_hud::Corners`), each in a corner of
its own, with an optional dark background (on by default). F3's debug screen takes the top corners as it does from
mods' boxes. Code: `crates/horizon-client-render/src/pvp_hud.rs` (lines, cache, layout) and `game/pvp.rs`.

- **Armour** (bottom right): each piece worn, head first, with the uses left as a number or a percent, coloured
  like the item's durability bar (green to red). Only what the inventory already shows.
- **Effects** (top right): icon, name and level, time left, for every active effect; in place of vanilla's
  icons there (`Hud.extractEffects`), as the inventory's effect panels show them.
- **Held item count** (bottom right): how many of the held item the inventory and off hand hold; with a bow or
  crossbow, the arrows. Hidden for a single item.

Cost: a widget keeps its lines (`pvp_hud::Widget`) and rebuilds them only when what it shows changes (a piece
takes damage, an effect's second ticks, the count changes); a frame reads a few slots, compares, and adds the
box's quads. With nothing changing, a frame allocates nothing.
- **Ping** (top left): the latency the server's player list reports for you (the tab list's bars as a number); nothing until the server's first latency update, up to 30 s after joining.
  Multiplayer only.
- **Coordinates** (top left): block position and the direction faced (`Direction.fromYRot`), as F3 shows them.
  Hidden whenever F3 is reduced (`reducedDebugInfo`: the option, the game rule or the server), so it never shows
  what the server hides.
- **Server address** (top left): the address you joined (`net::shown_address`: invite codes stay hidden).
  Multiplayer only.
- **Combo counter** (top left): your hits in a row, each counted when the server reports an entity hurt with you
  as the damage's direct entity (`ClientboundDamageEventPacket`: melee and spear hits, not arrows or fire). A
  report that names no attacker counts once, within a second of a swing at that entity. Back to 0 when you are hurt or
  3 seconds pass without a hit. It counts what the game already shows (the target's hurt flash).

## Enchantment levels as numbers (`numericEnchants`, default off)

`Enchantment.getFullname` with the level as a number ("Sharpness 5", also past vanilla's numerals: "Sharpness 255")
wherever the client names an enchantment (tooltips, the enchanting table's hint). A one-level enchantment still has
none ("Mending"). Code: `horizon_items::enchant::set_numeric_levels`, set each frame from the option.

## Enchantment glint colour (`glintColour`, default off)

The glint's shimmer in a solid colour: each sample's brightest channel over the colour, so the pattern, speed
and strength stay vanilla's (`entity.wgsl` `tinted_glint` for items in the world, held and worn armour,
`hud.wgsl` for items in the GUI). The colour rides in uniforms (`colored_count.z`, the HUD's `end_portal[2].yzw`),
so it costs nothing. Resource packs' core shaders and OptiFine CIT glints keep their own colours.

## Visual tweaks (each default off)

| Option | Key | What it does |
|---|---|---|
| Lower fire | `lowFire` (0-80%) | Moves the first-person fire overlay (`ScreenEffectRenderer.renderFire`) down by that share of its height. |
| Shield size | `shieldSize` (50-100%) | Scales the first-person shield toward the hand holding it. |
| Totem animation size | `totemSize` (25-100%) | Scales the totem of the item activation animation about its centre. |
| Clear water | `clearWater` | Water fog as vanilla's after 30 seconds under water (water vision 1), from the first tick. |
| Clear lava | `clearLava` | Lava fog as vanilla's with Fire Resistance (0 to 5 blocks), without the effect. |

Both fog options stop at the clearest view vanilla itself gives, but they give it at once: 96 blocks of water
on entering instead of 24, and 5 blocks of lava without the effect instead of 1. That is an edge in a fight
(see Fair play).
There is no "no hurt camera" option: vanilla's Accessibility → Damage Tilt already sets the shake from 0 to 100%.

## Your view: time of day and weather (`viewTime`, `viewWeather`, default the world's)

The time of day (morning, noon, sunset, midnight) and the weather (clear, rain, thunder) this player sees. Only
the frame changes: the sky's colours, sun, moon and stars, the sky's light in the lightmap, the rain and its
fog (`Game::view_day_time`, `Game::view_weather` in `game/pvp.rs`). The world's `day_time` and weather, and all
that plays by them (mobs, crops, beds, riptide, rain sounds), stay the server's, and nothing is sent to it.
The moon keeps the world's phase. Only a client of a server draws it (single player's window is one), in the
Overworld.

## Chat (each default off)

| Option | Key | What it does |
|---|---|---|
| Timestamps | `chatTimestamps` | The local time a message came in, before it, in grey (`horizon.chat.timestamp`). |
| Compact chat | `chatCompact` | A message equal to the one before it (same text, kind and tag, neither signed) raises that one's count, "(x3)", instead of adding lines. |
| Copy on click | `chatCopy` | In the open chat, a click on a message where the click has no action of its own copies the message's text (without time or count). |
| Chat search | `chatSearch` | Ctrl+F in the open chat: the box takes a query and only the messages holding it (any case) show. Enter, Esc or Ctrl+F again stops. |

They change how `chat::Chat` lays its lines out (`ChatQol`, `set_search`): the 100 messages it keeps are the same,
so turning an option off shows them as before. Code: `chat.rs`, `game/chat.rs`.

## PvP preset

The **PvP preset** button (first on the PvP & HUD page and on Horizon Extras) turns on, in one click: the custom
crosshair, the armour and effects widgets, vanilla's Toggle Sprint and Toggle Sneak (Controls), lower fire (40% unless
already set), and the official FPS counter and keystrokes (with CPS) native mods when they are installed
(`pvp::PRESET_MODS`). It only turns things on and keeps each option's own settings, so any of them can be turned
back off by hand. Code: `pvp::apply_preset`.
