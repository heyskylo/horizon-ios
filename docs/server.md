# Hosting a Horizon server: design

Horizon's dedicated server should be the easiest and lightest Minecraft 26.3 server to host. It should be one
small native program with no Java and no Python, one command to install it, one config file, and a CLI, a web
panel and a control API to manage it. At the same view and simulation distance and with the same players, it
should beat vanilla, Paper and Fabric + Lithium on TPS/MSPT, RAM, CPU, chunk generation and start time.
Gameplay stays vanilla: there are no Paper-style rule changes unless the owner turns one on.

This page is the design of `horizon-server` and the engine it runs; the section numbers are cited from the code.
The owner's guide is [hosting.md](hosting.md); every command, `server.toml` key and API route is in the generated
[api/server-reference.md](api/server-reference.md).

## 1. Today

### How it runs

The dedicated server is the game itself, run headless:
`horizon-client --resources <prepared> --dedicated [--nogui] [--port N] [--world NAME] [--universe DIR]
[--initSettings]`, started in the server folder (`crates/horizon-client/src/game/dedicated.rs`).

- **Startup.** It follows vanilla's `Main.main` and `DedicatedServer.initServer`. It reads `server.properties`
  and writes it straight back with vanilla's defaults filled in (`horizon_client::server`, byte-exact with
  vanilla's `Properties.store`), and refuses to start until `eula.txt` says `eula=true`. It then loads the
  operator, whitelist and ban lists in vanilla's order and format, and opens `<universe>/<level-name>` with
  the same `Game` as single player, minus the local player.
- **Network.** Players join through the play-state hub that Open to LAN uses (`crates/horizon-client/src/lan`).
  Status ping, legacy ping, compression, login gate (bans, whitelist, IP bans), operator permission levels,
  the player limit with `bypassesPlayerLimit`, MOTD and `server-icon.png` all follow `server.properties`.
- **Main loop.** It ticks at 20 TPS on one thread, with chunk generation and lighting on the worker pool.
  Chunks stay loaded around every player (`lan_update_streaming`), or around the spawn while nobody is
  online. It supports `pause-when-empty-seconds`, `player-idle-timeout` and the autosave.
- **Console (stdin).** `stop`, `save-all`, `save-on`/`save-off`, `list [uuids]`, `op`/`deop`,
  `whitelist on|off|list|reload|add|remove`, `ban`, `ban-ip`, `pardon`, `pardon-ip`, `banlist`, `kick` and
  `say` behave as in `DedicatedServer`. `tempban <player> <duration> [reason]` is Horizon's own: a ban that
  ends by itself ([hosting](hosting.md#temporary-bans)). Every other command goes to the game's own dispatcher as the
  server (permission level 4). The log follows vanilla's Log4j pattern, in `logs/latest.log` and on stdout.
- **Player names.** `usercache.json` in the universe folder works as vanilla's `CachedUserNameToIdResolver`.
  Players are added when they join and kept for a month, and the file keeps the 1000 most recently used,
  newest first. The console finds a player by name in it, in any case. A name it doesn't hold is asked of the
  profile service (`getByName`). If the service doesn't know the name, an offline server uses the offline
  player of the lower-case name; an online server answers "That player does not exist".
  `tools/test_user_cache_parity.py` checks this against vanilla, with a mock profile service.

### Scale

Measured against vanilla on equal work (section 9): 20 TPS up to 50 bots, 9 to 11 TPS at 100, where vanilla's
watchdog stops it. Horizon is ahead with few players and when exploring; with 25 to 50 players near spawn
vanilla's tick is shorter. What the dedicated server checks against vanilla (online mode, secure chat,
dimensions, remote players' gameplay) is in its harnesses: `python3 tools/parity.py dedicated_server online_mode
secure_chat server_dimension_players server_player_combat server_player_damage`.

## 2. Architecture: a supervisor and an engine

```
 owner ──► horizon-server (CLI, TUI)          browser ──► web panel           AI ──► MCP (stdio)
                 │                                │                               │
                 └────────── control API: HTTP + JSON on 127.0.0.1, token ────────┘
                                                  │
                                   horizon-server run   (the supervisor: no GPU, no game code)
                  server.toml ──► server.properties ──► │ ◄── stdout: the vanilla log
                  backups, updates, crash restart       │ ──► stdin: console commands
                                                        │ ◄─► engine endpoint: status, TPS/MSPT, players
                                                        ▼
                                   the engine: horizon-client --dedicated   (the game, headless)
```

`horizon-server` is a new crate and binary (`crates/horizon-server`). It is the only program an owner runs.
It depends only on the light crates (`horizon-core`, `horizon-protocol`, `horizon-world`, `horizon-bench`)
plus `serde`, `toml`, `toml_edit` and `zip`, which are already in the lockfile. It doesn't link wgpu, winit,
audio or controller crates. It runs the engine as a child process.

The supervisor/engine split stays even after the engine becomes headless, for these reasons:

- **Crash restart and diagnostics.** The supervisor outlives an engine crash. It keeps the crash report,
  restarts the engine with backoff, and the panel stays up to say what happened.
- **Safe updates.** An update swaps the engine binary and restarts. If the new engine fails its start check,
  the supervisor rolls back to the previous one.
- **Parity stays cheap.** The engine keeps reading vanilla's own files (`server.properties`, the four JSON
  lists, `eula.txt`) exactly as `test_dedicated_server_parity.py` checks them today. The clean config is a
  layer on top and never changes what the engine sees.
- **No conflicts.** The supervisor touches none of the game code that the tick and integrated-server work
  (perf-server-1, perf-ticks-2) are changing.

**The engine today** is `horizon-client --dedicated`. It needs one small, additive change: a status endpoint.
With `--control-socket <dir>/.horizon/engine.json` (the existing control protocol: TCP on 127.0.0.1 plus a token, on
every OS), the dedicated loop answers `server-status` with TPS, MSPT (average, P50/P95/P99 from the tick
clock `/tick query` uses), players (name, UUID, address, ping, position), loaded chunks and uptime. It answers
`console <line>` with the command's output.

**A proxy moving a player between two servers** uses more requests on this endpoint (`lan/arrivals.rs`,
`game/dedicated/transfer.rs`). They are a built-in module, **off by default**, that the owner turns on in
`server.toml`:

```toml
[transfer]
enabled = false              # true: take a proxy's player-* requests; needs Velocity forwarding (forwarding.secret)
expect_seconds = 10          # how long a player the proxy announced is waited for, when it names no time
max_expect_seconds = 60      # the longest the proxy may ask for
max_expected = 64            # players waited for at once
max_freeze_seconds = 30      # the longest a player stays frozen while it is handed over
max_hold_seconds = 60        # the longest a player arriving from another server is held before it is dropped
stop_wait_seconds = 10       # a stop with players on waits this long for the proxy to move them out (0 to 60; 0: no wait)
```

The supervisor passes the section to the engine as `--transfers expect_seconds=10,...`, beside
`--control-socket`, and not as a `server.properties` key: that file stays vanilla's. The section has nothing to do
with vanilla's `accepts-transfers`, which is about the transfer packet a server sends a client. While it is off,
every `player-*` request and `transfer-status` are answered `{"error": "transfers are off on this server"}`, the
engine keeps no table and does nothing for transfers on a tick.

With it on, the engine refuses to start unless Velocity forwarding is on (`forwarding.secret`,
[velocity-forwarding.md](velocity-forwarding.md)). The proxy must be the only way in: a player who joined a
source server directly would find there the older copy of itself that a hand-over left behind, and items would
double.

**Who can ask.** The requests do not widen who can reach the engine: they use the control endpoint, a Unix socket
or TCP on 127.0.0.1 only (horizon-server starts it on TCP with a token). A proxy on another machine needs
something local to each engine to relay for it. The token is all or nothing: whoever can move players can also
run any console command.

Answers are one JSON line, `{"ok": true, ...}` or `{"error": "..."}`, and a refused request changes nothing. A
unique id is written as 32 hexadecimal digits with or without dashes in a request, and as lowercase text with
dashes in an answer (`123e4567-e89b-12d3-a456-426614174000`). A hand-over id is 32 lowercase hexadecimal digits.

`transfer-status` is what a proxy reads before it sends players to a server. It names no player and changes nothing:

```json
{"ok": true, "players": 12, "held": 1, "expected": 2, "frozen": 0, "online": 13, "max_players": 20, "full": false, "stopping": false}
```

`players` are the players placed in the world (a player a `moved` release took out is no longer counted), `held`
the logins held for a hand-over, `expected` the expectations still alive, and `frozen` the players this server is
handing over. `online` is what the player limit counts: the placed and the held players and the logins still
joining. `full` is `online` at `max_players` or above: an ordinary login, held or not, is then refused as
`multiplayer.disconnect.server_full`, unless the player is an operator with `bypassesPlayerLimit`. `player-expect`
counts the expected players too: it is refused once `online` and `expected` reach `max_players`. `stopping` is true
once the server has begun to stop, and no player is taken in any more. A proxy asks it often, so it writes nothing to
the log.

`player-expect <uuid> [seconds]` tells the target server that the player is about to log in: within
`expect_seconds` by default, `max_expect_seconds` at most (a longer time is cut to it), and at most
`max_expected` players are expected at once. The reply is `{"ok": true, "renewed": false, "ttl_seconds": 10}`;
asking again for the same player renews the time and says `"renewed": true`. It is refused for a missing or
malformed id, a time that is not a whole number of seconds above zero, a player already connected, a full table,
a full server (`{"error": "the server is full"}`) and a server that is stopping (`{"error": "the server is
stopping"}`). Each expectation holds a place until it is used up or runs out: once the places the player limit
counts and those of the other expected players reach `max-players`, a new one is refused as a login would be, so two
proxies sending players at once do not fill the server past its limit. As at login, an operator with
`bypassesPlayerLimit` is never refused as full. The places are held against other expectations only: an ordinary
login (one nobody expected) sees the vanilla limit alone, so it can take a place an expected player was counting on.
That player's login is then refused as `server_full`, and the proxy aborts the hand-over on the source, where the
player goes on as it was. `full` in `transfer-status` is likewise the vanilla condition alone. The player's next
login uses the expectation up as it starts, once the bans, the whitelist and the player limit let it in, so a login still in configuration when the time runs out (a resource pack prompt) keeps it. A
login that then fails before it reaches play (its connection drops in configuration, or the client refuses a
required pack) gives the expectation back with its own deadline, so the proxy's next attempt within that time is
still expected. An expectation no login used is reported in the log when its time runs out.

A login that used up an expectation is a *held* player. It gets the whole join sequence and the level around it
(chunks, block and light changes, entities, the level's broadcasts), and its acknowledgements are handled (teleport,
chunk batch, loaded, keep-alive, ping, tick end, client information), but it is not placed: no join message, no
player-list entry for the others, not among the entities they are shown, not simulated, and nothing else it sends is
acted on. The world is kept loaded, and generated if need be, around a held player as around a placed one, so a
server with nobody else on it sends a held player its chunks. The log says `<name> is held: it arrives from another
server` instead of the join line. A held player nobody takes over is dropped after `max_hold_seconds` (1,200 ticks
by default), with a warning, and its departure is logged as `<name> was dropped while held: <reason>`, with no "left
the game" line. `player-export` refuses a held player.

`player-drop <uuid>` forgets the player's expectation, and disconnects the held player without a save or a word to
anyone. The reply is `{"ok": true, "held": true, "expected": false}`: whether a held player was disconnected, and
whether an expectation was forgotten. It is refused for an id that is neither held nor expected, and leaves a placed
player alone.

`player-export <uuid> [freeze]` reads a connected player out of the source server. It first closes the player's open
menu as vanilla does when a menu closes, in the player's own level (the cursor stack, a crafting table's grid and
the inventory's own 2x2 grid go back into the inventory, a trade back to its villager; the client is told to close
it and is sent its inventory), then answers:

```json
{"ok": true, "record": {
  "version": 1,
  "data_version": 5023,
  "handover": "9f2c4be0a1d34e6b8c0f1e2d3c4b5a69",
  "uuid": "123e4567-e89b-12d3-a456-426614174000",
  "name": "Alex",
  "dimension": "minecraft:overworld",
  "position": [10.5, 70.0, -3.25],
  "rotation": [90.0, 15.0],
  "velocity": [0.0, 0.0, 0.0],
  "game_mode": 0,
  "player": "<base64>",
  "advancements": { },
  "stats": {"stats": { }, "DataVersion": 5023}
}}
```

`player` is the base64 of the uncompressed named NBT (root name empty) of the player compound: what the server
writes to `players/data/<uuid>.dat` before it gzips it, taken from the connected player at that moment, with the
values mods attached to the player. `advancements` and `stats` are the JSON the server writes to
`players/advancements/<uuid>.json` and `players/stats/<uuid>.json`, or null on a server that keeps none.
`version` is the version of this record (1); `data_version` is the world data version of the server that made it
(5023 for 26.3). The other fields are a few of the compound's values, for a proxy that does not read NBT. While the
player is frozen, the reply also gives `"freeze_ticks_left"` beside the record: the ticks before the hand-over ends
by itself, so that a proxy does not commit on the target too late for its release to come in time.

How a server reads a record (the target's `player-import`):
- A record of another `version`, or of another `data_version` than the server's own, is refused.
- The NBT in `player` wins over the plain fields: they are copies of its values, for a proxy.
- Fields the reader does not know are ignored, so a later version 1 record may carry more.

**The hand-over.** `freeze` starts one: the record gets a `handover` id, which the later requests name, and the
player's own inputs (moves, clicks, chat, commands) stop being acted on, while what answers the server (teleport
confirmations, chunk batch acknowledgements, keep-alives, pings, tick ends, client information and chat
acknowledgements, and the messages a chat packet says the client has seen, in the order they came) still is. A frozen player is not
kicked for floating. Every later export of that player, with or without `freeze`, answers with the same record,
byte for byte, and does not restart the freeze: a proxy that lost the reply asks again. The hand-over ends after
`max_freeze_seconds` (counted in ticks, 600 by default) with a warning in the log, and the id is then forgotten.
Without `freeze` the export is a read: `handover` is null and the player goes on. The request is refused for a
player who is not connected, is held, has not finished joining, is leaving, is riding, is asleep or is dead.

While frozen, the player takes no damage (as vanilla's `Abilities.invulnerable`: the void and `/kill` still kill)
and picks nothing up (items, arrows, experience orbs), so its record stays what it is until the release. A player
who dies while frozen all the same ends the hand-over, with a warning in the log.

`player-release <uuid> <hand-over> moved [announce]` and `player-release <uuid> <hand-over> abort` end the
hand-over, on the source server. Both name the hand-over the export returned. `moved` takes the player out of the
world because another server has it now: the others see its entity and its player-list entry go, nothing is written
to its player file or its records (the other server has the player), the log and the `leave` event record the
departure with the reason `moved to another server`, and the player's connection is closed with a generic
disconnect. The "left the game" line is neither sent nor logged unless `announce` is added. Nothing is deleted: the
player's file here stays as it was. `abort` lifts the freeze, and the player goes on as if nothing had happened.
`moved` replies `{"ok": true, "released": "moved", "announced": false}` and `abort` replies
`{"ok": true, "released": "abort"}`. Both are refused for a player who is not in a hand-over, for another hand-over
than the player's current one, and for a player who died while frozen. Once the freeze has run out the hand-over is
over, and a late `moved` is refused: the player has played on since, and the record the other server has is older.
The proxy then fails the hand-over on the target (`player-drop`).

`player-import <uuid> <json>` applies a record to the held player `uuid` on the target server, whole or in parts,
and lets it into the world:

```json
{"record": { ...the record of player-export... }, "parts": ["inventory", "experience"], "announce": true, "place": true}
```

`parts` is optional (all of them by default), `announce` is optional (true by default) and `place` is optional (true
by default; see below). The parts, in the order the reply lists them, and what each sets:

| part | sets |
| --- | --- |
| `inventory` | the inventory, armour, off hand and selected slot |
| `ender_chest` | the ender chest |
| `experience` | the level, progress, total and seed |
| `health` | health and absorption, food, saturation, exhaustion and its timer, and the body's air, fire and freezing |
| `effects` | the active effects |
| `game_mode` | the game mode, flight and flight speed |
| `position` | where the player stands, how it looks, whether it is on the ground, its fall and motion, and the vehicle it rode |
| `spawn_point` | the respawn point |
| `recipe_book` | the recipe book |
| `advancements` | the advancements and the statistics |

A part that is not asked for keeps what the held player has, which is what this server loaded for it when it logged
in: its own file, or a new player's defaults. An imported inventory or ender chest replaces the held player's whole: a
slot the record leaves empty is empty afterwards. The keys of the compound that Horizon does not model, the values
mods attached to the player, and the last death, the post effects and the credits, are taken only when every part
is. Only the compound of the record is read: the fields above it (`position`, `rotation`, `velocity`, `dimension`)
are for whoever reads the record.

A record is refused, with `{"error": "..."}` and nothing changed, when its `version` is not 1, its `data_version` is
not this server's, its `uuid` is not the request's, it has no `handover` id (an export without `freeze`), its
compound is not valid base64 NBT, a part's name is unknown, or the player is not held or is leaving. It is refused
too when its values are ones no player can have: a position, rotation, motion, health, absorption, saturation,
experience progress or fall that is not a number, a position farther than 30,000,512 blocks from the origin (20,000,000
up or down), which vanilla's `Entity.load` would clamp, a health of zero or less, a health over 1,024 or an absorption
over 2,048 (the bounds of `max_health` and `max_absorption`), a food exhaustion over 40 (`FoodData.addExhaustion`'s)
or an effect amplifier outside 0 to 255. A stack's count out of vanilla's 1 to 99 is read as 1, as for any item. A record whose position is in
another dimension than the held player's world is refused when `position` is among the parts: the client is in one
dimension and only a login changes it.

The record's hand-over is bound to the held player at the import: a second import of the same record is taken (a
retry), one of another hand-over is refused, and `player-confirm` and `player-drop` name it.

**The commit point.** On success the player is sent what the import changed the way a join sends it (game mode,
abilities, held slot, a new position to confirm, the effects, with those it had from this server's own file removed,
inventory, recipe book and the player list as it now is). Then, unless `"place": false`, it is placed as
`player-confirm` places it: its player file, advancements and statistics are written to disk first, each to a new
file that is synced and then renamed into place (on Linux and macOS the folder is synced too), then it is placed: simulated, shown to the others and taking part from the next tick, when its first tick sends its
health, experience and advancements as a new player's does. Everyone gets its player-list entry, since everyone will
be shown its entity; with `announce` true they also get the join line. The log says `<name> placed: it has arrived
from another server`, and the join line when it is announced. The reply is `{"ok": true, "handover": "...", "parts":
[...], "placed": true, "announced": true, "recentered": false}` only once the file is written, so a proxy sends `player-release moved` to
the source after this reply: from then on a crash of the target, or a power loss, leaves the player's three files there
as it arrived. When one cannot be written, the reply is an error and the player stays held and imported, to be
confirmed again or dropped.

**A lost reply after the commit.** A `player-import` or `player-confirm` that names the hand-over of a player already
placed from it is answered `{"ok": true, "placed": true, ..., "retried": true}`, with nothing done again: a proxy that
lost the reply asks again and learns the commit happened.

**A refused `moved` after the commit.** The target then has the player placed and on disk, and nothing undoes that:
`player-drop` takes only a held player. It happens when the freeze ran out between the confirm and the release (the
proxy was late), or when an operator's `/kill` killed the frozen player (nothing else hurts it, and it does not
move). The proxy keeps its client on the target and disconnects the source's connection; the source then saves its
copy of the player as it is, so the two servers' files differ from that moment. To keep clear of the first case, the
proxy confirms only while the export's `freeze_ticks_left` leaves it time to release, and otherwise drops the arrival
and aborts.

**Where the player arrives.** Only the compound of the record is read, so to put the player somewhere other than the
record says (a proxy that knows where its client stands, which the frozen source may not have seen yet), the request
takes four optional fields, which win over the compound:

```json
{"record": {...}, "position": [300.5, 64.0, 300.5], "rotation": [90.0, 20.0], "on_ground": false, "velocity": [0.0, -0.08, 0.0]}
```

`position` is `[x, y, z]`, `rotation` is `[yaw, pitch]` in degrees, `on_ground` is a boolean and `velocity` is
`[x, y, z]` in blocks a tick. The pitch is taken as vanilla's `Entity.setXRot` takes it: modulo 360, then cut to ±90;
the yaw is kept as it is. A velocity component over 10 blocks a tick is taken as 0, as `Entity.load` takes a saved
`Motion`. The velocity is the server's: the arrival's teleport carries no motion, as a vanilla login's does not, so
the client starts still, and since a player's client moves itself, its first move replaces it. They belong to the `position` part: a
request that gives one and does not import that part is refused. A position that is not finite, or farther than the
record's own may be (30,000,512 blocks from the origin, 20,000,000 up or down), is refused, and so is a rotation or a
velocity that is not finite numbers (a rotation past what a float holds too). A record with a vehicle still to restore
(`RootVehicle`) puts the player back on it at the vehicle's own place, so a request that names a position for such a
record is refused. The border, the blocks and the chunks are not checked,
as a vanilla server does not check the place its player file names. The player is teleported to the position (the
client confirms a new teleport id), so an anticheat never sees the arrival as a move, however far it is from the
export.

**The view follows the imported position.** `recentered` says whether the import moved the player to another chunk
than the one its view was built around (only possible with the `position` part, and only once the player has been sent
chunks). When it did, the server moves the view at once, before the position the client confirms: it sends
`SetChunkCacheCenter` for the new chunk and forgets the chunks that are out of reach, as the next tick would have. The
chunks around the new place are **not** sent yet: they come one batch at a time with the next ticks, and the client
acknowledges each batch. Whoever reads the player's world (a proxy building a view of it) must therefore wait for new
chunk batches after a reply with `"recentered": true`, as it waited for the first ones; with `false` the chunks it
already has are the ones around the player.

**Importing in two steps, so that a hand-over can be cancelled.** With `"place": false` the record is applied and the
player's client is sent everything the import changed, but the player stays held: not placed, not simulated, not
shown, not announced, and nothing is written. The reply is `{"ok": true, "handover": "...", "parts": [...], "placed":
false, "announced": <what announce asked>}`. Then either:

- `player-confirm <uuid> <hand-over>` writes the player's files and places it as `place: true` would have, and
  announces it if the import's `announce` said so. What the client did since the import was not acted on, so it is
  sent its position to confirm again and its inventory. Reply `{"ok": true, "placed": true, "announced": true}`. It is
  refused, with nothing changed, for a player who is not held, is leaving, whose record was not imported, or whose
  record is of another hand-over.
- `player-drop <uuid> [hand-over]` cancels the arrival: the connection is closed and nothing is saved or said to
  anyone (the player was never placed, so the world never knew it). Reply `{"ok": true, "held": true, "expected":
  false}`. With a hand-over id, it must be the imported record's, so a late drop of an older hand-over leaves a newer
  one alone; before any import there is no id to name, and the drop goes by the unique id alone (a late drop of
  that kind can still cut a newer held player that was not imported yet, which loses nothing: the source still has
  the player frozen, and the proxy aborts there). Once dropped, the player can be neither imported nor confirmed.

A proxy that fails after a successful `player-import` with `place: false`, and before it has sent its client the first
byte of the hand-over, calls `player-drop` and the target is as it was. A proxy that dies in between leaves a held
player that is dropped by itself after `max_hold_seconds`, with a warning in the log.

**A hand-over, in order.** A proxy moving a player from server A to server B, without a loading screen. Both run
with `[transfer]` on and behind the proxy's Velocity forwarding:

1. `player-expect <uuid>` on B: B holds the player's next login.
2. `player-export <uuid> freeze` on A: the record and its `handover` id, and A stops acting on the player's inputs.
   A lost reply is asked again: the same record comes back.
3. The proxy logs the player in to B on its own connection: B sends the held player the world around the place B
   loaded for it.
4. `player-import <uuid> <json>` on B with `"place": false` and, if the proxy knows where its client stands, the
   request's `position`, `rotation`, `on_ground` and `velocity`. With `"recentered": true` the proxy waits for B's
   next chunk batches.
5. The proxy switches its client over to B's connection. If anything fails before the client got the first byte of
   it, `player-drop <uuid> <hand-over>` on B and `player-release <uuid> <hand-over> abort` on A, and the player goes on
   on A as before.
6. `player-confirm <uuid> <hand-over>` on B: B writes the player's files, then places it. Only after its `ok`,
   `player-release <uuid> <hand-over> moved` on A: the player is out of A's world, and A saves nothing of it.

Every step that is not taken in time is undone by itself: an expectation after `expect_seconds`, a held player after
`max_hold_seconds`, a freeze after `max_freeze_seconds`, each with a warning in the log. A `moved` that comes after
the freeze ran out is refused, so the proxy must confirm and release within `max_freeze_seconds` of the export.

**A server that stops.** From the `/stop` command (the console's, a player's or the control endpoint's) or a stop
signal on, `player-expect`, `player-import` and `player-confirm` are refused `{"error": "the server is stopping"}`: a
player must not arrive on a server that is about to close. The proxy drops the held player there and aborts on the
source. A retry after the commit is still answered, and `player-export`, `player-release` and `player-drop` stay
open: they are the way out.

With players placed, the stop then waits `stop_wait_seconds` (10 by default; 0 stops at once, as vanilla does) so a
proxy can move them out first. A proxy that moves them one at a time, as Meridian's `evacuate` does, takes about
2 s a player over a real network (1.4 to 2.3 s measured): 10 s is room for about four, and a fuller server needs
more (60 at most, about 25 players). The log says `Stopping in 10 s: a proxy may move the players out first`, and
horizon-server sends `server/stopping` on the management protocol then, for a stop from the game as for its own.
During the wait every login is refused as `multiplayer.disconnect.server_shutdown`, and the server ticks on and
answers the proxy. The wait ends as soon as no player is placed, at `stop_wait_seconds` when no hand-over is frozen,
and at the latest a freeze's length later, so a hand-over started in time finishes (the `N s` of the log line is
then longer); the whole wait is cut at 90 s, which leaves the saves time before horizon-server kills a game that has
not stopped within 120 s, so `stop_wait_seconds` and `max_freeze_seconds` may add up to 90 at most when the stop
waits. A freeze lasts `max_freeze_seconds` worth of ticks (20 a second), so its length in time is reckoned at the
tick length of the moment, the rate's or longer when the server is slower than its rate: at 10 ticks a second it
lasts twice as long. `player-export ... freeze` is refused (`no hand-over starts this late`) once a freeze that
starts then would end past the limit, unless the player is frozen already: a hand-over the limit cut would leave
the player saved by the source after the target placed it. That holds at the tick rate of the moment the freeze
starts: a server that slows down after it can still reach the limit with it. A hand-over still frozen at the limit
is logged as a warning, and its player saved here. Then the stop goes on as vanilla's (`Stopping server`, the
saves, the kicks). A second `stop` during the wait stops at once, with the saves; a second stop signal ends the
process, as without `[transfer]`.

**In the log.** Each `player-*` request writes one line with what became of it: `INFO` and what it did
(`player-import Alex (<uuid>): ok, parts inventory,experience, placed false, announced true, recentered false,
hand-over <id>`), or `WARN` and the reason it was refused (`player-export Alex (<uuid>): refused: that player is
riding`). The hand-over's id is in the lines of the export, the import, the confirm, the release and a named drop, so
the lines of one hand-over can be found on both servers. Only the ids, the name when the server knows it, and a few
words of the outcome are written: never the record or the request's body, which are large and are the player's. A
held player has its own lines instead of the join and leave ones: `<name> is held: it arrives from another server`,
`<name> placed: it has arrived from another server` and `<name> was dropped while held: <reason>`.

**The headless engine** comes after perf-ticks-2's work lands: Resources split into shared and per-game, the
game built in `Game::new`, the integrated server on its own thread. On that base:

1. The dedicated path skips everything only a window uses: the HUD atlas, item renderer, block-entity and
   entity models, panorama, fog, music, and mesh tables (perf-ticks-2 already stops server-side meshing).
   This is the RAM and start-time item.
2. The game moves into a library crate, with the window parts behind a `client` feature, so
   `horizon-engine` links no wgpu or winit either. Sound and controllers are already optional (the
   `audio` and `controllers` features, on by default): `horizon-engine` today is the game built without
   them (section 3).

## 3. Packaging

**Binaries.** The release archives hold `horizon-client`, `horizon-ctl`, `horizon-prepare` and
`horizon-server`. On Linux the release also has `horizon-server-linux-x86_64.tar.gz`: `horizon-server`,
`horizon-prepare` and `horizon-engine`, the game built with `--no-default-features` (no sound, no
controllers, no Discord presence, Play with Friends or "I'm playing" ping: the `discord`, `friends` and
`heartbeat` features), which links neither `libasound.so.2` nor `libudev.so.1`. `release.yml` builds it after the
client archive (`xtask package-server`) and fails if `tools/check_imports.py --server` finds either library;
CI lints the same build. `horizon-server` runs `horizon-engine` when it is beside it or on the PATH, else
`horizon-client`. Build it yourself with `cargo build --release -p horizon-client --no-default-features`
and copy `target/release/horizon-client` as `horizon-engine`. macOS and Windows have no such libraries to
avoid, so they keep the one archive.

**Game files: the owner's own copy, as with `prepare`.** Nothing of Mojang's is redistributed and nothing is
downloaded on the owner's behalf. `horizon-server` looks for prepared resources in this order:

1. `[engine] resources` in `server.toml`;
2. `resources/` in the server folder;
3. `.local/horizon-client/26.3` beside the engine or up to four folders above it: a release folder, the
   installer's `game-files` (below), or a checkout.

If none is found, `init` says where it looked. Today the owner runs `horizon-prepare --prepare --server-jar
PATH` from the release folder (it needs a JDK and the client files, as for playing), or copies a folder
prepared on their own computer to the server (`scp -r`). Planned wrappers:

- **On this machine:** `horizon-server prepare --server-jar PATH` runs `horizon-prepare` on the owner's own
  server jar plus their launcher install.
- **Prepared elsewhere:** `horizon-server import-resources DIR` copies a folder prepared on the owner's own
  computer, for example by the launcher, to a headless machine. It first checks the folder: `horizon-resources.json`
  is there and names Minecraft 26.3, the derived files match their checksums, and the font and texture folders
  aren't empty; otherwise it lists what is wrong and changes nothing. It copies next to the destination and
  renames the copy in, so a failure partway keeps the old copy. The destination is the first place the server
  reads: `[engine] resources` when set, else `resources/` in the server folder when it exists or the server
  wasn't installed by `install.sh`, else the installer's `game-files/horizon-client/26.3` (where the release's
  `.local/horizon-client` points). It ends by saying to start or restart the server.

A server-only `prepare` (from `server.jar` alone, without the client jar or a JDK) is the follow-up that
removes the last Java step. It needs a list of which prepared files the server really reads, and that list
comes out of the headless-engine work.

**One-command install** (`packaging/server/`):

- Linux and macOS:
  `curl -fsSL https://raw.githubusercontent.com/horizon-rust/horizon/main/packaging/server/install.sh | sh`
  The script picks the release archive for the OS and architecture (on Linux x86_64 the server archive,
  on macOS arm64 the game's), downloads it from GitHub Releases, and checks it against the release's signed
  `SHA256SUMS` when OpenSSL 3 is installed, else against the SHA-256 GitHub publishes for each asset
  (docs/security.md). It never asks for sudo. Options: `--version vX.Y.Z`, `--dir` (default
  `~/.local/share/horizon`), `--bin` (default `~/.local/bin`), and `--archive FILE` to install an archive
  already on the machine (a build of your own; no download and no checksum).
  A Linux release from before the server archive installs the full game, which links ALSA's and udev's
  libraries (`libasound.so.2`, `libudev.so.1`); minimal server images often lack them (Debian and Ubuntu:
  `sudo apt install libasound2 libudev1`, `libasound2t64` on Ubuntu 24.04 and later). Then the script lists
  any that are missing, `init` warns, and `run` stops at once with what to install, rather than restarting
  a game that can't load.
- Windows:
  `irm https://raw.githubusercontent.com/horizon-rust/horizon/main/packaging/server/install.ps1 | iex`
  The script does the same into `%LOCALAPPDATA%\Horizon` (`-Version`, `-Dir`), with junctions instead of
  links (no admin rights), and adds `bin` to the user's PATH. It finds OpenSSL 3 on the PATH or in Git for
  Windows to check the signature.
- **Layout.** Each release goes to `<dir>/releases/<tag>` and `<dir>/current` points at the one in use, so an
  update or a rollback is one link. `<bin>/horizon-server` and `<bin>/horizon-prepare` are two-line launchers
  that run `current`. The prepared game files live in `<dir>/game-files/horizon-client` and each release's
  `.local/horizon-client` links there, so updates keep them. The script then prints the next steps
  (`horizon-prepare`, `horizon-server init`).

**Docker** (`packaging/server/Dockerfile`, built the same way as the relay's image): a multi-stage build that
ends on `debian:bookworm-slim` as user `horizon`, with the server build as `horizon-engine`, so the image needs
no ALSA or udev. It uses volume `/data` for the server folder, takes the owner's prepared game files as a read-only
mount on `/opt/horizon/.local/horizon-client/26.3`, exposes port `25565/tcp`, and keeps the control API and
the panel on 127.0.0.1 inside the container: `docker exec <name> horizon-server status|cmd|backup`.
On the first start, `HORIZON_EULA=true` and `HORIZON_WORLD`, `HORIZON_SEED`, `HORIZON_PORT`,
`HORIZON_WHITELIST`, `HORIZON_PLAYERS` and `HORIZON_OP` answer `init`'s questions; without the EULA answer
the container exits and says why. `docker stop` sends SIGTERM, which saves and stops cleanly (give it
`--stop-timeout 180`). The image is published only with Ludovic's OK (it's hosted infrastructure).

**Services.**

- `horizon-server service install` writes and enables a systemd unit (`--user`, or a system unit with
  `--system`) on Linux, a launchd agent (`~/Library/LaunchAgents/dev.horizon.server.<name>.plist`) on
  macOS, or a Scheduled Task at logon on Windows (a real Windows service needs a wrapper and comes later).
- Each runs `horizon-server run --dir <folder>` through the installer's launcher, so an update applies at the
  next restart. The supervisor restarts the engine itself, and the service manager restarts the supervisor.
  `service uninstall` stops and removes it and leaves the server folder alone.
- `service print` shows the unit or agent it would write, for owners who'd rather install it by hand.

**First run (`horizon-server init`, interactive on a terminal):**

1. **EULA.** It shows the link to https://aka.ms/MinecraftEULA and asks "Do you agree? [y/N]". Only a yes
   writes `eula=true`.
2. **World.** It asks for the world name (`world`) and an optional seed. An existing world folder, or a
   vanilla server folder, is detected and kept: its `server.properties` is imported (section 4).
3. **Port.** 25565 by default, checked free right away.
4. **Players.** It asks whether to use a whitelist (yes by default, as vanilla) and who to add first, then
   offers to make the first player an operator.
5. **Game files.** It finds or explains the prepared resources (see above).
6. It writes `server.toml`, shows the panel address and the commands to start it in the background
   (`horizon-server start`) or as a service.

Every question has a flag (`--eula`, `--world`, `--seed`, `--port`, `--whitelist`/`--no-whitelist`,
`--player NAME`, `--op NAME`, `--yes`) and an environment variable for Docker (`HORIZON_EULA`, …; flags win).

## 4. One config file: `server.toml`

The owner edits only `server.toml`. The supervisor writes `server.properties` from it before every engine
start, so the engine stays exactly vanilla. The file is short: typed sections for what owners change, and
a `[properties]` table that passes any other vanilla key through unchanged.

```toml
# Horizon server settings (docs/server.md). The server writes them into server.properties before
# every start: edit this file, not that one.

[server]
motd = "A Minecraft Server"
port = 25565
bind = ""                    # listen on every address, or only on this one (server-ip)
max_players = 20
online_mode = true           # check players' Minecraft accounts (false: anyone joins under any name)
whitelist = true             # only whitelisted players and operators can join
enforce_whitelist = false    # kick online players taken off the whitelist
view_distance = 10
simulation_distance = 10

[world]
name = "world"               # the world folder
seed = ""                    # empty: a random seed on the first start
game_mode = "survival"       # survival, creative, adventure or spectator
difficulty = "easy"          # peaceful, easy, normal or hard
hardcore = false

# Any other server.properties key, spelled as vanilla spells it, for example:
# spawn-protection = 16
[properties]

[engine]
path = ""                    # the game program; empty: the one installed beside horizon-server
resources = ""               # the prepared game files; empty: resources/ here, else the usual places
restart_on_crash = true

[backups]
every = "6h"                 # automatic backups while the server runs: 30m, 6h, 1d...; "0" turns them off
keep = 12                    # automatic backups kept (backups made by hand are never deleted)

[updates]
keep = 1                     # other releases kept by updates, besides the one in use and the one rollback goes to
```

This is the file `init` writes today, and it also writes `[panel]` (`bind = "127.0.0.1:8130"` and
`allow_public = false`, section 6), `[api]`, `[transfer]` (off; a proxy's hand-overs, section 2) and `[discord]`. The defaults are vanilla 26.3's (a test checks
each one). Unknown keys are errors, as are a typed key set again under `[properties]` and values vanilla would
refuse. There is no `backups.before_update` setting: an update always backs up the world first (section 7).

- **Import.** `horizon-server init` in a folder with a `server.properties`, or `horizon-server import
  server.properties`, reads it with vanilla's own parser (`ServerProperties`). Keys with typed fields go into
  their sections; any other key that differs from vanilla's default goes into `[properties]`. Keys left at
  their defaults are dropped, so the TOML stays short. The original file is kept as
  `server.properties.imported`.
- **Changes made in game.** The engine writes `server.properties` itself, for example after `/whitelist on`.
  After it stops, the supervisor compares the file with what it wrote and copies any changed keys back into
  `server.toml`, so an in-game change survives the next start. Edits go through `toml_edit`, so the owner's
  comments and order are kept.
- **Changes without opening the file.** `horizon-server config set KEY=VALUE...` (the launcher's Servers
  page uses it) writes through `toml_edit`, checks the result as `server.toml` is checked, and writes nothing
  if a value is wrong. On a running server it sends what vanilla changes live through its console:
  `difficulty`, `defaultgamemode` and `whitelist on|off`; `server.motd` and `server.max_players` go to the
  game as the management protocol's `serversettings/motd/set` and `serversettings/max_players/set` do, so
  the server list and the next logins use them at once; the backup settings apply at once because the
  supervisor reads them each time. Everything else waits for the next start, and the command says which. A
  hot reload of the view distance needs engine support and comes later.
- `server.properties`, `ops.json`, `whitelist.json`, `banned-*.json` and `world/` keep vanilla's names and
  formats. A vanilla server folder works as is, and so does going back to vanilla.

## 5. CLI and TUI

```
horizon-server init                      first run (section 3)
horizon-server run                       supervisor + engine in the foreground; the console on stdin
horizon-server start | stop | restart    in the background (detached `run`); start also resumes a stopped supervisor
horizon-server status [--json]           up?, uptime, TPS, MSPT, players, RAM, CPU, crashes
horizon-server cmd <command…>            one console command, with its output
horizon-server console                   attach: the live log, and send commands (Ctrl-D detaches)
horizon-server logs [-f] [-n N]
horizon-server op|deop <player>
horizon-server whitelist on|off|list|add|remove [player]
horizon-server players [--json]          who is online, operators, whitelist, bans
horizon-server kick|ban <player> [reason]   pardon <player>   ban-ip <address|player> [reason]   pardon-ip <address>
horizon-server backup [NAME]             backups             restore <backup> [--yes]
horizon-server import <server.properties>
horizon-server config [--json]           config set KEY=VALUE... [--json]
horizon-server service install|uninstall|print [--system]
horizon-server panel                     the panel's address and a one-time login link
horizon-server update [--check] [--version vX.Y.Z] [--timeout S]     rollback [--timeout S]
horizon-server update --archive FILE --version NAME     an archive you have (your own build)
horizon-server mcp                       MCP server on stdio (section 8)
horizon-server import-resources <dir>    copy game files prepared elsewhere into place (checked first)

Planned:
horizon-server prepare --server-jar PATH
```

- Every command takes `--dir` (the server folder; default: the current folder).
- While the server runs, management commands go through the control API, so the engine applies them as
  vanilla's console would and its lists stay right. While it's stopped, `op`, `deop`, `whitelist`, `ban`,
  `pardon`, `ban-ip` and `pardon-ip` edit the vanilla files directly with the same code the engine uses
  (`horizon_server_files::user_lists`).
- **The TUI** is `horizon-server console` (built, `console.rs`): the recent log and then the live one (the
  control API's event stream), a one-line header (TPS, MSPT, players, RAM) that updates
  every second, and a command line. It uses plain ANSI with no TUI framework, and falls back to plain lines
  when stdout isn't a terminal.

## 6. Web panel

**Scope (MVP, PR 4):** status (phase, uptime, version, crashes), live TPS, MSPT, RAM and CPU graphs (the last
10 minutes, sampled once a second), players (list with ping, op, kick), the console (live log plus a command
line), and backups (list, back up now, restore with confirmation), with restart and stop. The panel lives in
the supervisor, so it is up exactly when the server is: starting a stopped server stays a command (or the
service manager's job). Whitelist and bans go through the console for now.

**Updates:** when a newer release is out, the panel says so with an Update button, which runs the same
path as `horizon-server update` (section 7) and then shows how it went.

**Later:** bans and IP bans, worlds, `server.properties` import, logs and crash reports, and modpacks (with
the modpack work).

**Tech:** one HTML page with plain JS and CSS (`crates/horizon-server/panel/`), embedded in the binary with
`include_str!`. No build step, no framework, no CDN. It uses Horizon's design system tokens (`horizon-theme`'s
colours, light and dark) and draws charts on a canvas. It polls the control API's routes once a second
(`status`, `metrics?since=`, `logs?since=`), served by the same small HTTP code on `std::net`, with no async
runtime or new dependency. Server-sent events can replace the polling later if a tab ever costs too much.

**Auth: local or tailnet only, never an open admin port.**

- **Bind.** The panel listens on `127.0.0.1:8130` by default. `panel.bind` can name a tailnet address
  (100.64.0.0/10 or Tailscale's fd7a:115c:a1e0::/48) or another loopback address. Any other address, such as
  0.0.0.0 or a public IP, is refused unless `panel.allow_public = true` is set, and even then the panel logs
  a warning at every start.
- **Who may act.** The API token in `.horizon/api.json` (file mode 0600) is what the CLI and MCP use. Browsers
  get a one-time login link from `horizon-server panel`: a code good for 10 minutes, swapped for an
  `HttpOnly; SameSite=Strict` session cookie. Anyone who can read the server folder can manage the server,
  and no one else can. Sessions end when the supervisor stops.
- **Protection.** The `Host` header must name the panel itself: its address, `localhost` on loopback, or a
  tailnet name (a MagicDNS short name or `*.ts.net`); bound to every interface (`0.0.0.0` or `::`), any IP
  address with the panel's port. That stops DNS rebinding. Changes (POST, PATCH, DELETE) need
  the `X-Horizon: 1` header, which stops CSRF. After 10 failed logins in a minute, logins are refused for a
  while. The page is served with a strict Content-Security-Policy (nothing from elsewhere) and can't be
  framed.
- **Remote access** goes over Tailscale/SSH (`ssh -L 8130:127.0.0.1:8130 host`), with no TLS or passwords to
  manage. HTTPS comes for free with `tailscale serve`, and the doc shows how to set it up.

## 7. Backups, updates and crash restart

- **Backups.** The supervisor runs `save-off`, then `save-all flush`, then zips the world plus the vanilla
  files and `server.toml` into `backups/<UTC time>-<name>.zip`, then runs `save-on`. Players stay online. Zip
  works for everyone, Windows included. It runs on the `every` schedule, on demand and before an
  update. It keeps the last `keep` automatic backups and never deletes one made by hand. A backup is written
  to a temporary name and renamed when complete, so a crash never leaves a half-written backup that looks
  finished. A stopped server is backed up straight from its files.
- **Restore.** `restore NAME` takes a file name, a unique prefix, or the name given to `backup` (the newest
  with it). The server stops. The current world is moved to `backups/replaced-<time>/` (never deleted), the
  backup is unpacked into a temporary folder and checked (`level.dat` is there), then moved in, and the server
  starts again if it was running. One command, or one click with confirmation in the panel. `restore NAME
  --dimension minecraft:the_nether` (API: `dimension` in the body; MCP: `dimension`) brings back only that
  dimension's folder (`dimensions/<namespace>/<path>`), the same way, and leaves the other dimensions,
  `level.dat` and the players as they are; the folder it replaces goes to `backups/replaced-<time>/`.
- **Stopped game.** A `stop` that reaches the game in game, over RCON or through `horizon-server cmd` leaves the
  supervisor, control API and panel alive with no engine process; `stop` on the supervisor's own stdin (`run`, a
  hosting panel's console) still ends both, and while the game is stopped that stdin takes `start` and `stop`.
  `horizon-server start` or `POST /api/v1/start` starts the game again; `horizon-server stop` and the management
  protocol's `server/stop` (as vanilla's, the process exits) still shut down both the game and the supervisor. File-backed player-list edits are safe during this stopped state.
- **Crash restart.** If the engine exits without being asked to stop, the supervisor keeps the last 200 log
  lines and the engine's crash report (`crash-reports/`), restarts it after 5 s, and doubles the wait up to
  5 minutes. It gives up after 5 crashes in 10 minutes, so it doesn't loop on a broken world, and shows the
  reason in `status` and the panel.
- **Updates** (`crates/horizon-server/src/update.rs`), for a `horizon-server` installed by `install.sh` or
  `install.ps1`. Releases ship often (release-flow.md), so updates stay opt-in: a running server looks for a
  newer release a minute after it starts and then every 6 hours, and only says so, in its log, in `status`
  and in the panel (`HORIZON_NO_UPDATE_CHECK=1` turns the check off). `update --check` asks GitHub once.
  `update`:
  1. downloads the current Nightly's archive for this computer (the only build Horizon ships since 2026-10-28;
     GitHub's latest release while there is none; on Linux the server build; `--version` picks another: a
     `vX.Y.Z` release, or a `nightly-<date>` of the last 7 days) and checks it against its SHA256SUMS, whose Ed25519
     signature must check with a key built into `horizon-server` (docs/release-flow.md, "Signed checksums"),
     signed for the Nightly channel: an unsigned or tampered build is refused. A Nightly installs as
     `nightly-<date>-<commit>`, newer than every `vX.Y.Z`; another build of the same day is an update too;
  2. unpacks it into `<dir>/releases/<tag>`, beside the others, with its game files linked to
     `<dir>/game-files` as the installers do;
  3. stops the game, backs up the world (`backups/<time>-before-<tag>.zip`), records the release in use in
     `<dir>/previous` and points `<dir>/current` at the new one;
  4. checks that the new game loads on this system (`--help`), then starts it;
  5. waits until it has logged `Done` and answered a status request, for 300 s (`--timeout`).

  If the new game doesn't load, stops, or isn't up in time, the supervisor goes back by itself: `current`
  points at the release before, `previous` is left as it was, the world is put back from the backup (the
  one the new game left is kept in `backups/replaced-<time>/`), and the game starts again on the old
  release. `rollback` switches to `previous` by hand, through the same path (the world is backed up first
  and kept as it is). The supervisor keeps running as it was: the new `horizon-server` itself takes over
  the next time the server starts (`stop` and `start`, or the service's restart). A stopped server is
  switched and its new game checked to load; it starts on the new release next time. One install serves
  every server folder on the machine, so each one runs the new release from its next start. Once the new
  game is up (or a stopped server is switched), old releases are deleted from `<dir>/releases/`: the one in
  use, the one `previous` names (where `rollback` goes), the one the running `horizon-server` itself came
  from, and the `updates.keep` newest of the others (1 by default) stay. A release that can't be deleted (a
  program still running from it on Windows) is left for the next update.

## 8. Control API and MCP

- **Control API.** The control API is the panel's HTTP API, documented as stable (`/api/v1/...`). The
  supervisor serves it on `127.0.0.1` at a port of the system's choosing, written with the token to
  `.horizon/api.json`. It's plain HTTP/1.1 on `std::net` (`crates/horizon-server/src/http.rs`), no runtime.
  - built (PR 2b, 2c): `GET status` (phase, uptime, crashes, RSS, CPU, and the engine's TPS, MSPT
    average/P50/P95/P99, players, chunks), `GET logs?since=`, `POST command {line}` (returns the output),
    `POST start`, `POST stop`, `POST restart`, `GET backups`, `POST backups {name}`, `POST restore {name}`, `POST update
    {tag, timeout}` (without a tag: the newest release, downloaded first; `status` has an `update` object
    that says how it went), `GET metrics?since=`;
  - built (server-admin): `GET players` (online with address, ping, position and op level; ops, whitelist, bans, IP
    bans), `POST players/{kick,ban,pardon,ban-ip,pardon-ip,op,deop}`, `POST whitelist {enabled}`,
    `POST whitelist/{add,remove}` (list edits also work while the game is stopped; running edits use
    vanilla's console command and answer `changed` and its lines),
    `GET metrics/prometheus` (Prometheus text);
  - built (server-admin): `GET config` (the token hidden), `PATCH config {changes}` (as `config set`;
    not `engine.*`, `panel.*`, `api.*`, nor the RCON, query and management keys, so the API can't widen who
    reaches the server or change what program runs), `DELETE backups?name=`, `GET backups/download?name=`
    (the zip, streamed);
  - built (server-admin): `GET events` (server-sent events: log lines, the phase, and the engine's typed join,
    leave, kick, chat and death events, which the supervisor fetches with `server-events SINCE` on the
    engine's control endpoint every 250 ms; JSON when the client doesn't ask for a stream).
- **Auth.** Every call needs `Authorization: Bearer <token from .horizon/api.json>` (file mode 0600), and the
  `Host` header must name the address it listens on (as the panel's). `[api] bind` and `[api] token` fix the
  address and the token for a program that runs elsewhere (a moderation bot); `bind` takes loopback or a
  tailnet address unless `allow_public = true`, which drops the Host check and warns at every start.
- **MCP** (built, `mcp.rs`). `horizon-server mcp` is an MCP server on stdio, for Claude Code or any MCP client
  run as the same OS user. It reads the token itself, and its tools map one to one onto the API: `status`,
  `players`, `run_command`, `logs`, `events`, `kick`, `ban`, `pardon`, `ban_ip`, `pardon_ip`, `op`, `deop`,
  `whitelist`, `list_backups`, `backup`, `delete_backup` and `restore` (both need `confirm: true`), `get_config`,
  `set_config`, `start`, `restart` and `stop` (needs `confirm: true`). Tools that send someone away or can't be
  undone carry MCP's `destructiveHint`. It's the same idea as the game's `horizon-ctl`
  (docs/agent-control.md): an AI can run and diagnose a server, and every action it takes shows in the log.
- **RCON.** `enable-rcon`, `rcon.port` and `rcon.password` turn on vanilla's remote console, for the hosting panels,
  Discord bridges and scripts written for it. The supervisor serves it (`rcon.rs`, std only like `http.rs`), so it
  answers while the game restarts (a command then says the game isn't running), and it reads its settings as
  `RconThread.create` does: no password or a port outside 1 to 65535 leaves it off with vanilla's warning. The
  packets are `RconClient`'s: login (3), command (2), the output in packets of 4096 characters, `-1` for a refused
  login, `Unknown request <hex>` for another type, `Error executing: <command> (<reason>)` for a command that failed.
  A command goes to the game as an `rcon <line>` request on the control endpoint (`horizon-server cmd` sends
  `console <line>`, the same console under the `Server` source), so the
  output is what the console prints, its messages joined as `RconConsoleSource` joins them. Differences, each one
  named in `rcon.rs`: the packets are read from the stream, the password is compared in constant time, a
  connection that hasn't logged in within 30 seconds is closed and at most 16 are open. A command runs as
  `RconConsoleSource` runs it: at level 4 from a source named `Rcon`, whose `say` reads `[Rcon] message` and whose
  feedback reaches the operators in game as `[Rcon: message]` while `broadcast-rcon-to-ops` is on (the default), and
  `send_command_feedback` is on; the reply to the client always carries the feedback, whatever those say.
- **GS4 query.** `enable-query` and `query.port` turn on vanilla's UDP query, which server lists, status pages and
  monitoring tools read the MOTD, the version and the players from. The supervisor serves it (`query.rs`, std
  only), reads its settings as `QueryThreadGs4.create` does (a port outside 1 to 65535 leaves it off with
  vanilla's warning) and speaks `QueryThreadGs4`'s protocol: the handshake (type 9, a token below 2^24 as text),
  the basic stat and the full stat (a request of 15 bytes), each only with the token. The players come from the
  game's `server-status`, read at most once a second, and a full stat is kept 5 seconds, as in vanilla. Differences,
  each one named in `query.rs`: nothing answers while the game isn't running, and a session id outside ASCII is
  echoed as lossy UTF-8.
- **Vanilla's own management protocol.** Vanilla 26.3 has a server management protocol: JSON-RPC over a
  WebSocket, set up by the `management-server-*` keys in server.properties, so tools written for vanilla can
  manage a Horizon server too. The supervisor serves it (`management.rs`, the WebSocket in `websocket.rs`, std
  only like `http.rs`; TLS, on by default as in vanilla, through rustls in `management_tls.rs`, from a PKCS12
  keystore, or a self-signed one Horizon makes when none is set), so it answers while the game restarts
  (`server/status` says `started: false` then); calls that change the game go through its console.
  Authentication, errors and replies follow vanilla's `net.minecraft.server.jsonrpc` (`AuthenticationHandler`,
  `Connection`, `methods/*Service`), and `rpc.discover` gives vanilla's OpenRPC document (`management_schema.rs`).
  `tools/test_management_protocol_parity.py` sends vanilla's server and Horizon's the same messages over TLS
  (one keystore for both) and compares the answers, and the document with the data generator's
  `json-rpc-api-schema.json`.
  - built: the WebSocket and its authentication, TLS, `rpc.discover`, `players`, `players/kick`,
    `server/{status,save,stop,system_message}`, and the four lists (`allowlist/*`, `bans/*`, `ip_bans/*`,
    `operators/*`), whose changes the game applies itself (its control endpoint's `management` request, in
    `game/dedicated/management.rs`), so the source, expiry, log lines and kicks are vanilla's;
  - notifications: the supervisor pushes them to every connected client as vanilla's `JsonRpcNotificationService`
    does (a JSON-RPC request without an `id`, its one parameter in an array), through a queue and a thread so a slow
    client never holds the game's output. Built: `server/started` (the game's `Done`) and `server/stopping`
    (when the supervisor or the game's own `stop` begins the shutdown, once per run; with `[transfer]` on and
    players placed, at the start of the wait, `Stopping in N s`, so a proxy has that long to move them),
    `server/saving` and `server/saved` (around every world save of the running game: `save-all`, the autosave, the
    pause when empty; a save the game makes while it stops is after the last one the supervisor hears of), and
    `players/joined` and `players/left` (from the game's typed events, within a quarter of a second), and
    `operators/*`, `allowlist/*`, `bans/*` and `ip_bans/*` `added`/`removed`: each of the game's lists tells a
    watcher what `add`, `remove` and `clear` do, as vanilla's `StoredUserList` tells its `NotificationService`,
    so a change by the console, a command in the game or the protocol itself is told alike (`clear` once per
    entry); the event stream carries them as `operators_added`, `bans_removed`... too;
  - game rules: `gamerules` lists every rule the game has as a typed rule (`{key, type, value}`), `gamerules/update`
    sets one (checked against its type and bounds with vanilla's error texts, and set, logged and told on every
    update, as vanilla's does) and `notification/gamerules/updated` follows every change, by that method or by
    `/gamerule` (`Game::set_game_rule`); the parity script compares both servers on all of it;
  - `server/activity`: a login attempt (the game records `login_activity` in its login gate, as
    `ServerActivityMonitor.reportLoginActivity` is called when a login starts), told at most once in thirty
    seconds and lost otherwise, as the monitor clears its flag whatever it did; and `server/status`, the
    heartbeat: every `status-heartbeat-interval` seconds (none at 0, the first after as many) the status
    `server/status` answers, up or not, to whoever is connected;
  - planned: the rest of `serversettings/*` and the world-upgrade notifications (after `WorldUpgrader`'s server
    side).

## 9. Benchmark plan (bots against vanilla and Paper)

The goal is the published claim: "runs 50–100 players on hardware where vanilla struggles". It must be
reproducible by anyone, and show the same gameplay on both sides.

- **Servers:**
  - Horizon (`horizon-server`);
  - vanilla 26.3 (the owner's `server.jar`);
  - Paper (a jar the owner downloads; Paper's defaults are kept, and noted as non-vanilla);
  - optionally Fabric + Lithium (via the pinned `tools/benchmark-deps/mods.lock.json`).

  Java servers run on Java 25 with `-Xms` = `-Xmx` (the same heap for all, 4 GB by default), and the heap is
  reported.
- **Same conditions:**
  - the same seed (`12345`) and a fresh world, plus a second pass on a pre-generated copy of the same world;
  - `view-distance` 10 and `simulation-distance` 10;
  - `online-mode=false` and the same `server.properties`;
  - one server at a time on the same machine, with the bots on another machine on the LAN, or pinned to other
    cores when they share one (the report records which).
- **Bots:** `horizon-bots` (PR 5), a small Rust load client on `horizon-protocol`. It handles handshake,
  offline login and configuration, accepts teleports, answers keep-alives and pings, acknowledges chunk
  batches as vanilla's client does, and sends movement at 20 Hz. The bots play in creative and fly 24 blocks
  above their spawn point: they never touch terrain, so they need no copy of the world, and no server can
  refuse a move for walking into a block. Each behaviour is deterministic per bot from a seed:
  - `idle`: hovers above spawn;
  - `walk`: flies to random points within 48 blocks of spawn at sprint speed (5.6 blocks/s);
  - `explore`: flies straight outwards at creative flying speed (22 blocks/s), which forces chunk generation;
  - `mixed`: 60 % walk, 20 % explore, 20 % idle.
  - `fight`: survival bots (`gamemode=survival`, `force-gamemode=true`, `allow-flight=true`) that track the entities the server
    sends, walk to the nearest living one and attack it every 5 ticks, vanilla's bare-hand cooldown; with `--pvp true` (the
    default) they attack each other too, which needs `pvp=true` on the server. The report counts `attacks`.

  A `build` behaviour (breaking and placing blocks) needs the bots to read terrain and comes later. Runs use
  1, 10, 25, 50 and 100 bots, joining 1 per second. Bots are needed because 100 real vanilla clients can't run
  on one machine.
- **Measured** (once a second, for 5 minutes after the last bot joins, after a 1-minute warm-up):
  - TPS and MSPT (average, P50/P95/P99), from each server's own `/tick query` (vanilla's
    `ServerTickRateManager`; Horizon implements the same command and clock), so every server measures itself
    the same way;
  - RSS and CPU (process tree, `horizon_bench::rss_bytes`/`cpu_seconds`);
  - chunk generation: chunks per second while the `explore` bots run, and time until each new bot has its
    full view loaded;
  - start time: cold and warm, to `Done`;
  - bot-side: chunk arrival, keep-alive round trip, and disconnects (a run with disconnects is reported as
    failed).
- **Output.** `tools/server_bench.py` drives it: a fresh server per run from the same `server.properties`,
  `Done`, the bots, a warm-up, then the measurement. It writes a `horizon-server-benchmark` JSON report (the
  machine, commit, Java version, settings and every run) and prints a comparison table. Results are
  published like the client benchmark, with the machine, the versions and the exact commands. TPS is derived
  from `/tick query`'s average (20 while it fits in 50 ms), since the command reports times, not the rate.
- **Results.** The latest comparison (Horizon, Pumpkin and vanilla under the same bots) is
  [performance/server-vs-pumpkin.md](performance/server-vs-pumpkin.md); `tools/server_bench.py` reruns it.
