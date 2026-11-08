# Layers: engine, platform, Horizon

Horizon is three things in one repository:

- a **vanilla engine**;
- a **platform** that makes the vanilla game easier to change and run;
- **Horizon's own product** on top.

This page says what belongs to each layer, which crate may depend on which, and where a new feature goes.
Contributors fixing vanilla behaviour should never have to touch cosmetics, the shop or a server address. A fork
should be able to take the engine and the platform without Horizon's product.

CI checks the rules, so review doesn't have to. Everything stays in one repository: crates are checked now, and
the modules of the mixed crates follow as code moves.

Credits: the three layers, the dependency rule, the address rule and the plan come from duckybertduck's proposal
("3-layer split: engine / platform / Horizon", #contributors, 2026-11-08). The rule that the mod API owns its types
and the property check that keeps engine types out of it come from arismoko's mod API work on
[arismoko/horizon](https://github.com/arismoko/horizon) (arismoko/horizon#1608).

## The rule

```
engine  <  platform  <  Horizon          (tool: dev tools, outside the order)
```

A crate depends only on crates of its own layer or of a lower one. The engine never depends on the platform or on
Horizon, and the platform never depends on Horizon. Normal, build and optional dependencies all count;
dev-dependencies don't, so tests may use anything.

When lower code needs something from above, the lower layer offers an extension point and the higher layer plugs
into it. The extension point is a trait, a callback list, an event or a registry, and it names no type from above.
The program players run is assembled at the top, where every layer is in reach.

## Engine

Vanilla Minecraft: Java Edition 26.3, and nothing else.

- **Belongs:** gameplay, world data and generation, block behaviour, mobs, items, commands, the network
  protocol, rendering, sound, vanilla's screens, saves, the integrated and dedicated servers. It also holds
  optimizations that keep vanilla's behaviour, and the extension points the platform plugs into.
- **Examples:** `horizon-world` (`worldgen/`, `level/redstone.rs`), `horizon-protocol` (`packets/`),
  `horizon-items`, `horizon-command`, `horizon-core`, `game/creatures.rs`, `game/integrated.rs`, `gfx.rs`,
  `mesher.rs`, `streaming.rs`, `lan/`.
- **Never:** a mod hook's implementation, a shader or pack format vanilla doesn't read, a Horizon-only setting
  or screen, a network address vanilla doesn't contact, Horizon branding.
- **Network:** only what vanilla contacts: Mojang and Microsoft services, and the servers the player joins.
- **May depend on:** engine crates.

## Platform

What makes vanilla nicer to change and run, with no Horizon branding and no address of its own.

- **Belongs:** the mod API and mod formats, the native mod host, the shader API and pack formats beyond vanilla
  (OptiFine, Polytone, `shaders/`), HDR, controllers, Horizon-only settings that are off by default (zoom, mouse
  tweaks, splitscreen), agent control (`horizon-ctl`, MCP), the dedicated server's anticheat and chat filter,
  modpack porting, the design tokens of Horizon-only screens.
- **Examples:** `horizon-shader-api`, `horizon-shaders`, `horizon-optifine` (`cem/`), `horizon-polytone`,
  `horizon-hdr`, `horizon-dualsense`, `horizon-mods`, `cargo-horizon-mod`, `horizon-anticheat`, `horizon-mcp`,
  `horizon-theme`. Still in engine crates today: `game/mod_api_host.rs`, `game/native_*.rs`, `game/extras.rs`,
  `shader_ui.rs`, `hdr_calibration.rs`.
- **Never:** a built-in address (every address comes from the player, a file or a setting), cosmetics, the shop,
  official or partner servers, uploads to playhorizon.dev.
- **Network:** allowed, but never to a built-in address.
- **May depend on:** engine and platform crates.

## Horizon

Horizon's product, and every built-in address of an outside service vanilla doesn't contact.

- **Belongs:** cosmetics, emotes, the wardrobe and the shop, the official and partner servers in the list,
  Play with Friends and its relay, Discord, the "I'm playing" ping, telemetry and benchmark uploads, releases and
  updates, hosting, the launcher, and the assembled game.
- **Examples:** `horizon-cosmetics`, `horizon-friends`, `horizon-relay`, `horizon-telemetry`, `horizon-bench`,
  `horizon-release-sign`, `horizon-server`, `launcher/`. Still in engine crates today: `game/cosmetics.rs`,
  `game/emotes.rs`, `game/discord.rs`, `heartbeat.rs`, `multiplayer.rs` (`add_official_server`).
- **Rules:** in the game, every Horizon feature sits behind the `horizon-extras` feature ([forking.md](forking.md)),
  so [horizon-plain](https://github.com/horizon-rust/horizon-plain) builds without it.
- **May depend on:** anything but tools.

## Tools

Developer tools that no player runs: `xtask`, `horizon-bots`, `horizon-mod-api-gen-tests`, and the Python in
`tools/`. They may depend on anything, and no shipped crate depends on them.

## Where a new feature goes

| I add... | Layer | Where |
|---|---|---|
| A vanilla behaviour, block, mob, packet, command or screen | engine | the crate that owns the area ([CLAUDE.md](../CLAUDE.md) crate table) |
| A speed-up that keeps vanilla's results | engine | next to the code it speeds up |
| A call to a Mojang or Microsoft service vanilla makes | engine | `auth.rs`, `realms/`, `skins.rs`, `horizon-server-files` |
| A place mods or packs can hook into | engine | an extension point (trait, callback, event) in the engine crate, naming no platform type |
| A mod hook, host method or mod format | platform | `horizon-mods`, the mod API (today: `game/mod_api_host.rs`, `game/native_*.rs`) |
| A shader or resource pack format vanilla doesn't read | platform | `horizon-shaders`, `horizon-optifine`, `horizon-polytone`, or a new platform crate |
| A setting or screen vanilla doesn't have, off by default | platform | its own module, reached from Horizon Settings |
| A dedicated server option (anticheat, chat filter) | platform | `horizon-anticheat`, `horizon-chat-filter` |
| Anything that talks to playhorizon.dev, a partner or a third-party service by itself | Horizon | a Horizon crate, with its address in the address list below |
| Cosmetics, emotes, the shop, Discord, telemetry, official servers | Horizon | `horizon-cosmetics`, `horizon-telemetry`..., behind `horizon-extras` |
| A launcher page | Horizon | `launcher/src/` |
| A harness, script or dev tool | tool | `tools/`, `xtask/` |
| A new crate | its layer | add it to [`tools/layers.json`](../tools/layers.json) in the same PR |

Code that belongs higher than the crate it has to live in today (a platform setting in `horizon-client-render`,
say) goes in its own module, never mixed into an engine module. Horizon code in the game also goes behind
`horizon-extras`. That keeps each later move a pure file move.

## The check

`python3 tools/check_layers.py` runs in CI's verify job and in `tools/pr_ready.py`. It reads the four workspaces'
`cargo metadata` (no build, about a second) and [`tools/layers.json`](../tools/layers.json), which gives every
crate its layer. It fails when:

- a crate has no layer;
- a dependency goes up a layer and `allowed` doesn't list it;
- a pull request adds an entry to `allowed` between two crates that already had layers;
- shipped code depends on a tool.

Each failure names the section of this page that applies. An `allowed` entry that has no dependency left is only
reported: take it out when you like. `--list` prints every crate's layer and the upward dependencies left.

### Today's exceptions

Today `allowed` has 40 entries, all from the three mixed engine crates (main at d5504cd328, 2026-11-08):

| Crate | Depends upward on |
|---|---|
| `horizon-client` | platform: `horizon-anticheat`, `-chat-filter`, `-dualsense`, `-hdr`, `-mod-api-gen`, `-mods`, `-optifine`, `-shader-api`, `-shaders`, `-theme`; Horizon: `horizon-bench`, `-cosmetics`, `-friends`, `-telemetry` |
| `horizon-client-lib` | the same, plus `horizon-mcp`, minus `horizon-mod-api-gen` |
| `horizon-client-render` | platform: `horizon-hdr`, `-mod-api-gen`, `-optifine`, `-polytone`, `-shader-api`, `-shaders`, `-theme`; Horizon: `horizon-bench`, `-cosmetics`, `-telemetry` |
| `horizon-client-input` | platform: `horizon-dualsense`, `horizon-theme` |

No platform crate depends on Horizon. `horizon-client` is the gameplay library and the executable at once. It
splits in two: the engine's gameplay library, and a thin executable at the Horizon layer that wires in the
platform and Horizon crates.

## Built-in addresses

An address is built in when the code contacts it without the player typing it. Engine code may hold vanilla's own
endpoints. Every other built-in address belongs to the Horizon layer, in one file: `crates/horizon-addresses/src/lib.rs`,
which the game's Horizon crates and the launcher both read. Platform code gets its addresses passed in from there.

`python3 tools/check_addresses.py` (CI's verify job, and pr_ready's "Addresses" step) reads the string literals of
shipped Rust and finds every host: a URL's host, or a whole string that is a host name. Tests are skipped, and so are
placeholders (`example.com`, `*.test`) and local IPs. [`tools/addresses.json`](../tools/addresses.json) says where
each host may be:

- `homes`: the address home above, where anything goes;
- `vanilla`: vanilla's endpoints, allowed anywhere;
- `not_addresses`: strings that look like hosts but are not contacted, each with its reason;
- `allowed`: today's addresses outside the home, as "path host".

`allowed` only shrinks. A branch may move an entry with its code to another file, but it may not add a host or grow
the list: a new address goes in the home. An entry whose address left its file is reported as a note. `--list`
prints every host found and its files.

Today's addresses outside the home are below.

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
| `play.playhorizon.dev` (status `:24454`), previously `horizon.bunchy7s.dev` | the official server in the list, its status | `horizon-telemetry/src/server_status.rs`, `launcher/links.json` |
| `afri-network.co.za` | the withdrawn partner server, so the game can remove the entry it added | `multiplayer.rs` (`WITHDRAWN_PARTNER`) |
| `cosmetics.playhorizon.dev` | cosmetics, emotes, presence | `horizon-cosmetics/src/lib.rs` |
| `telemetry.playhorizon.dev` | opt-in telemetry, the "I'm playing" ping, benchmark uploads | `horizon-telemetry/src/lib.rs` |
| `stun.cloudflare.com`, `stun.l.google.com` | Play with Friends NAT discovery | `horizon-friends/src/stun.rs` |
| `horizon-rust.github.io/horizon-mods`, `github.com/horizon-rust/horizon-mods` | the mod registry index and files | `mod_registry.rs`, `horizon-mods/src/index*.rs`, `cargo-horizon-mod` |
| `api.modrinth.com`, `cdn.modrinth.com` | the in-game resource pack browser, modpack porting | `pack_browser.rs`, `horizon-port-pack` |
| `github.com`, `raw.githubusercontent.com`, `gitlab.com`, `maven.neoforged.net` | modpack porting's sources | `horizon-port-pack` |
| `playhorizon.dev` | the download link shown with no resources | `no_resources.rs` |
| `api.github.com`, `discord.com` | hosting: updates, Discord webhooks | `horizon-server` |
| `api.github.com`, `uploads.github.com`, `playhorizon.dev` | the registry's submission service | `horizon-mod-registry` |
| Modrinth, CurseForge, Fabric, Quilt, Forge, NeoForge, GitHub, Microsoft and Xbox sign-in, Tailscale, `discord.gg`, Sculk Hosting, `static.rust-lang.org` | the launcher's browsers, loaders, sign-in, hosting and partner pages | `launcher/src/` |

These are not addresses:

- `bluemap_detect.rs` lists dynamic-DNS suffixes, to guess a server's own map host;
- `horizon-server/src/panel.rs` matches `.ts.net`;
- the Windows and Apple schema URLs in manifests.

## Reviewer checklist

- The PR names its layer, and new code sits in a crate or module of that layer (the table above).
- No new upward dependency. CI fails on one; a module that calls up from an engine crate is just as wrong.
- Horizon features in the game are behind `horizon-extras`. Platform features that change the vanilla look are
  off by default.
- No new built-in address outside the Horizon layer.

## Plan

One small PR at a time, stacked:

1. This page.
2. `tools/check_layers.py` in CI and pr_ready, with the checklist in the PR template.
3. `tools/check_addresses.py` in CI and pr_ready: built-in addresses only in the home, the rest only shrinks.
4. Moves, most contributor value first. The extras (cosmetics, emotes, Discord, the official server, telemetry
   and the ping) leave the engine crates for Horizon crates. Then the renderer's platform and Horizon modules
   leave `horizon-client-render`. Then the executable splits from the gameplay library. Each move:
   - keeps the default build the same for players and costs nothing at run time;
   - keeps `--no-default-features` and horizon-plain building;
   - takes its entries out of `allowed`.
5. Optionally, a lines-of-code check per file.

arismoko's API boundary work brings the mod API upstream from arismoko/horizon. The API owns its types,
`horizon-mod-runtime` gets no game dependency, and a CI property check keeps `horizon-mod-api` free of engine
crates other than `horizon-core`. With it, the mod API becomes a platform crate under this page's rule, and its own
stricter check stays.
