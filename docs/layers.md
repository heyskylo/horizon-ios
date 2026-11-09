# Layers: engine, platform, Horizon

Horizon is three layers:

- a **vanilla engine**;
- a **platform** that makes the vanilla game easier to change, port and run;
- **Horizon's own layer**: its extras and its product (cosmetics, the shop, telemetry, sponsors, official servers,
  the launcher...).

**The target** (Ludovic, 2026-11-09). This repository holds the engine and the platform. Built alone, it is a
complete vanilla-like game. The Horizon layer moves to its own repository, public if its license needs it, and
ships with every official launcher download (see [The Horizon layer in its own repository](#the-horizon-layer-in-its-own-repository)).
The split comes first, inside this repository, one checked step at a time. The repositories separate after that.

This page says:

- what belongs to each layer and which layer may depend on which;
- where a new feature goes;
- where every built-in outside address lives;
- the plan.

CI checks the rules, so review doesn't have to. Contributors fixing vanilla behaviour never have to touch
cosmetics, the shop or a server address. Mods and desktop-class ports (iOS, Android) build on the platform without
editing engine files; ports to machines the desktop client can't run on (a handheld console, a browser) build on
the [portable crates](#portable-crates).

Credits:

- **NikitaMGrimm** (DuckyBertDuck on Discord): the three layers, the dependency rule, the address rule and the plan
  ("3-layer split: engine / platform / Horizon", #contributors, 2026-11-08), the portable crates (#12997, #13002),
  and the review that made the Horizon layer linked crates rather than native mods.
- **arismoko:** the mod API owns its types and a CI property check keeps engine types out of it
  ([arismoko/horizon](https://github.com/arismoko/horizon), arismoko/horizon#1608). Also the idea of shipping
  Horizon's own parts from another repository as a launcher plugin.
- **Martipops** (controllers, splitscreen): the compositor's place in the engine, and where controllers,
  splitscreen, Replay Studio, PvP & HUD and a UI page API go.
- **Contributors at the 2026-11-08 stage:** the file-size rule, the single list of addresses, sponsors in the
  Horizon layer, a platform that exposes what vanilla already uses, and extras as native mods.

## The rule

| Layer | May depend on |
|---|---|
| engine | engine |
| platform | engine, platform |
| Horizon | platform, Horizon |
| tool (dev tools) | anything; no shipped crate depends on a tool |

The engine never depends on the platform or on Horizon. The platform never depends on Horizon. **The Horizon layer
never depends on the engine:** it uses only the platform's public API, so it can build from another repository.
Normal, build and optional dependencies all count; dev-dependencies don't, so tests may use anything.

When lower code needs something from above, the lower layer offers an extension point and the higher layer plugs
into it. The extension point is a trait, a callback list, an event or a registry, and it names no type from above.
When Horizon needs something from the engine, the platform exposes it as public API.

## Engine

Vanilla Minecraft: Java Edition 26.3, and nothing else.

- **Belongs:** gameplay, world data and generation, block behaviour, mobs, items, commands, the network
  protocol, rendering, sound, vanilla's screens, saves, the integrated and dedicated servers. It also holds
  optimizations that keep vanilla's behaviour, and the extension points the platform plugs into.
- **Also the render pipeline's own capabilities that vanilla's path doesn't use**, when a higher layer could only
  get them by rebuilding the pipeline. The one today is the **compositor**: drawing a view into an offscreen target
  and composing several targets into a window's views. With one view (vanilla, and splitscreen with one player)
  the renderer takes vanilla's path and the compositor does nothing, so the engine still plays 1:1 vanilla.
  Splitscreen, the feature that uses it, is platform (below).
- **Examples:** `horizon-world` (`worldgen/`, `level/redstone.rs`), `horizon-protocol` (`packets/`),
  `horizon-items`, `horizon-command`, `horizon-core`, `game/creatures.rs`, `game/integrated.rs`, `gfx.rs`,
  `mesher.rs`, `streaming.rs`, `lan/`.
- **Never:** a mod hook's implementation, a shader or pack format vanilla doesn't read, a setting or screen vanilla
  doesn't have, a network address vanilla doesn't contact, Horizon branding.
- **Network:** only what vanilla contacts: Mojang and Microsoft services, and the servers the player joins.

### Portable crates

`horizon-core`, `horizon-world`, `horizon-protocol`, `horizon-items`, `horizon-command`, `horizon-client-core`
and `horizon-assets` hold vanilla's rules and data with no desktop in them: no window, GPU, sound, gamepad or
clipboard library, no HTTP client, and no engine crate outside this list. The platform sits on the desktop engine
(`wgpu`, `winit`, `cpal`), which a handheld console or a browser can't run, so a port there builds on these crates
instead. They stay that way: the layer check fails when one takes `winit`, `wgpu`, `cpal`, `gilrs`, `arboard` or
`ureq`, or an engine crate outside the list (#13002). Gameplay itself still lives in `horizon-client`; step P of
the [plan](#plan) moves it here over time.

## Platform

What makes vanilla nicer to change, port and run, with no Horizon branding and no address of its own. It is the
public API that mods, desktop-class ports and the Horizon layer build on.

- **Belongs:** the mod API and mod formats, the native mod host, the shader API and pack formats beyond vanilla
  (OptiFine, Polytone, `shaders/`), HDR, controllers, splitscreen, Replay Studio, the HUD widget framework, UI
  pages and the settings system, agent control (`horizon-ctl`, MCP), the dedicated server's anticheat and chat
  filter, modpack porting, the design tokens of non-vanilla screens.
- **Exposes what vanilla already uses.** FOV changes, potion effects, the player's state, screens, rendering
  hooks and so on become public API with stable types (arismoko/horizon#1608's patterns). Then a mod, a port or a
  Horizon feature builds on them instead of editing or copying engine code.
- **Examples:** `horizon-shader-api`, `horizon-shaders`, `horizon-optifine` (`cem/`), `horizon-polytone`,
  `horizon-hdr`, `horizon-dualsense`, `horizon-mods`, `cargo-horizon-mod`, `horizon-anticheat`, `horizon-mcp`,
  `horizon-theme`. Still in engine crates today: `game/mod_api_host.rs`, `game/native_*.rs`, `shader_ui.rs`,
  `hdr_calibration.rs`.
- **Never:** a built-in address (every address comes from the player, a file or a setting), anything that names
  a Horizon crate, cosmetics, the shop, official or partner servers, uploads to playhorizon.dev.
- **Network:** allowed, but never to a built-in address.

### Controllers, splitscreen, replays and UI pages

Vanilla has none of these, and the engine stays 1:1 vanilla, so they are platform features on hooks the engine
exposes. Ports to consoles and handhelds need them, and other projects can build on them.

- **Controllers:** the pad backend (gilrs), bindings, deadzones and the look curve, button glyphs, the pad's menu
  navigation and cursor, the controller settings, DualSense output. Today in `horizon-client-input/src/gamepad.rs`,
  `game/pad.rs`, `game/pad_*.rs` and `game/pad/`, `pad_cursor.rs`, `pad_glyph.rs`, `controller_ui.rs`, and
  `horizon-dualsense`. The engine's hooks:
  - **Actions.** Every input the game reads is an `Action`, vanilla's key mappings (`horizon-client-input/src/actions.rs`),
    and code asks whether an action is held (`held.rs`), never a key. A pad presses and holds actions like a
    keyboard does. This hook exists today.
  - **Movement and look.** An analog move vector and a look delta that the player's movement reads instead of
    keys (today `game/pad.rs` calls `gamepad::move_vector` and `Game::look_turn` itself).
  - **Menu focus.** Screens say which widget has focus and take "move focus" and "press" (today the pad's
    cursor drives vanilla's screens through mouse events).

  The last two become a registered input source, a trait in the engine that the platform's pad code implements,
  in M6. Until then the pad code stays in its own modules, so that move is a pure file move.
- **Splitscreen:** more local players, each its own `Game` joined to P1's server thread through memory; which
  device drives which player; the setup screen, window layouts and profiles. Today in `game/split.rs` and
  `game/split/`, `split_layout.rs`, `split_map.rs`, `split_profiles.rs`, `split_ui.rs`. It draws through the
  engine's compositor (see [Engine](#engine)), and its input comes through the input hooks above, one input
  source per player.

  The compositor stays in the engine, as Martipops (who builds splitscreen) and duckybertduck argued: it is part
  of how the renderer presents a frame (targets, submission, pacing, present). Moving it above the engine would
  mean exposing the whole frame pipeline as API or rebuilding it up there, for a feature that on vanilla's path
  costs nothing. What the engine must not hold is the feature: the router, the screens, the layouts.
- **Replay Studio:** recording the session's packets as `.mcpr` (the format other replay tools read), playing
  them back, and its camera paths and timeline (`studio/`, `studio_ui.rs`). It uses no address and carries no
  branding, and mods and tools can build on the recordings. It reads the network and rendering through the
  platform, and stays off by default.
- **UI pages:** a general API for pages a mod, a port or the Horizon layer adds (the settings shell, its rows,
  search and undo, today `settings_shell.rs` and `extras_ui.rs`'s pages). The engine exposes screens: open one, draw on
  the GUI layer, take input and focus. The platform gives the page API stable types (as #1608 does for the mod
  API). Horizon Settings, the controller pages and the PvP & HUD page all become clients of it.
- **PvP & HUD:** the HUD widget framework (placing, sizing and toggling HUD elements, `hud_layout.rs`) is
  platform. The PvP presets and Horizon's own widgets (keystrokes, CPS, reach readout, `pvp.rs`, `pvp_hud.rs`)
  are Horizon extras, and candidates to become native mods.

## Horizon

Horizon's own extras and its product, and every built-in address of an outside service vanilla doesn't contact.

- **Belongs:** Horizon's extras (zoom, Mouse Tweaks, sorting, smooth chat, the PvP & HUD presets and widgets, the
  music page, keystrokes, auto-reconnect...), cosmetics, emotes, the wardrobe and the shop, sponsors, the official
  and partner servers in the list, Play with Friends and its relay, Discord, the "I'm playing" ping, telemetry and
  benchmark uploads, releases and updates, the hosting supervisor, the launcher.
- **Examples:** `horizon-addresses`, `horizon-cosmetics`, `horizon-friends`, `horizon-relay`, `horizon-telemetry`,
  `horizon-bench`, `horizon-release-sign`, `horizon-server`, `horizon-mod-registry`, `launcher/`. Still in engine
  crates today: `game/cosmetics.rs`, `game/emotes.rs`, `game/discord.rs`, `game/extras.rs`, `game/tweaks.rs`,
  `heartbeat.rs`, `multiplayer.rs` (`add_official_server`), `extras_ui.rs`, `pvp.rs`.
- **Rules:**
  - It uses the platform's public API only, never the engine.
  - Until it moves, every Horizon feature in the game sits behind the `horizon-extras` feature
    ([forking.md](forking.md)), so [horizon-plain](https://github.com/horizon-rust/horizon-plain) builds without it.

## Tools

Developer tools that no player runs: `xtask`, `horizon-bots`, `horizon-mod-api-gen-tests`, and the Python in
`tools/`. They may depend on anything, and no shipped crate depends on them.

## Where a new feature goes

The rule, in order (duckybertduck's):

1. Vanilla has it: **engine**. Hooks into it are fine there.
2. It is Horizon's own (an address, the website, the name or branding): **Horizon**.
3. It helps others build on the project (mods, ports, other clients, tools): **platform**.
4. Anything else: **Horizon**, or a native mod that players add themselves.

| I add... | Layer | Where |
|---|---|---|
| A vanilla behaviour, block, mob, packet, command or screen | engine | the crate that owns the area ([CLAUDE.md](../CLAUDE.md) crate table) |
| A speed-up that keeps vanilla's results | engine | next to the code it speeds up |
| A call to a Mojang or Microsoft service vanilla makes | engine | `auth.rs`, `realms/`, `skins.rs`, `horizon-server-files` |
| A place mods, ports or Horizon can hook into | engine + platform | an extension point in the engine crate, naming no type from above; its public face in the platform |
| A mod hook, host method or mod format | platform | `horizon-mods`, the mod API (today: `game/mod_api_host.rs`, `game/native_*.rs`) |
| A shader or resource pack format vanilla doesn't read | platform | `horizon-shaders`, `horizon-optifine`, `horizon-polytone`, or a new platform crate |
| A dedicated server option (anticheat, chat filter) | platform | `horizon-anticheat`, `horizon-chat-filter` |
| Controller support: a device, bindings, glyphs, pad menus | platform | the pad modules, on the engine's input hooks ([Controllers](#controllers-splitscreen-replays-and-ui-pages)) |
| Splitscreen or more players on one machine | platform | the `split` modules, on the engine's compositor |
| A render pipeline capability vanilla's path doesn't use but a higher layer can't add without rebuilding the pipeline (the compositor) | engine | `gfx.rs` and `gfx/`, doing nothing on vanilla's path |
| A UI page others can add to, a HUD widget framework, a replay format | platform | the page API, `hud_layout.rs`, `studio/` |
| A Horizon extra (a setting, screen or HUD element vanilla doesn't have) | Horizon | its own module behind `horizon-extras`, built on platform API; or an optional native mod that players add themselves |
| Anything that talks to playhorizon.dev, a partner or a third-party service by itself | Horizon | a Horizon crate, its address in `horizon-addresses` |
| Cosmetics, emotes, the shop, sponsors, Discord, telemetry, official servers | Horizon | `horizon-cosmetics`, `horizon-telemetry`..., behind `horizon-extras` |
| A launcher page | Horizon | `launcher/src/` |
| A harness, script or dev tool | tool | `tools/`, `xtask/` |
| A new crate | its layer | give it a layer in `tools/layers.json` in the same PR |

Code that belongs to another layer than the crate it has to live in today (a Horizon extra in
`horizon-client-render`, say) goes in its own module, never mixed into an engine module. Horizon code in the game
also goes behind `horizon-extras`. That keeps each later move a pure file move.

## The Horizon layer in its own repository

The target, after the split. It is not built yet: nothing moves before the split below is done.

**What a build of this repository gives.** A complete game: vanilla play, the platform (mods, shader packs,
controllers, splitscreen, Replay Studio, HDR, the dedicated server) and no Horizon layer. That means no extras, cosmetics, shop, telemetry,
sponsors or official server, no built-in outside address, and no launcher. Forks and builds from source get exactly
that. horizon-plain becomes a plain build of this repository.

**The seam.** The Horizon repository has to plug into the game without changing it:

1. No engine or platform code names a Horizon crate. The layer check refuses it.
2. The Horizon layer uses the platform's public API only. The layer check refuses its engine dependencies, and
   today's are on the exceptions list.
3. Horizon's in-game parts (extras, cosmetics, emotes, Discord, Play with Friends, the ping) are crates linked into
   the official game build in CI, plugged in through the platform's hooks. Players never compile them: a native
   mod is source the launcher compiles per instance, so it is not the way. The official executable is a thin
   binary crate in the Horizon repository that links the platform and the Horizon crates; this repository keeps
   its own plain executable. The game runs the same without them.
4. Every Horizon address is in `horizon-addresses`, which moves with the layer.
5. The launcher is Horizon's: it moves too. It installs the game from this repository's releases and adds the
   Horizon layer to it.

**Every official download includes it.** These paths must ship the Horizon layer:

| Path | Today |
|---|---|
| The launcher's release builds, every platform and format: Linux `.tar.gz` and AppImage, macOS `.zip` and signed `.dmg`, Windows `.zip` and `horizon-launcher-setup*.exe` | `.github/workflows/launcher.yml` |
| GitHub releases (`launcher@X.Y.Z`) | `launcher.yml`'s release job, `tools/release_train.py` |
| playhorizon.dev's download page | horizon-rust/horizon-site, reading `launcher/links.json` and the releases |
| The launcher's self-update | `launcher/src/backend.rs` (`stage_update`), `launcher/src/update_handoff.rs`, `launcher/src/nightly.rs` |
| The game builds the launcher installs, so the in-game part reaches every player | `.github/workflows/nightly-build.yml`, `release.yml`, `game-build.yml` |
| The links the launcher and the site read (Discord, sponsors, website, official server) | `launcher/links.json` |

These stay without it: builds of this repository from source, horizon-plain, and `packaging/server/install.sh`
and `install.ps1` (the dedicated server). The server's anticheat and chat filter are platform features.

## Extras that may leave the game

Horizon's extras are Horizon-layer code. Each one is a candidate to become an optional native mod that players add
themselves, or stays a cfg-gated module linked into the official build. None moves yet; the list is here so new work doesn't add to them. See [extras.md](extras.md) for each one:

- zoom (OptiFine-style), keystrokes and FPS overlays, own name tag;
- Mouse Tweaks controls, sorting, scroll-wheel settings, smooth chat;
- auto-reconnect, UI-sound channels, the music page;
- the PvP & HUD presets and widgets, the AI agent page;
- the Horizon Settings menu itself, the advanced menu that hosts them;
- the renderer's extras: fullbright, dynamic lights, hidden block-break particles;
- chunk pregeneration.

The minimap already moved to a native mod ([extras.md](extras.md#moved-to-native-mods)). Splitscreen, Replay
Studio and controllers are not extras: they are platform ([above](#controllers-splitscreen-replays-and-ui-pages)).

## File size

A Rust file over 3000 lines may not grow compared with the base of the branch. `game.rs` (~21,700 lines),
`game/remote.rs` (~10,900) and `gfx.rs` (~10,100) are the worst; they slow incremental builds. Smaller files may
grow to 3000 lines, and a big file may shrink or stay as it is. There is no list of files, so no check breaks on
an unrelated edit. Generated files and `third_party/` are not checked.

## The checks

- `python3 tools/check_layers.py` (#12983) reads the workspaces' `cargo metadata` (no build, about a second) and
  `tools/layers.json`, which gives every crate its layer and each layer what it may use. It fails on:
  - a crate with no layer;
  - a dependency its layer may not use that `allowed` doesn't list;
  - a branch adding an `allowed` entry for a crate that already had a layer (a crate newly split out of another
    may carry the old crate's exceptions);
  - shipped code depending on a tool.

  Each failure names the section of this page that applies. An `allowed` entry with no dependency left is only a
  note.
- `python3 tools/check_addresses.py` (#12990) checks the built-in addresses ([below](#built-in-addresses)).
- `python3 tools/check_file_size.py` (#13015) checks the file-size rule above.

All three run in CI's verify job and in `tools/pr_ready.py`.

### Today's exceptions

47, on main at d5504cd328 (2026-11-08):

| Crate | Uses, against the rule |
|---|---|
| `horizon-client` | platform: `horizon-anticheat`, `-chat-filter`, `-dualsense`, `-hdr`, `-mod-api-gen`, `-mods`, `-optifine`, `-shader-api`, `-shaders`, `-theme`; Horizon: `horizon-bench`, `-cosmetics`, `-friends`, `-telemetry` |
| `horizon-client-lib` | the same, plus `horizon-mcp`, minus `horizon-mod-api-gen` |
| `horizon-client-render` | platform: `horizon-hdr`, `-mod-api-gen`, `-optifine`, `-polytone`, `-shader-api`, `-shaders`, `-theme`; Horizon: `horizon-bench`, `-cosmetics`, `-telemetry` |
| `horizon-client-input` | platform: `horizon-dualsense`, `horizon-theme` |
| `horizon-launcher` (Horizon) | engine: `horizon-core`, `horizon-prepare`, `horizon-protocol`, `horizon-world` |
| `horizon-server` (Horizon) | engine: `horizon-core`, `horizon-server-files`, `horizon-world` |

No platform crate depends on Horizon.

## Built-in addresses

An address is built in when the code contacts it without the player typing it. Engine code may hold vanilla's own
endpoints. Every other built-in address belongs to the Horizon layer, in one list file:
`crates/horizon-addresses/src/lib.rs` (#12993). The game's Horizon crates and the launcher read it, and it moves
with the Horizon layer. Platform code gets its addresses passed in.

`tools/check_addresses.py` reads the string literals of shipped Rust and finds every host. It skips tests,
placeholders (`example.com`, `*.test`) and local IPs. `tools/addresses.json` says where each host may be:

- `homes`: the list file;
- `vanilla`: vanilla's endpoints, allowed anywhere;
- `not_addresses`: strings that look like hosts but are not contacted, each with its reason;
- `allowed`: today's addresses outside the list file, as "path host".

`allowed` only shrinks. A branch may move an entry with its code, but it may not add a host or grow the list.

Vanilla's endpoints (engine, the same as Java Edition):

| Address | For | Where |
|---|---|---|
| `discovery.minecraftservices.com`, `api.minecraftservices.com` | services discovery, profile keys, player certificates | `auth.rs`, `horizon-server-files/src/profiles.rs` |
| `textures.minecraft.net` | skins and capes | `skins.rs` |
| `pc.realms.minecraft.net`, `java.frontend.realms.minecraft-services.net` | Realms | `realms/client.rs` |
| `aka.ms/…` | vanilla's help links (accessibility, moderation, EULA, Realms terms), opened only on a click | `game/options.rs`, `game/screens.rs`, `ban_notice.rs`, `realms.rs`, `eula.rs` |
| `libraries.minecraft.net`, `piston-meta.mojang.com`, `resources.download.minecraft.net` | the vanilla jars, libraries and assets | `horizon-prepare`, launcher |

Horizon's own and third-party addresses (Horizon layer):

| Address | For | Where today |
|---|---|---|
| `play.playhorizon.dev` (status `:24454`), previously `horizon.bunchy7s.dev` | the official server in the list, its status | `horizon-addresses` (#12993), `launcher/links.json` |
| `cosmetics.playhorizon.dev` | cosmetics, emotes, presence | `horizon-addresses` (#12993) |
| `telemetry.playhorizon.dev` | opt-in telemetry, the "I'm playing" ping, benchmark uploads | `horizon-addresses` (#12993) |
| `stun.cloudflare.com`, `stun.l.google.com` | Play with Friends NAT discovery | `horizon-addresses` (#12993) |
| `afri-network.co.za` | the withdrawn partner server, so the game can remove the entry it added | `multiplayer.rs` (`WITHDRAWN_PARTNER`) |
| `horizon-rust.github.io/horizon-mods`, `github.com/horizon-rust/horizon-mods` | the mod registry index and files: a build of this repository alone asks the player for a registry | `mod_registry.rs`, `horizon-mods/src/index*.rs`, `cargo-horizon-mod` |
| `api.modrinth.com`, `cdn.modrinth.com` | the in-game resource pack browser, modpack porting | `pack_browser.rs`, `horizon-port-pack` |
| `github.com`, `raw.githubusercontent.com`, `gitlab.com`, `maven.neoforged.net` | modpack porting's sources | `horizon-port-pack` |
| `playhorizon.dev` | the download link shown with no resources | `no_resources.rs` |
| `api.github.com`, `discord.com` | hosting: updates, Discord webhooks | `horizon-server` |
| `api.github.com`, `uploads.github.com`, `playhorizon.dev` | the registry's submission service | `horizon-mod-registry` |
| Modrinth, CurseForge, Fabric, Quilt, Forge, NeoForge, GitHub, Microsoft and Xbox sign-in, Tailscale, `discord.gg`, `mclo.gs`, Sculk Hosting, `static.rust-lang.org`, GitHub Sponsors | the launcher's browsers, loaders, sign-in, logs, hosting, partner and sponsor pages | `launcher/src/`, `launcher/links.json` |

These are not addresses:

- `bluemap_detect.rs` lists dynamic-DNS suffixes, to guess a server's own map host;
- `horizon-server/src/discord.rs` lists the hosts a webhook URL may have;
- the Windows, Apple and SOAP schema URLs in manifests and requests, and the UPnP multicast group.

## Reviewer checklist

- The PR names its layer, and new code sits in a crate or module of that layer (the table above).
- No dependency its layer may not use. CI fails on one; a module in an engine crate that calls platform or Horizon
  code is just as wrong, and so is Horizon code that reaches into the engine instead of the platform.
- Horizon features in the game are behind `horizon-extras`, and nothing new names a Horizon crate below it.
- No new built-in address outside `horizon-addresses`.
- No file over 3000 lines grows.

## Plan

Review first. The design and this plan are open for review in the tracking issue for ~48 hours. The small
mechanical steps (checks, M1–M4) continue meanwhile. M5 and M6 wait for that review, or for no objection after
48 hours. Each step is a PR under ~500 lines. Each move:

- keeps the default build the same for players and costs nothing at run time;
- keeps `--no-default-features` and horizon-plain building;
- takes its entries out of the allowlists;
- runs the harnesses `tools/parity.py --changed-files … --list` selects when it touches gameplay.

| Step | What | Removes | State |
|---|---|---|---|
| 1 | This page, first version | | #12979, merged |
| 2 | Layer check in CI and pr_ready, with the PR template's checklist | | #12983, merged |
| 3 | Address check in CI and pr_ready | | #12990, merged |
| 4 | File-size check in CI and pr_ready | | #13015, merged |
| 2b | The portable crates' rule in the layer check ([Portable crates](#portable-crates)) | | #13002 (NikitaMGrimm), merged |
| M1 | `horizon-addresses`, the address list file; the Horizon crates read it | 7 address entries | #12993, merged |
| M2 | `horizon-system` (engine): the hardware probes leave `horizon-telemetry`, and the crash report and GPU info read them there; `horizon-platform`, the platform's public API, re-exports them for the Horizon layer | prepares M3 | #13058, merged |
| M3 | The renderer's telemetry module leaves `horizon-client-render` for `horizon-client-lib`, with Horizon's other in-game parts; the renderer offers the adapter (`gfx::adapter`) and a crash-report hook (`crash::on_report`) | render → telemetry, bench | #13060 |
| M4 | Cosmetics leave `horizon-client-render`: the tab list's mark through a name-decoration callback (#13094); the settings lines leave with `extras.rs` and `extras_ui.rs` in M5, as clients of the UI page API | render → cosmetics | #13094, then M5 |
| M5 | Horizon's in-game parts (extras, cosmetics, emotes, Discord, Friends, ping, benchmark upload, official server) leave `horizon-client` for Horizon crates linked into the official build through platform hooks | client, client-lib → cosmetics, friends, telemetry, bench | after review |
| M6 | The engine crates stop depending on platform crates: the renderer's shader-pack, pack-format, HDR and theme code moves behind engine extension points; the mod host and agent control move to platform crates; controllers, splitscreen, Replay Studio and the UI page API move to platform crates on the engine's input-source, compositor and screen hooks; the launcher and `horizon-server` reach the engine through platform API | the 29 engine → platform and 7 Horizon → engine exceptions | after review; HDR's part with NikitaMGrimm, below |
| P | Optional, longer: gameplay into the portable crates. `Game` holds the world and its presentation (renderer, screens, particles, sound): split its state module by module, turn gameplay's direct particle and sound calls into level events as vanilla's server sends them, then move each clean module into a portable crate | | any time, small PRs |
| R | The Horizon layer moves to its own repository and ships in every official download (the table above) | | after the split, Ludovic's call |

HDR in M6: moving HDR behind engine extension points moves the frame pipeline (float targets, tone mapping,
present). NikitaMGrimm leads HDR, so that part is planned with them and comes after HDR's step 5 (float targets on
by default), so the pipeline moves once. HDR off stays bit-exact.

The mod API work on arismoko/horizon lands upstream as part of the platform. The API owns its types,
`horizon-mod-runtime` gets no game dependency, and its CI property check stays as a stricter check inside the
platform.
