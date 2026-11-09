# Horizon native client

`horizon-client` is the playable Rust client for Minecraft Java Edition 26.3.
It streams a world around the player on worker threads, renders every vanilla
block model with smooth lighting and biome tints, and runs the validated player
physics, health and hunger at 20 ticks per second. You can mine with vanilla
hardness/tool rules, collect drops from the block loot tables, place blocks,
craft in the 2x2 inventory grid or on a crafting table, and save and reload worlds
in vanilla's Anvil format.

**Status:** playable; players get it through [the launcher](../launcher/README.md). The [harness table](../README.md#project-status)
in the README scores each subsystem against vanilla and lists what is still
missing; [Rust rewrite status](rust-parity.md) has the details.

On Windows, the launcher uses a stable executable path for each game home across
release and native-mod updates. Its private runtime copy lives under the launcher's
data folder in `run/<home key>/game/`; installed versions remain intact. Opening a
world to other machines can still require an initial Windows Firewall permission.
The launcher does not create firewall rules or change network-profile settings.

## musl Linux: manual installation

On x86-64 musl distributions, download `horizon-linux-musl-x86_64.tar.gz` from the
[Nightly release](https://github.com/horizon-rust/horizon/releases/tag/nightly), when available. This optional
build is omitted if its build fails. The launcher does not install or update it. Its desktop gameplay and
vanilla parity are still unverified; see the [musl build limitations](release-flow.md#builds).

You need a working X11 or Wayland desktop, musl, libgcc, ALSA and libudev. For Vulkan, install both the loader
(`libvulkan.so.1`) and your GPU's driver. On Alpine, the common runtime packages are:

```sh
sudo apk add musl libgcc alsa-lib eudev-libs vulkan-loader
```

Install the appropriate Mesa/Vulkan driver and window-system libraries for your desktop separately; OpenGL
also needs a working musl-compatible OpenGL/EGL stack. NVIDIA's proprietary userspace drivers require glibc
and cannot be loaded by this build; see [Alpine's GPU requirements](https://wiki.alpinelinux.org/wiki/Requirements).

Resource preparation needs a **musl-compatible system JDK 25** (including `java` and `javac`). Mojang's bundled
Linux Java runtime needs glibc. Install JDK 25 from your distribution or another musl-compatible provider,
set `JAVA_HOME` to that JDK, and check `"$JAVA_HOME/bin/java" -version`. You also need your own Minecraft Java
Edition 26.3 installation and its server jar. Download the server jar yourself from the `downloads.server.url`
in that version's launcher JSON. Horizon's preparer does not download it.

Extract the archive and, inside its folder, run the following with your actual paths:

```sh
./horizon-prepare --prepare --minecraft-dir /path/to/minecraft --server-jar /path/to/server.jar
./horizon-prepare sounds --minecraft-dir /path/to/minecraft
./horizon-client --resources "$PWD/.local/horizon-client/26.3"
```

The sounds command copies the sounds from your Minecraft installation's asset store. If that store is
incomplete, obtain the missing assets through your Minecraft launcher first. Preparation details and alternative
layouts are described below.

Updates are manual: download and extract the next musl archive, prepare its resources, and use that version's
programs. Keep your saves in a separate `HORIZON_HOME` and point each version at it. Back up saves before
updating. The bundled `horizon-server` also refuses automatic updates because the server update archives use
glibc. An absent musl archive is not a reason to install the glibc archive on a musl system.

## Run from a checkout

Use current stable Rust (1.90+) and JDK 25 (Python 3.10+ only for the development
harnesses). A checkout never
downloads the game: resource preparation imports **your own copy** (the 26.3
client from your official launcher install, and the 26.3 server jar you
downloaded yourself; see [Build from source](../README.md#build-from-source)),
verifies it against the pinned checksums, runs vanilla's data generator and small
exporters against those jars, and extracts the jar's assets and data into ignored
`.local/horizon-client/26.3/`. Sounds (about 380 MB) are copied from your
launcher's asset store by `horizon-prepare sounds`, each file checked against the asset
index's SHA-1. Preparation also tries to replace the JAR's gray 1×1 title panorama
placeholders with the full textures from that asset store. If the store is missing or
incomplete, preparation logs a warning and keeps the placeholders. To import the panorama
later without rerunning the Java exporters, run `horizon-prepare panorama` (or
`cargo run --release -p horizon-prepare -- panorama` in a checkout). Pass
`--minecraft-dir PATH` when using a different Minecraft installation.
Prepared resources are private; never commit them.

Preparation is `horizon-prepare` (`crates/horizon-prepare`, Rust): it needs no Python, only
the JDK, for the small exporters that call Mojang's own code. Release folders ship it beside
`horizon-client`, and the launcher prepares with it and copies the sounds as part of preparing.

`--output DIR` puts the resources elsewhere (the exporters' scratch files stay in the version
folder's `.local/`), for preparing and for the asset commands alike. `horizon-prepare --check`
exits 0 when the resources are complete and current for this preparer, and otherwise prints
each gap with the command that fills it on stdout and exits 1. It also exits 1 when it can't
check, with `error:` on stderr (no `research-lock.json`, say). It runs no Java and reads no game
files: it tests which files are there and the versions they were made for, not their contents, so
a launcher that suspects damage prepares again. A preparation stamps its resources with the
preparer's `OUTPUT_VERSION` (`horizon-prepare.json`) when it finishes, and removes the stamp
before it writes anything, so `--check` reports a folder being prepared, or whose preparation
stopped, as not current. Resources prepared before the stamp existed report an unknown version
until they are prepared again. One preparation at a time writes to a folder: each holds
`.horizon-prepare.lock` in the resources and in the version folder's `.local/`, and a second one
waits for the first. `--complete` prepares, copies the sounds, and fails unless `--check` would
then pass: one command for resources with nothing missing. `--progress json` prints one JSON
object a line instead of plain text (its format is part of the Launch API:
[docs/api/launch.md](api/launch.md#--progress-json)). Exit codes: 0 done, 1 failed or gaps found,
2 a refused command line.

A launcher whose files are laid out its own way names them one by one instead of
`--minecraft-dir`: `--version-json` (the pinned version's JSON), `--client-jar`,
`--libraries-dir` (the client's libraries, Maven layout) and `--assets-dir` (the asset store,
with `indexes/` and `objects/`); each replaces only what it names, and every file is still
checked against `research-lock.json`'s pins. When an earlier import beside `--root` is still
verified, it is reused and the named files aren't read. Two things no such launcher keeps:
- **The server jar.** The first `--prepare` needs `--server-jar`. Horizon never downloads it; a
  launcher downloads it (the version JSON's `downloads.server`) only for a signed-in account that
  owns Minecraft: Java Edition, as `horizon-launcher` does.
- **Java 25.** `--java-home PATH` names a runtime the launcher manages. The preparer takes the first
  Java of the pinned version from `JAVA_HOME`, the `PATH`, `--java-home`, then the official
  launcher's runtimes.

```sh
cargo run --release -p horizon-prepare -- --prepare --server-jar ~/Downloads/server.jar
cargo run --release -p horizon-prepare -- sounds
cargo run --release -p horizon-client -- --render-distance 12
```

Development builds that link quickly: `cargo build --profile fast -p horizon-client`.

`HORIZON_OCCLUSION_PROFILE=1` enables detailed entity-occlusion phase and bounds-scan timings
in `horizon-ctl stats` and `bench`. The normal benchmark keeps these CPU-clock reads disabled;
query, cache-hit and cull counters remain available without profiling.

Optional Windows allocator build features are documented in
[Windows allocator experiments](windows-allocators.md).

| Option | Meaning |
| --- | --- |
| `--world NAME` | World folder under `saves/` (default "New World"); reopened if it exists |
| `--seed N` | Seed for a new world (numbers or text, like vanilla) |
| `--creative` | Creative mode (instant breaking, pick block) |
| `--render-distance N` | Chunks, 2..128 (default 12) |
| `--resources PATH` | Prepared resource directory |

| Control | Action |
| --- | --- |
| Mouse | Look (vanilla default sensitivity) |
| WASD / Space / Left Shift / Left Ctrl | Move, jump, sneak, sprint |
| Left / right click | Break (hold) / place or use; right click opens crafting tables |
| Middle click | Pick block (creative) |
| 1–9, scroll | Select hotbar slot |
| E | Inventory and 2x2 crafting (shift-click to quick-move or craft all) |
| Q / Ctrl+Q | Drop one / drop stack |
| F3 | Debug overlay (position, chunk, light, biome, FPS) |
| F3 + S | Dump the texture atlas to `screenshots/debug/`: `minecraft_textures_atlas_gui.png` with `minecraft_textures_atlas_gui.txt` beside it, every sprite's rect in texels (vanilla's `TextureManager.dumpAllSheets`) |
| Esc | Pause menu (Save and Quit) |

Worlds autosave every 6000 ticks (5 minutes), when chunks unload and on quit.
On headless Linux use `vscreen start ./target/release/horizon-client` and
`vscreen shot`.

Sound: the game prints the output it opened (`Horizon: audio output ...`) or
why it has none (`Horizon: no sound: ...`). It opens none with
`HORIZON_NO_AUDIO` set, nor on a `vscreen`, where tests run beside someone
listening; `HORIZON_AUDIO=1` opens it there anyway, so send it to a null sink
(`PIPEWIRE_PROPS='{ target.object=<sink> node.dont-fallback=true }'`). Don't
mute a test run's stream in the mixer: PipeWire remembers the mute for every
later `horizon-client`. The launcher starts the game as "Horizon" for that
reason. On Linux, Horizon requests a 1024-frame fixed buffer for lower latency;
if your audio server underruns (e.g. PipeWire with `min-quantum` ≥ 1024), set
`HORIZON_AUDIO_BUFFER=2048` or `HORIZON_AUDIO_BUFFER=default` to use the backend's
default buffer size.

### Experimental older servers

The native Rust adapter recognizes 26.2, 26.1.x, 1.21.11, 1.21.9/1.21.10,
1.21.7/1.21.8, 1.21.6, 1.21.5, 1.21.4, 1.21.2/1.21.3, 1.21/1.21.1,
1.20.5/1.20.6, 1.20.3/1.20.4 and 1.20.2. It is off by default while compatibility is incomplete. Set
`HORIZON_OLDER_SERVERS=1` before starting the game; the server's status ping selects
the adapter. The client still needs its prepared **26.3** resources.

```sh
HORIZON_OLDER_SERVERS=1 cargo run --profile fast -p horizon-client -- --server 127.0.0.1:25565
```

In PowerShell, set `$env:HORIZON_OLDER_SERVERS = '1'`, then run the same Cargo
command without the environment assignment at its start.

This does not load Fabric JARs or emulate every older version's movement and
combat. Older item components still have gaps; a recognized protocol is not a
promise of complete compatibility. The 1.21.6 and 1.21.7/1.21.8 adapters use vanilla
packet layouts shared by these releases, with separate item/sound mappings for
1.21.6. The 1.21.5 adapter also removes vanilla's chunk-buffer padding and converts crouch/action commands,
hanging-entity metadata and the pre-1.21.6 attribute-modifier/equippable layouts.
The 1.21.4 bridge converts its older chunks, tooltip flags, nested stacks, chat,
team rules and entity metadata. Old inventory clicks use the server's authoritative
slot/cursor resynchronization. Smithing previews follow the server's synced trim
registry; inline 1.21.4 trim holders and templates outside the known vanilla item
set remain unsupported.
The 1.21.2/1.21.3 bridge also handles particle flags and client-chosen trail lifetimes,
vehicle movement, tab-list hats, integer custom-model data, old creaking-heart states
and the transient creaking type. Custom-model data uses the first float when sent
back; integers beyond exact float precision can lose precision. Server-delegated
block/entity picking is not supported on 1.20.2 through 1.21.3, whose old pick packet
requires a client inventory index instead of a world target.
The 1.21/1.21.1 bridge converts older recipe declarations and book updates, food
consumption data, potions, instruments, teleports, inventory windows and settings.
Boat wood variants become their corresponding modern entity types, retaining
movement, metadata and passengers when recreated. Old explosions preserve their
radius, destroyed-block updates and knockback. Custom recipe serializers and
consumable effects without an old representation remain unsupported.
The 1.20.5/1.20.6 bridge also converts UUID-based attribute modifiers, food without
a remainder field, horse inventory sizes, projectile acceleration and item-use
packets without rotation. Static enchantment and painting names are supplied in
their original wire-id order; their definitions use the client's prepared resources.
Old modifier names become vanilla's canonical identifiers and cannot retain arbitrary
original display names on write-back. New identifiers without a known old UUID
receive a stable generated UUID, so their original identifier is lost on a round trip.
The old three-component projectile acceleration is represented by its
magnitude; a custom direction independent of velocity has no scalar representation.
Wolf registry references on 1.21 through 1.21.4 are normalized past their older
holder encoding; inline wolf definitions on those versions remain unsupported.
The 1.20.3/1.20.4 bridge splits the whole-NBT registry sync and upgrades legacy
NBT item stacks through Horizon's datafixes. A connection-local cache retains original
tags for unchanged stacks, bounded to 1,024 entries and 8 MiB of serialized values;
ambiguous normalized keys, evicted entries and entries over 256 KiB use semantic
reconstruction. Modified entity/block-entity data has no general reverse datafix,
and newer components without an old field cannot be represented. Incoming block-entity
updates and chunk entries use forward datafixes for banner patterns, skull profiles,
text and nested inventory stacks, retaining opaque fields and the original tag identity.
Independent vanilla fixtures cover these conversions; they do not establish full rendered parity. Static banners,
enchantments, paintings, instruments and the original wolf coat use vanilla names.
Old recipes, trades, horse equipment, attribute/effect metadata and particles are
translated; legacy commands retain the chat tracker's acknowledgement even without
signable arguments. Full older movement, combat and particle behavior parity remains
unfinished. Authentication follows the login encryption request, and old secure-chat
enforcement is applied when server data arrives; online-mode chat remains unvalidated.
The 1.20.2 bridge converts JSON component fields to NBT while retaining real NBT
and the login-disconnect JSON exception. It selects saved-item data version 3578,
handles width-first shaped recipes and grass/short_grass names, splits old score
removals, supplies absent objective/explosion fields from vanilla defaults and
reads vibration source identifiers. Its single server resource-pack slot uses
connection state across configuration and play; newer intermediate statuses have
no old response. Pack replacement and asynchronous results retain native pack
generation identity. Vanilla's global inactive-pack failure policy still applies,
including a late failed download affecting newly pending packs. Advanced mutated
entity NBT has no general reverse schema, including old projectile potion data.
Structured hover-item and hover-entity rendering still has native gaps; verified
wire-text conversion does not establish complete interactive hover behavior.
Online-mode login and complete interactive playthroughs on these versions
have not been validated. Earlier Java releases, Classic/Alpha/Beta,
April Fools and Bedrock are outside the implemented adapter set.

For a protocol-only development check, append `--server-protocol 772` to
`cargo run -p horizon-protocol --example protocol_probe -- 127.0.0.1:25565 20`.
The probe fails if it encounters packet or chunk decode errors.
Use `--server-protocol 764` for 1.20.2, `765` for 1.20.3/1.20.4, `766` for 1.20.5/1.20.6,
`767` for 1.21/1.21.1, `768` for 1.21.2/1.21.3, `769` for 1.21.4, `770` for 1.21.5, or `771` for 1.21.6;
1.21.7 and 1.21.8 both use `772`.

The registry mappings and named pinned IDs are generated from official vanilla
reports. `tools/regenerate_registry_id_adapters.py` records each release's exact
rename, reassignment, state-alias, inheritance and static-name options. Each
release listed in `RELEASES` needs `VERSION/jars/client.jar` and its matching
official `VERSION/datagen/reports/{registries,blocks}.json`. For unobfuscated
26.1+ releases, `tools/version_port.py prepare VERSION --download` supplies these.
For older releases, use owned official client jars and reports exported by the
matching vanilla server's data generator:

```sh
java -DbundlerMainClass=net.minecraft.data.Main -jar VERSION/jars/server.jar --reports --output VERSION/datagen
```

With those inputs prepared, run:

```sh
python3 tools/regenerate_registry_id_adapters.py --versions /path/to/prepared/versions
python3 tools/regenerate_registry_id_adapters.py --versions /path/to/prepared/versions --check
```

The version store defaults to `HORIZON_VERSIONS` (or `~/code/horizon-parity/versions`).
The pin uses the repository's imported `.local/` jar and reports; `--new DIR`
selects a separately prepared pin. `--only VERSION...` selects releases while
still checking pinned constants. `--check` writes no tracked files and fails if
any output differs; regeneration itself downloads nothing. This checks
report-derived IDs; gameplay still needs its parity harnesses. When bumping the
pin, regenerate these files and review the mappings with its reports.

### Agent control (`horizon-ctl` and MCP)

AI agents and scripts play Horizon through `horizon-ctl`, a command line that
reads the game as JSON and acts like a player, and `horizon-ctl mcp`, an MCP
server with the same commands. `--control` makes the game accept them on a
local socket; under control, losing window focus does not pause the game or
release held keys, so a script can play while you use other windows.

```sh
cargo build --release -p horizon-client -p horizon-client-lib --bins
C=target/release/horizon-ctl
$C launch --world Survival --seed 5 --wait   # start the game, open or create the world, wait for it
$C state                                     # player, world, screen, crosshair target
$C find-block birch_log                      # the 16 nearest loaded logs
$C goto -0.5 65 -2.5 --near 2.5              # walk there along a path
$C break -1 65 -3                            # mine it: {"drops": {"minecraft:birch_log": 1}, ...}
$C inventory                                 # hotbar, inventory, armor, offhand, counts
$C open-inventory                            # craft planks in the 2x2 grid:
$C click-slot 0; $C click-slot 100 right; $C click-slot 0; $C click-slot 200 shift; $C close
$C place 2 65 -2 --item birch_planks         # {"changed": true, "block": {...}}
$C pause; $C ui                              # the pause menu's widgets: ids, kinds, labels, bounds
$C click options; $C set fov 0.75; $C click done   # widgets by id
$C screenshot /tmp/view.png                  # PNG of the next presented frame
$C quit                                      # save and exit
```

`horizon-ctl help` lists the commands by group and `horizon-ctl COMMAND --help`
explains one with examples; `horizon-ctl help --game` lists what the running
game accepts, test hooks included.

- **Observe:** `state` (position, rotation, velocity, health, food, saturation,
  XP, effects, game mode, dimension, time, weather, the screen and what the
  crosshair points at), `looking-at`, `entities [RADIUS] [TYPE]` (mobs with
  ids and health, dropped items, orbs), `block X Y Z`, `blocks X1 Y1 Z1 X2 Y2 Z2`, `light X1 Y1 Z1 X2 Y2 Z2`,
  `find-block BLOCK [RADIUS]`, `inventory` (stacks with their components as
  SNBT), `screen` (the open screen or title menu, its buttons and slots) and
  `messages` (chat, title, subtitle and action bar).
- **Move and act:** `move DIR SECONDS`, `jump`, `goto X Y Z` (A* over standable
  blocks: steps up one block, drops up to three, avoids lava), `look`,
  `look-at` a point, block face or entity, `attack [ID]`, `break X Y Z`,
  `place X Y Z [--item ITEM]`, `interact X Y Z` (doors, chests, crafting
  tables), `use-item [--hold S]` (eat, drink, throw), `select SLOT|ITEM`, `drop`.
- **Widgets:** `ui` lists every widget of the open screen (title menus, pause,
  options pages, inventories and menus): an id, its kind
  (`button`, `slider` with its 0..1 `value`, `field`, `row`, `tab` with
  `selected`, `slot` with its stack), the label and its bounds. Ids come from
  labels (`FOV: 70` is `fov`, `Video Settings...` is `video-settings`, a
  world row is the world's name; a repeat gets `-2`), else `row-0`, `field-0`,
  and slots are `slot-ID`. `click ID [right] [shift]` clicks one as the mouse
  would and `set ID VALUE` moves a slider to 0..1; after a click, the next
  `ui`, `click` or `set` waits for the screen to be redrawn. Design and plans:
  [agent-control.md](agent-control.md).
- **Options:** `options-get [KEY...]` gives every `options.txt` option by its
  vanilla key with its value and what it accepts (`{"type": "int", "min": 30,
  "max": 110, "value": 70}`; enums with `choices`, key bindings as
  `key.keyboard.w`); `options-set KEY VALUE` sets one, applies it at once and
  saves `options.txt` as the options screen does. A value the option refuses
  is an error saying what it accepts, not vanilla's silent reset.
- **Waits and events:** `wait-for screen NAME|widget ID|loaded|chat TEXT|event
  KIND [--timeout S] [--since SEQ]` replies once the game gets there (a timeout
  is an error with `"kind": "timeout"`), so scripts don't poll. `events [--since
  SEQ]` lists what happened, oldest first, each with a `seq` and the `tick`:
  `screen` (from, to), `chat` (text), `death`, `respawn`, `item` (item, delta),
  `dimension`, `world` (opened, closed). Take `last` from `events` before an
  action whose event you then `wait-for --since` it, so a fast one isn't missed.
- **Screens:** `open-inventory`, `click-slot SLOT [right] [shift]` (0-8 hotbar,
  9-35 inventory, 100+ crafting grid, 200 result, 300+ containers, 500-503
  armor, 504 offhand, 600+ stations), `close`, `click-button LABEL` (any
  button of any screen or title menu, by its label),
  `click-at X Y`, `type TEXT` (on the create-world screen `--field name|seed
  --replace`), `chat`, `command`, `respawn`, `pause`, `resume`.
- **World:** `tp`, `give`, `time`, `setblock`, `summon`, `wait TICKS`, and the
  benchmarking commands `bench`, `stats` and `fly` (below).

Actions block until they finish or time out and reply with the outcome: `goto`
says whether it arrived, `break` what dropped and what was picked up, `place`
the block placed. `stop` cancels the running action. Every reply is one JSON
line; errors are `{"error": "..."}` objects, and the exit status is 0 on
success, 1 when the game replied with an error, 2 for a usage error and 3 when
the game cannot be reached or started. `horizon-ctl script FILE` (or stdin)
runs one command per line with shell-like quoting and stops at the first error
(`--keep-going` doesn't); `sleep S` waits. `horizon-ctl -` is the older form
that reads stdin and keeps going.

`launch` finds the game to start: `--game PATH`, else a `horizon-client` next
to `horizon-ctl` (a build folder or a release folder), else the newest version
the launcher installed (`--version TAG` picks another), run with its prepared
resources from the game home, where the saves are. So after installing with
the launcher, `horizon-ctl launch --world X --wait` is all an agent needs.
Other arguments go to the game; its output goes to a `.log` file beside the
socket.

**Logs and crash reports.** Like vanilla, the game writes `logs/latest.log`
in the game home (`$HORIZON_HOME`, else `~/.local/share/horizon`; the
dedicated server in its own folder) with lines like
`[12:04:31] [Render thread/INFO]: message`, and at start archives the last
run's as `logs/<yyyy-mm-dd>-<n>.log.gz` (seven a day). Messages still print on
stderr as before. A panic on the main thread or a fatal error writes
`crash-reports/crash-<yyyy-mm-dd_HH.MM.SS>-client.txt` (the error, its stack,
the machine, the GPU, render settings, resource packs, the open world and the
last 50 log lines) and the game exits with a non-zero code. `horizon-ctl logs
[--lines N]` prints the end of latest.log and `horizon-ctl logs --crash` the
newest report (the `logs` MCP tool returns the same). F3 + C held for ten
seconds crashes the game on purpose, as in vanilla (the world is saved first);
development builds also crash with `HORIZON_DEBUG_CRASH=SECONDS`, that long
after the first frame.

**MCP.** `horizon-ctl mcp` serves the Model Context Protocol over stdin and
stdout (JSON-RPC 2.0, one message per line), with one tool per command and a
JSON schema for each; `screenshot` returns the frame as an image. Register it
in an MCP client as a stdio server with the command `horizon-ctl` and the
argument `mcp` (`--socket PATH` before `mcp` to pick a socket).

**Transport.** On Linux and macOS the endpoint is a Unix socket:
`$HORIZON_CONTROL`, else `$XDG_RUNTIME_DIR/horizon-control.sock` (without a runtime
directory and with a temporary directory every user shares, as `/tmp`,
`horizon-<uid>/horizon-control.sock` in it, a folder only that user may open), or
`--control-socket PATH` (`horizon-ctl --socket PATH`). Windows has no Unix
sockets: there the game listens on a random `127.0.0.1` port and writes
`{"transport":"tcp","port":N,"token":"..."}` to the endpoint path
(`%TEMP%\horizon-control.json` by default), readable only by its user; the
client's first line must be `auth TOKEN`. Any endpoint path ending in `.json`
selects that transport on Linux and macOS too.

`tools/agent_control_e2e.py` checks it all on a virtual screen: it creates a
world through the title menus, walks to a tree, breaks a log, crafts planks in
the inventory screen and places one, through the CLI and through MCP.

`fly X1 Y1 Z1 X2 Y2 Z2 SECONDS [PITCH]` moves the camera along a line at
constant speed and replies with the same summary as `bench` (avg/1%-low FPS and
p50/p95/p99/max frame times), plus CPU and GPU time per frame; [the graphics
benchmark](graphics.md#benchmark) is built on it. Frame times include waits for
presentation, so `bench` reports what is on screen. With asynchronous entity
culling active, the summary also includes `async_occlusion_culling`: cache hits
and misses, worker time, queued and skipped queries, budget limits, and entity
vertices culled.

### Discord Rich Presence

While Discord runs on the same computer, the player's Discord profile shows
"Playing Horizon" and what they are doing: "In the menus", "Singleplayer",
"Multiplayer" or "Hosting a world" (with the party size, of 8), the dimension
("In the Nether") and the time since they entered the world. It never shows a
world's name or a server's address. The switch is on the Extras page, in its
Multiplayer group (**Discord Rich Presence**, on by default; `discordPresence`
in `horizon-extras.txt`; in splitscreen only the first player has it), and
`HORIZON_NO_DISCORD=1` turns it off for one run.

The crate `horizon-discord` (Horizon layer) speaks Discord's local IPC itself
(the `discord-ipc-N` socket or named pipe, no SDK) from a thread of its own.
The game hands it a status only when it changes, through the platform's
`activity` hook (`game/activity.rs` says what the player does), and keeps the
switch itself: its settings store and its Extras row (the platform's settings
and pages registries). Only the official executable (`horizon-official`)
registers it: horizon-client's own executable shows no presence and no switch. It retries every 15 s
while Discord isn't running and stays within Discord's 5 updates per 20 s.
Dedicated servers never start it, and automated runs (offscreen, benchmarks)
only talk to a fake Discord: `lab.game_env` sets `HORIZON_NO_DISCORD=1`, and
`tools/discord_presence_e2e.py` points games at a fake server with
`HORIZON_DISCORD_IPC_DIR` (title, world, Nether and quit on both single-player
paths; a LAN host and guest; the switch; a Discord started after the game).
`DISCORD_APP_ID` is Discord's "Horizon" application, whose `horizon` image is
the large one; there are no game-mode images yet.

### Play on another of your machines

Horizon targets desktop Linux, macOS and Windows (see [Windows ARM64](windows-arm64.md)).
To play on another machine without Rust, JDK or prepared resources,
package a release build where they exist:

```sh
cargo build --release -p horizon-client
python3 tools/package_native_client.py --out ~/builds/horizon
```

The archive holds the binary, the prepared resources and `horizon.sh`. Copy it
to the target machine (same OS and architecture), unpack it and run
`horizon/horizon.sh`; `horizon.sh --install` adds an application menu entry.
Saves and `options.txt` go to `~/.local/share/horizon` (or `$HORIZON_HOME`), so
unpacking an update keeps them. The archive contains Mojang assets: keep it
private.

## How it fits together

Where each subsystem lives (game loop, rendering, streaming, light, meshing, saves) is in
[AGENTS.md](../AGENTS.md#horizon-client-where-things-live) and [architecture.md](architecture.md). Every vanilla
comparison is a harness in `tools/` ([parity-workflow.md](parity-workflow.md)).
