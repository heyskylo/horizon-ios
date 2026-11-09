# Cosmetics

Small items Horizon players wear: a hat, something on the back, a shirt, a pet on the shoulder, a
style for their name tag. They are drawn by Horizon clients only and change nothing in the game.
**On by default** (Ludovic, 2026-11-04: on when the shop opens), your own shown. Other players'
show by default only if your account owns a cosmetic: a player who owns none sees a vanilla game,
and the game asks the service nothing about other players (CONTRIBUTING.md: Horizon-only features
that change the look are off by default). The settings below change both.

## Rules

- **Only Horizon clients see them.** Other Horizon players learn what someone wears from Horizon's
  cosmetics service, keyed by the Minecraft UUID. The game protocol doesn't change: servers, vanilla
  clients and anticheats receive nothing.
- **No gameplay effect of any kind**, no capes or anything that looks or acts like one, no random
  draws.
- **A setting hides them**: other players' cosmetics, and your own.
- **Zero cost when none is in view**, and a vertex budget per item.

## The item format (`crates/horizon-cosmetics`)

One JSON file per item. Coordinates are model pixels (1/16 block) in the player model's space:
y points down, as in vanilla's `ModelPart`s and Blockbench's "Modded Entity" models.

```json
{
  "format": 1,
  "id": "example_hat",
  "slot": "head",
  "bone": "head",
  "attach": { "offset": [0, -8, 0], "rotation": [0, 0, 0], "scale": 1 },
  "texture": { "png": "<base64 PNG>" },
  "parts": [
    { "name": "brim", "pivot": [0, 0, 0], "rotation": [0, 0, 0],
      "cubes": [{ "origin": [-4, -2, -4], "size": [8, 2, 8], "uv": [0, 0], "inflate": 0, "mirror": false }],
      "children": [] }
  ],
  "animations": [
    { "part": "brim", "motion": "swing", "axis": "z", "amplitude": 10, "period": 40, "phase": 0 }
  ]
}
```

- `slot`: `head`, `back`, `torso`, `shoulder`, `name_tag`, `emote`, `companion`, `trail` or
  `aura` (the last four below); a player wears at most one item per slot, and plays emotes. A
  `torso` item is hidden under a chestplate (not an elytra).
- `bone`: the player model part the item follows (`head`, `body`, `left_arm`, `right_arm`,
  `left_leg`, `right_leg`; `head` if left out). The item sits at the bone's matrix times `attach` (offset in pixels,
  rotation in degrees, a uniform scale in (0, 2]).
- `parts`: a tree of parts, each with a pivot, a rotation in degrees and boxes with Minecraft's box
  UV (`ModelPart.Cube`'s faces and order), grown by `inflate`, mirrored in x on request. Names are
  unique and never start with `cosmetic`.
- A **top-level part's `bone`** (optional) makes it ride another bone than the item's, under the
  same attachment: a shirt's sleeves (`"bone": "right_arm"`) follow the arms as they swing. Only
  top-level parts take one; the reach limit counts from each part's own bone.
- `animations`: loops on top of a part's rest pose, a sine of `period` ticks (4 to 1200) started
  `phase` periods in. `swing` turns the part `amplitude` degrees (at most 45) about the axis, `bob`
  moves it `amplitude` pixels (at most 4), `spin` turns it all the way round once per period.

A **name tag style** (`"slot": "name_tag"`) has no model: its texture is a square badge of at most
16×16 drawn after the player's name (before it sits Horizon's own mark, for Horizon players), and
`"tag": {"line": "#rrggbb"}` the colour of a line under it. `parts` stays empty and there are no animations; no other slot has a `tag`.

```json
{ "format": 1, "id": "example_tag", "slot": "name_tag", "texture": { "png": "<base64 8×8 PNG>" },
  "parts": [], "tag": { "line": "#a98fff" } }
```

Limits, checked when an item is read (`Item::parse`); an item past one is refused, never drawn:

| Limit | Value | Why |
|---|---|---|
| Vertices | 512 (21 boxes) | the player's own model and outer layer are 288 |
| Texture | 64×64 at most | it fits one of the atlas's skin slots |
| Parts | 16 | |
| Reach | 24 pixels from the bone's pivot, animations included | items stay near their wearer |

Unknown fields are refused, so a file never carries anything the game would silently ignore.

### Companions, trails and auras (`fx.rs`)

Three kinds ride no bone (no `bone` for the item or a part), so each goes with any other item, a
shoulder pet included. Like everything else they are worn (`PUT /v1/equipped`), reach other players
through `/v1/worn`, and change nothing in the game: no entity, no collision, no packet.

- **A companion** (`"slot": "companion"`): a small model floating beside the player. Its block says
  where it floats at rest (`anchor`: pixels from the neck in the wearer's body space, y down, +x the
  wearer's left, +z behind), how slowly it catches up when the player moves (`lag`, the follow's
  time constant, 1 to 40 ticks; 6 if left out) and which part, if any, turns to look where the
  wearer looks (`look`); `glow` draws it at full light. Idle motion is the usual `animations` (a
  `bob`, a wing's `swing`).

  ```json
  "companion": { "anchor": [-14, -6, 8], "lag": 8, "look": "head", "glow": false }
  ```

- **A trail** (`"slot": "trail"`): particles left behind while the player moves, no model (`parts`
  stays empty). The texture is a row of `sprites` square sprites of at most 16 texels; each
  particle shows one. `rate` is particles per block moved, `lifetime` ticks each lives, `size` its
  smallest and largest side in pixels, `gravity` blocks per tick² (negative rises, as embers),
  `drift` how far it sways sideways (pixels), `spin` degrees a tick, `from` `feet` (the default) or
  `body`, `fade` fades it over its last third, `glow` draws it at full light.

  ```json
  "trail": { "sprites": 4, "rate": 2, "lifetime": 30, "size": [1.5, 3], "gravity": 0.01,
             "drift": 1.5, "spin": 8, "from": "feet", "fade": true, "glow": false }
  ```

- **An aura** (`"slot": "aura"`): a see-through model whose origin is the middle of the player's
  feet (y down, so up is negative), `at` the `feet` or the `body`, drawn at `alpha`, at full
  light with `glow`.

  ```json
  "aura": { "at": "feet", "alpha": 0.5, "glow": true }
  ```

Their limits, on top of the vertices, texture and parts above:

| Limit | Value | Why |
|---|---|---|
| Companion size | 8 pixels from its origin, animations included | a small creature, not a second player |
| Companion anchor | 28 pixels from the neck at most; from the head's centre, 11 plus its size at least | it floats near the player and never over their face |
| Trail particles | `rate` ≤ 4 per block, `lifetime` ≤ 40 ticks, `size` ≤ 4 pixels, `gravity` within 0.04, `drift` ≤ 4, `spin` ≤ 20 | a short trail of small particles |
| Aura reach | 14 pixels sideways from the feet's middle; 6 pixels up at the feet, 30 around the body; 1 below | it frames the player without hiding anyone near them |
| Aura opacity | 0.6 at the feet, 0.35 around the body | a player inside one is as easy to see and hit |

## Emotes (`horizon_cosmetics::Emote`)

An emote is an item of the `emote` slot: a short animation of the player model, owned like any
item, never worn in a slot. Its `emote` block keyframes the bones; its `parts`, if any, are props
(boxes on a bone, as any item's, with the item's texture; an emote without props needs no
`texture`). Purely visual, nothing random, nothing sent to game servers.

```json
{
  "format": 1, "id": "example_wave", "slot": "emote", "parts": [],
  "emote": { "length": 40, "loop": false, "blend": 4, "tracks": [
    { "part": "right_arm", "keys": [
      { "t": 0 },
      { "t": 8, "rotation": [0, 0, 150] },
      { "t": 16, "rotation": [0, 0, 120], "ease": "linear" },
      { "t": 40 } ] },
    { "part": "root", "keys": [ { "t": 0 }, { "t": 6, "offset": [0, -2, 0] }, { "t": 12 } ] }
  ] }
}
```

- `length`: ticks; a one-shot ends there, a loop (`"loop": true`) repeats with that period and
  wraps from its last key back to its first.
- `blend`: ticks over which the emote fades in from the pose the player had (and a one-shot out
  at its end; a cancelled emote fades out from where it was), at most half the length; 4 if left out.
- `tracks`: one per thing moved. `part` is a bone (`head`, `body`, `right_arm`, `left_arm`,
  `right_leg`, `left_leg`): the key's `rotation` (degrees) replaces the bone's own and its `offset`
  (pixels) moves it from its rest; a bone no track names keeps vanilla's animation (the head
  still looks around). `root` turns (`rotation`) and moves (`offset`) the whole body about the
  feet. A prop's part name moves that part from its rest (in place of its own loop animations), and only a
  prop's key may `scale` it (0 to 2: it may pop in from nothing). A missing value is the rest.
- `ease`, from a key to the next: `smooth` (Catmull-Rom through the neighbouring keys: no corner
  at a key; the default), `linear`, `in_out` (smoothstep) or `step` (held).

Limits, checked by `Item::parse` with the item's own (vertices, parts, texture):

| Limit | Value |
|---|---|
| Length | 4 to 200 ticks (10 s) |
| Tracks | 16, one per part |
| Keys | 32 a track, 192 in all, going forward in time within the length |
| Turns | 360° about an axis (the root: 720°, two spins) |
| Moves | 8 pixels from the rest (the root: 16, a block's hop) |
| Props' reach | 24 pixels from their bone, through every key and 32 points of the length |

`Emote::pose_body` poses the six bones at a time and weight (the blend), `Emote::pose_props` the
props; `Playing` is an emote someone plays (its start, its cancel) and gives its time and weight.

### Playing an emote (`game/cosmetics/emotes.rs`, `game/cosmetics/emote_wheel.rs`)

- **The wheel**: *Emote Wheel* (`key.emote`, **B** by default: no vanilla key uses it; Controls
  rebinds it, kept as `emoteKey` in `horizon-extras.txt` so `options.txt` stays vanilla's, and so
  does the controller's page, with no button by default: the stick's cursor picks a slice). Up to 8 emotes around the middle of the screen, the game running behind. Hold
  the key, point at one and let go; or tap the key and click one, or press its number (1–8).
  Escape or the key again closes it. The wheel lists the emotes the account owns: when it opens,
  at most every 10 minutes, the game asks the service (`GET /v1/me`'s `emotes`: id, file hash
  and names, signed in with the session `account::Kept` keeps) and reads the files it lacks
  (`/v1/items`); `HORIZON_COSMETICS_LOCAL`: the folder's. An offline account has none.
- **Cancelled** by moving more than a hundredth of a block, attacking or mining, using an item,
  being hurt, riding, sleeping, gliding, sneaking, swimming or dying: it fades out over its blend.
  Only a standing player starts one. A loop plays until cancelled.
- **The camera**: from first person it turns to the front view (third person, facing you) for
  the emote and eases out from close to its usual distance over a quarter second; when the emote
  ends it goes back to first person, unless you changed the view meanwhile. From a third person
  view it stays as it is.
- **Visual only**: it changes nothing of the game; the game server receives nothing and sees a
  player standing still. `horizon-ctl emote [ID|stop]` plays one as the wheel does.

### Others' emotes (`horizon_cosmetics::Feed`)

Vanilla servers stay untouched: emotes travel through the cosmetics service only.

- **Mine**: while one plays, the game says so, signed in as for *Show me as Horizon*
  (`POST /v1/emote {"emote": id}`, `{"emote": null}` when it stops early), with a service
  session kept for the hour it lives (`account::Kept`: one Mojang `join` an hour, not one per
  emote). A loop is said again every 30 s (the service keeps one a minute). Only while *Show me
  as Horizon* is on: playing an emote tells anyone who asks that you play Horizon, so with it off
  nothing is sent. An offline account sends nothing. The service takes at most 6 at once, then
  one every 2 s per account, owned emotes only.
- **Theirs**: each tick the game names the Horizon players in view (the `horizon` list of the
  `/v1/worn` answers, at most 64) to a thread of its own, which asks
  `POST /v1/emotes {"players": [...]}` at most once a second, only while someone is named
  (nobody in view who plays Horizon: no request). The answer gives each player's emote, its
  file's hash and how long ago it started (`{"playing": {uuid: {"id", "hash", "ms"}}}`), and the
  game plays it on that player's model from that point; the item file comes once
  (`/v1/items`). A player who moves ends theirs on this screen at once. A failure waits 15 s,
  doubling to 10 minutes, and ends the emotes shown.
- **Switches**: others' emotes follow *Other players' cosmetics*: shown only with *Shown*; with
  *Name tags only* or *Hidden* nothing is asked and nothing is drawn. Vanilla clients see nothing.
- **Cost**: per player in view, one cache look a tick (the same lock as what they wear) and,
  while one plays, the six bones' sampling (a few keys each) after the vanilla animation in the
  pass that draws them; props are boxes in the item's budget, drawn as cosmetics are (the GPU
  skin path). Nobody emoting: one `Option` check per player model. Measured (2026-11-05, an
  i7-7500U laptop, `Playing::at` + `pose_body` + `pose_props` over each emote's length): 0.26 to
  0.41 µs per emoting player and frame, about 1 µs for one that turns its root (Victory Hop,
  Harvest Jig); 30 players emoting at once stay under 35 µs a frame.

## What other players wear (`horizon_cosmetics::Cosmetics`)

The game asks Horizon's cosmetics service (HTTPS, JSON) what the players in view wear, and draws
from its answers. The frame never waits on the network: it reads a cache, and a thread of its own
does the asking.

- **One request for the players in view**: `POST /v1/worn` with up to 64 UUIDs
  (`{"players": ["<uuid hex>", …]}`) answers `{"ttl": seconds, "players": {"<uuid hex>": [{"id",
  "hash"}, …]}, "horizon": ["<uuid hex>", …]}`; a player who wears nothing is left out of
  `players`, and `horizon` lists the batch's Horizon players (below). Only the UUIDs of players the
  game draws or lists are sent, nothing else about them or the server.
- **Cached for the service's TTL** (kept within 1 minute to 1 hour). Past it the last answer stays
  drawn while the player is asked for again. The 4096 players seen most recently are remembered.
- **Item files** come from `GET /v1/items/<id>?hash=<hash>` once and are kept on disk under their
  hash (they never change for one); a file past the limits is left out and not asked for again.
- **Offline**: a request that fails leaves every answer as it was, and the next one waits 15 s, then
  30 s, doubling up to 10 minutes. A player whose item didn't arrive is asked for again after 15 s.
- **Across restarts**: the last answers are kept beside the item files (`worn.json`: each player's item
  ids and hashes, written only when an answer changes them). They are read only when the service doesn't
  answer, so a game started while it is down still shows what players last wore, until it answers.

## Horizon players' mark

Like Lunar Client's: Horizon players see Horizon's mark (two white bars over a violet line,
`horizon_cosmetics::mark`) before the names of other Horizon players, above their heads (both
views, and your own with *Own name tag*) and in the player list, after the head. Nothing changes on
the wire to Minecraft servers: which players use Horizon comes from the cosmetics service, in the
same cached `/v1/worn` answers as what they wear (no request of its own, nothing per frame).

- **Who is reported**: a player whose game said "I play Horizon" lately. While *Show me as
  Horizon* is on, the game signs in (Mojang's `join`, as for *My cosmetics*) and sends
  `POST /v1/presence {"listed": true}` at start, then every hour (`account::PRESENCE_EVERY`); the
  service reports them for 3 hours after the last time (`COSMETICS_PRESENCE_WINDOW`). Off, the game
  sends `{"listed": false}` and the service forgets them at once: it never reports them again until
  they turn it back on. An offline account sends nothing and is never reported.
- **Anyone can ask**: `/v1/worn` takes no sign-in (as for what players wear), so anyone, a server
  included, can learn whether a UUID played Horizon in the last 3 hours while that player shows
  themselves. *Show me as Horizon*'s tooltip says so.
- **Settings** (Extras › Cosmetics, both on by default): *Horizon logos* ("Show the Horizon logo
  next to Horizon players"; off: the game draws no mark, and asks for nothing more than cosmetics
  need) and *Show me as Horizon* ("Show me as a Horizon player").
- **The player list** keeps a 9-pixel column for the mark once anyone listed has it, so names stay
  aligned.
- **Cost**: in a name tag, the mark is five filled rectangles beside the text, looked up under the
  same lock as the name tag style; in the player list, one look per listed player while it shows.

## In the game

- **The flag**: `horizon_cosmetics::ON_BY_DEFAULT` is `true`. `HORIZON_COSMETICS=0` turns
  everything off for one run (no thread, no request, no setting, no drawing), `=1` on;
  `HORIZON_COSMETICS_API=<url>` points the game at another service, and
  `HORIZON_COSMETICS_LOCAL=<folder>` reads items from a folder instead: every player and every
  mannequin wears its first item of each slot, and every mannequin plays the folder's first
  emote over and over (the art and the bench, without the network).
- **Settings** (Extras, under "Cosmetics", shown only with the flag on), the same three as the
  launcher's Wardrobe (`horizon_cosmetics::prefs`, lines of `horizon-extras.txt`):
  - *My cosmetics: Shown / Hidden* is the account's (`PUT /v1/visible`): hidden, every Horizon
    player sees you without them, you included. The game signs in for that one call the way the
    launcher does (`horizon_cosmetics::account`, Mojang's `join` with the player's session) and
    notes what the service confirmed in `cosmetics-visible.txt`; offline it tries again after 15 s,
    doubling to 10 minutes, until the service takes it. An unchanged switch sends nothing.
  - *Other players' cosmetics: Shown → Name tags only → Hidden*. Hidden, the game asks for nothing
    about them; name tags only, their name tag styles show and nothing on their bodies. Until the
    player presses it there is no `cosmeticsOthers` line, and it follows ownership
    (`prefs::Others::effective`): *Shown* if the signed-in account owns a cosmetic, else *Hidden*.
    The button shows that value; pressing it writes the line, which then wins. Hidden, nothing of
    theirs is drawn, yet with *Horizon logos* on the game still asks the same `/v1/worn` batch who
    plays Horizon.
  - **Ownership** (`horizon_cosmetics::account::owns`): once a run, in a world, the game signs in
    as for *My cosmetics* and reads `GET /v1/me`; an account whose `owned` list has anything owns
    one. The answer is kept per account in `cosmetics-owned.txt` (beside `cosmetics-visible.txt`;
    the launcher's Wardrobe writes it too) and used at the next start until the service answers.
    Not known yet, failed (retried after 15 s, doubling to 10 minutes) or offline: not owning. An
    answer that says owns shows others' at once, without a restart. Only one of the sign-ins (this,
    *My cosmetics*, *Show me as Horizon*) runs at a time (Mojang keeps one joined server id per
    account). `HORIZON_COSMETICS_LOCAL` counts as owning.
  - *Hide other players' trails and auras*, off by default (`cosmeticsHideFx`): their trails
    and auras stay hidden, their other cosmetics (companions included) still show.
  - *In single player*, off by default, the game's only (`cosmeticsSolo`). The
    window's own server names the player by their offline profile, which the service doesn't
    know, so in single player (open to LAN too) the game draws and asks nothing for the player's
    own model. On, it asks by the signed-in account's UUID: their cosmetics, their name tag style
    (with *Own name tag*) and Horizon's mark show in their own worlds. Offline accounts: nothing.
- **Name tag styles** (`game/names.rs`): the badge, 8 text pixels square, after the name and a
  one-pixel line in the style's colour under the name's background, in the name tag's own pass
  (the badge is in the atlas like the other items' textures). Horizon's mark goes before the name.
- **Over tall items** (`names::cosmetic_lift`, Horizon only): a player wearing an item whose
  highest point (`Item::top`: pixels above the model's neck with the bones at rest, animations
  included, found when the item is read) passes the tag's lowest pixel (2.05 blocks up, standing)
  has the whole tag, mark and badge included, lifted to one text pixel over it. Everyone else's
  tag stays at vanilla's height, as does the tag of anyone whose items aren't drawn (others'
  cosmetics on *Name tags only* or *Hidden*, *My cosmetics* hidden, a helmet over a hat). Cost:
  one max over the worn items per tag drawn, inside the look the mark already does.
- **Drawing** (`game/cosmetics.rs`): an item's texture takes one of the skin atlas's 64×64 slots
  and its boxes are posed on the wearer's bone each frame, through the same GPU skin path as the
  player model (the CPU path where that is off), lit and culled with the player. A helmet hides a
  head item, an elytra or cape a back item. The third person view and the
  inventory's player preview show your own.
- **Companions, trails and auras** (`game/cosmetics/fx.rs`), drawn only by the game, never sent to
  a server, never an entity: nothing collides with them or can be hit through them.
  - A *companion* is posed on no bone. Each frame its place moves toward the anchor (in the
    wearer's model space, turned with the body) by `follow::Follow::step`, a lag of `lag` ticks
    that doesn't depend on the frame rate; it jumps there when 8 blocks away (a teleport). It faces
    the wearer's head yaw and its `look` part turns with their pitch (60 %, ±30°). It is drawn with
    the wearer's items, lit where the wearer stands (`glow`: full bright). It never comes nearer
    the head than 11 pixels plus its size, and the player's own companion, in first person, never
    enters the 35° cone around the crosshair (`follow::out_of_view`); it is drawn after the
    first-person body goes to the shadow pass only.
  - A *trail*'s particles (`trail::Emitter`) are spawned only by distance walked (`rate` per
    block, none standing, sneaking or asleep, none after a jump of 4 blocks or more), live their
    `lifetime` exactly and are placed from their age alone (fall, sway, spin, fade): nothing is
    stepped per tick. 24 per player and 512 in all (`trail::Budget`); past them none spawn until
    some die. The player's own trail is never drawn in first person (their body isn't).
  - An *aura* is a model at the feet or round the body, its texels' alpha times `alpha`, its
    faces turned away from the camera dropped (`mesher::SHADED_CULL_W`) so nothing in it is drawn
    twice: it never shows more than `alpha`. None while the wearer flies with an elytra, swims or
    sleeps.
  - None is drawn for a wearer farther than 48 blocks from the camera (a few pixels across), and
    none for one outside the view (they are drawn with the wearer, after its frustum test).
  - Trails and auras of every player go in one see-through span after the players (shader packs
    see it as `minecraft:player`, translucent). Others' are hidden by *Hide other players' trails
    and auras*, and with every other cosmetic by *Name tags only* and *Hidden*.
- **Cost of companions, trails and auras**: the work of `horizon-cosmetics` per frame
  (`examples/fx_bench.rs`, items at the format's limits: a 504-vertex companion and aura and the
  densest trail; release build on an i7-7500U) is 3.6 µs for 1 wearer in view, 58 µs for 16 and
  189 µs for 64 (trails stop growing at the 512-particle budget). The game spends more: posing,
  the aura's vertices built on the CPU and their copy into the frame. On that laptop (Intel HD 620,
  `HORIZON_COSMETICS_LOCAL` mannequins wearing all three and walking in view), the frame's
  `prepare` phase grows by about 0.6 ms with 16 wearers and 1 to 2.4 ms with 64, mostly the auras.
  None in view: nothing.
- **Cost**: a player with nothing to wear adds one cache lookup; with no cosmetic in view the frame
  does no cosmetics work beyond that. The bench scene `stress_avatars` (300 mannequins, 20×15)
  measures the cost with `HORIZON_COSMETICS=1 HORIZON_COSMETICS_LOCAL=…` against the same run off.

## The live view (`horizon_cosmetics::preview`, `horizon_cosmetics::viewer`)

The launcher shows items on the player's own skin without the game: `preview::render` draws the
player model (vanilla's `PlayerModel` boxes on a 64×64 skin, wide or slim arms, the outer layers
included) and each item on its bone on the CPU, with a depth buffer and cutout alpha, as the game's
entity cutout draws them, seen from a `preview::Camera` (turn, tilt, zoom).

- **Camera**: dragging turns the player as if held (0.6° a point; the tilt stays within −30° to
  60°), the wheel comes up to 3 times closer (×1.15 a step), closing in on the head; *Reset view*
  goes back to `Camera::HOME` (30° turned, 10° from above, the whole player).
- **Animations** play on the game's clock (20 ticks a second, the same `Item::animate` as the
  game), only while the window is in front and not under reduced motion (`HORIZON_LAUNCHER_MOTION`,
  the system's setting); in the background the clock stops and they go on from there.
- **Cost**: `viewer::Stage` says when to draw again. Nothing changed (camera, outfit, scale, the
  clock), nothing is drawn and no timer runs: an unchanged page, an idle view or a window in the
  background does no work. At most 20 pictures a second while something animates (one a game
  tick), 60 while dragging or zooming. A picture is drawn at the view's size in points (240×330,
  each pixel a block of device pixels, as the launcher's pixel art), about 2 ms on the CPU on any
  screen. The view is its own GPUI view: a new picture rebuilds it alone, the
  page around it is drawn from the last frame.
- **Emotes**: `viewer::BodyMotion` is a body animation played once (the parts' poses over its
  length, on the same clock); `preview::Draw::item_posed` keeps each item on its bone as posed. The
  launcher's `Viewer::play` takes one: the emote format implements `BodyMotion` and the Wardrobe
  calls it.

## The Wardrobe's account check (`launcher/src/wardrobe.rs`)

The launcher talks to the same service for the player's own items: the catalogue, what the account
owns, its purchase history, what each slot wears and whether other players see it. Paying happens
on the website: the launcher's Shop tab starts the checkout and opens https://playhorizon.dev/shop/pay
in the browser; no card is ever typed into the launcher.

- **Who the player is**: the service hands out a one-time server id (`POST /v1/auth/start`), the
  launcher tells Mojang's session server that the account joins it
  (`account::session::join_server`, the call a client makes before joining an online-mode server),
  and the service asks Mojang (`hasJoined`) before it answers `POST /v1/auth/finish` with a
  session of its own. The Minecraft token goes to Mojang only; the service never sees it.
- **Then**: `GET /v1/me`, `PUT /v1/equipped {"slot", "item"}` and
  `PUT /v1/visible {"visible"}` ("Show my cosmetics": off, `/v1/worn` leaves the player out) with
  that session (`Authorization: Bearer`), kept in memory for the time the service gave, less a
  minute.
- **The founder set** (Founder Visor, Founder Tee, Founder Backpack, Founder Tag) is free and can
  be claimed on the website only between the window's start and end; after that it is retired for
  good and never offered again. Players who claimed it keep it, and it shows in the Wardrobe.
- **Launcher themes** are never claimed or sold: they are free for everyone in Settings ›
  Appearance (*Launcher theme*: Horizon or First Light, `horizon_launcher::themes::THEMES`). A
  catalogue entry with `"slot": "theme"` is ignored.
- **Offline**: the last catalogue and answer, and the changes not sent yet, are kept per account
  in the launcher's data folder (`wardrobe/<uuid>.json`; never a session or token), item files
  under their hash. A change shows at once; the ones the service couldn't take (offline, or a
  failure on its side) are sent in order when it answers again, and a newer change of the same
  slot replaces a waiting one.

## The Wardrobe page (`launcher/src/wardrobe_page.rs`)

A top-bar page between Servers and Versions, for the selected Microsoft account (signed out, it asks
you to sign in). It shows only when the service has a founder set to show (or with
`HORIZON_COSMETICS=1`, for development), whatever cosmetics in the game do (`horizon_launcher::shop`): the launcher asks `GET /v1/shop` at start and every
half hour, with no session and nothing about the player. The page shows when the set can be claimed
now (`open`), when the public window has dates, or when the selected account's own answer, kept from
its last visit, does (a claim, or a preview the service grants that account). With no window set, the
page stays hidden.

- **A row per slot** (head, back, shirt, shoulder pet, name tag) with what it wears,
  "Nothing equipped" or "None owned yet"; **your player** in the middle, in 3D on your own skin,
  wearing it, with your name tag over the head as others see it (Horizon's mark unless the game's
  *Show me as Horizon* is off, your style's badge after the name): drag to turn, scroll to zoom,
  *Reset view* (the live view above, `launcher/src/wardrobe_page/viewer.rs`); **what you own for
  the chosen slot**, "Nothing" first. Hovering a card tries its item on the player (or, for
  "Nothing", takes the slot's item off) until the pointer leaves; equipping shows at once.
- **One way to the Shop tab**: a plain button by the page's title, *Get more
  cosmetics*, opens the launcher's Shop tab (below; paying happens on the website); no badge,
  reminder or pop-up points to it. An account that owns nothing yet sees "You don't own any
  cosmetics yet." with that button instead. The founder set's free *Claim for free* where Paddle
  doesn't sell (`Claim::here`) stays here too: the site sends those players to the Wardrobe, since
  only the launcher can prove the account with Mojang. **Your purchases** lists the account's orders (date, item, price, state, order number).
- **Three switches**: *Show my cosmetics* (the account's, on the service: off, everyone sees you
  without them), *Hide other players' cosmetics* and *Hide other players' trails and auras* (off by
  default). The last two are lines of the game's `horizon-extras.txt` (`horizon_cosmetics::prefs`:
  `cosmeticsOthers` = `shown`, `name_tags` or `hidden`, `cosmeticsHideFx`), the same ones the
  game's settings change. With no `cosmeticsOthers` line, *Hide other players' cosmetics* shows
  what the game does: on unless the account owns something. Each answer of the service also
  notes the account's ownership in the game's `cosmetics-owned.txt`.
- **States**: offline, the page keeps working from what's kept on disk and says how many changes
  wait to sync; a service failure says what you wear still shows in the game, with *Try again* and
  *Copy details* (host, status, time).
- `HORIZON_LAUNCHER_PAGE=wardrobe` in a development build shows the page for a made-up account
  wearing `HORIZON_COSMETICS_LOCAL`'s items; `HORIZON_WARDROBE_STATE=offline|failed|empty`
  shows that state.

## The Shop page (`launcher/src/wardrobe_page/shop.rs`, `launcher/src/shop.rs`)

A plain top-bar tab beside the Wardrobe, shown under the same rule. Ludovic (2026-11-05): not in your
face, people play without pop-ups. So the tab has no badge, dot, count or "New"; it never opens by
itself and is never the start page; nothing anywhere (launcher or game) points to it; Play never waits
for it. No countdowns, scarcity, buyer counts, "limited" wording or loot boxes.

- **Everything comes from `GET /v1/catalogue`** (the Wardrobe's own trip): `currencies`, each item's
  `prices` (minor units per currency, only while on sale), `sets`, `collections` and `supporter`.
  What the catalogue doesn't price isn't sold, so items the service puts on sale
  (`COSMETICS_ON_SALE`) show up by themselves. The currency is guessed from the system locale
  (francs in CH/LI, pounds in GB, euros in the euro area, else dollars) and can be switched.
- **Laid out like a game's item shop** (Ludovic, 2026-11-07: "you just don't see the items"): the
  first collection's `pack` (else its largest set) across the top, big, with its name, item count,
  price and *Buy*; then *Looks* (the other sets), each collection's items, and the founder set and
  the Supporter badge. A tile is the thing itself drawn, its name and its price, nothing else; it
  lifts under the pointer. Very little text anywhere.
- **Pictures** (`shop/thumbs.rs`): drawn by the Wardrobe's renderer (`preview::fitted_view` frames
  an item on itself) at the screen's pixels, twice as fine then halved, on background threads, kept
  by the item files' hashes (and the skin). An item alone at an angle (a back item from behind, an
  aura from above, a trail's particles grown), a shirt on a plain figure, an emote on the player at
  its fullest move, a look on the player, the pack as its looks side by side. Name tags show as the
  game draws them. Without a skin the launcher can read (signed out, offline, an unreadable one),
  the player is Horizon's stand-in, the website shop's own original art (`stand_in` in
  `wardrobe_page.rs`, `standInSkin` on the site), never Mojang's Steve or Alex.
- **A tile opens it big**: the account's own player in 3D wearing it over what it wears
  (`shop::try_on`; drag to turn, scroll to zoom; an emote plays, again each time it ends), its name,
  price and line, a look's or the pack's items, *Buy* (not for what the account owns) and *Gift*
  (the friend's Minecraft name, then *Pay … in your browser*, priced without what they own). Each
  is one checkout of that offer (`POST /v1/web/checkout`); "Ask whoever pays before you buy.
  Refunds within 14 days for any reason." sits under the buttons.
- **The founder set**: *Claim for free* where Paddle doesn't sell (`Claim::here`), else Paddle's free
  checkout in the browser (`POST /v1/web/checkout` with `founder_set`).
- **A catalogue that doesn't load** shows "The shop didn't load." with *Retry*; nothing else changes.
- `HORIZON_LAUNCHER_PAGE=shop` in a development build shows it for the made-up account of the
  Wardrobe's preview; `HORIZON_COSMETICS_CATALOGUE=<file>` gives it a saved `GET /v1/catalogue`
  (prices, sets), `HORIZON_COSMETICS_SKIN=<png>` a skin, `HORIZON_SHOP_OPEN=<id>` an offer open.
