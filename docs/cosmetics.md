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

- `slot`: `head`, `back`, `torso`, `shoulder` or `name_tag`; a player wears at most one item per
  slot. A `torso` item is hidden under a chestplate (not an elytra).
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
  mannequin wears its first item of each slot (the art and the bench, without the network).
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
  - *Others' auras and trails*, off by default (no item of that kind exists yet).
  - *In single player*, off by default, the game's only (`cosmeticsSolo`). The
    window's own server names the player by their offline profile, which the service doesn't
    know, so in single player (open to LAN too) the game draws and asks nothing for the player's
    own model. On, it asks by the signed-in account's UUID: their cosmetics, their name tag style
    (with *Own name tag*) and Horizon's mark show in their own worlds. Offline accounts: nothing.
- **Name tag styles** (`game/names.rs`): the badge, 8 text pixels square, after the name and a
  one-pixel line in the style's colour under the name's background, in the name tag's own pass
  (the badge is in the atlas like the other items' textures). Horizon's mark goes before the name.
- **Drawing** (`game/cosmetics.rs`): an item's texture takes one of the skin atlas's 64×64 slots
  and its boxes are posed on the wearer's bone each frame, through the same GPU skin path as the
  player model (the CPU path where that is off), lit and culled with the player. A helmet hides a
  head item, an elytra or cape a back item. The third person view and the
  inventory's player preview show your own.
- **Cost**: a player with nothing to wear adds one cache lookup; with no cosmetic in view the frame
  does no cosmetics work beyond that. The bench scene `stress_avatars` (300 mannequins, 20×15)
  measures the cost with `HORIZON_COSMETICS=1 HORIZON_COSMETICS_LOCAL=…` against the same run off.

## A still picture (`horizon_cosmetics::preview`)

The launcher shows items on the player's own skin without the game: `preview::render` draws the
player model at rest (vanilla's `PlayerModel` boxes on a 64×64 skin, wide or slim arms, the outer
layers included) and each item on its bone on the CPU, with a depth buffer and cutout alpha, seen
from any angle.

## The Wardrobe's account check (`launcher/src/wardrobe.rs`)

The launcher talks to the same service for the player's own items: the catalogue, what the account
owns, its purchase history, what each slot wears and whether other players see it. Buying and
claiming the founder set happen on the website's shop (https://playhorizon.dev/shop), never in the
launcher.

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
  "Nothing equipped" or "None owned yet"; **your player** in the middle, on your own skin, wearing
  it, with your name tag over the head as others see it (Horizon's mark unless the game's *Show me
  as Horizon* is off, your style's badge after the name) (`horizon_cosmetics::preview`); **what you own for the
  chosen slot**, "Nothing" first. Equipping shows at once.
- **The shop is on the website**: an *Open the shop* button by the page's title opens
  `https://playhorizon.dev/shop` in the browser (browsing, prices, the founder claim, purchases through
  Paddle). An account that owns nothing yet sees "You don't own any cosmetics yet." with that button
  instead. **Your purchases** lists the account's orders (date, item, price, state, order number).
- **Three switches**: *Show my cosmetics* (the account's, on the service: off, everyone sees you
  without them), *Hide other players' cosmetics* and *Show other players' auras and trails* (off by
  default). The last two are lines of the game's `horizon-extras.txt` (`horizon_cosmetics::prefs`:
  `cosmeticsOthers` = `shown`, `name_tags` or `hidden`, `cosmeticsAuras`), the same ones the
  game's settings change. With no `cosmeticsOthers` line, *Hide other players' cosmetics* shows
  what the game does: on unless the account owns something. Each answer of the service also
  notes the account's ownership in the game's `cosmetics-owned.txt`.
- **States**: offline, the page keeps working from what's kept on disk and says how many changes
  wait to sync; a service failure says what you wear still shows in the game, with *Try again* and
  *Copy details* (host, status, time).
- `HORIZON_LAUNCHER_PAGE=wardrobe` in a development build shows the page for a made-up account
  wearing `HORIZON_COSMETICS_LOCAL`'s items; `HORIZON_WARDROBE_STATE=offline|failed|empty`
  shows that state.
