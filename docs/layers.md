# Layers: engine, platform, Horizon

Horizon is three things in one repository: a vanilla engine, a platform that makes the vanilla
game easier to change, and Horizon's own product on top. This page says which code belongs to
which, which crate may depend on which, and where every built-in outside address lives. A
contributor fixing vanilla behaviour should never need to touch cosmetics, the shop or a server
address, and a fork should be able to take the engine and the platform without Horizon's product.

It stays one repository. The split is enforced crate by crate, and inside the mixed crates
(`horizon-client`, `horizon-client-render`, `horizon-client-lib`) module by module as code moves.

Credits: the three layers, the dependency rule, the address rule and the plan come from
duckybertduck's proposal ("3-layer split: engine / platform / Horizon", #contributors, 2026-11-08).
The rule that the mod API owns its types and depends on nothing in the engine but its vocabulary
crate, checked as a property in CI, comes from arismoko's mod API work on
[arismoko/horizon](https://github.com/arismoko/horizon) ("API boundary: four patterns",
arismoko/horizon#1608).

## The layers

| Layer | What belongs there | Network |
|---|---|---|
| **foundation** | Minecraft's vocabulary and OS helpers with no game behaviour: positions, Java `Random`, `jmath`, noise, identifiers, NBT, thread priorities | none |
| **platform** | What makes the vanilla game nicer to change and run, with no Horizon branding: the mod API and mod formats, the shader API and shader/resource pack formats, HDR, controllers, the design tokens, the dedicated server's anticheat and chat filter, agent control (MCP) | may have network code, never a built-in address: every address comes from the player, a file or a setting |
| **engine** | Vanilla: gameplay, world, generation, protocol, rendering, sound, screens, saves, the integrated and dedicated servers | contacts only what vanilla contacts (Mojang and Microsoft services, the servers the player joins) |
| **Horizon** | Horizon's product: cosmetics, emotes, the shop and wardrobe, the official and partner servers, Play with Friends and its relay, Discord, the "I'm playing" ping, telemetry and benchmark uploads, releases and updates, the launcher, hosting | holds every built-in address of an outside server that vanilla doesn't contact |
| **tool** | Developer tools that no player runs: `xtask`, load-test bots, generator tests | anything; nothing shipped depends on a tool |

### Order

```
foundation  <  platform  <  engine  <  Horizon
```

A crate depends only on crates of its own layer or of a lower one (normal and build
dependencies; dev-dependencies are free, tests may use anything).

The proposal leaves the order of engine and platform open. Platform goes below the engine
because that is how the code already works: the renderer uses the shader API, the pack formats
and HDR; the game implements the mod API's traits (arismoko/horizon#1608: the API declares what a
mod may ask, the game implements it, the API names no engine type). A platform crate is a
contract or a format; where it plugs into the game is engine code. Platform features that need
the game's own types (the Horizon Settings pages, the mod host, the HDR output pass) are modules
of the engine crates, tagged platform below, until they move.

## Crates

| Crate | Layer | Why |
|---|---|---|
| `horizon-core` | foundation | vocabulary and simulation primitives; the mod API's only engine-side dependency |
| `horizon-threads` | foundation | OS scheduling hints |
| `horizon-shader-api` | platform | Horizon's WGSL shader packs |
| `horizon-shaders` | platform | `shaders/` pack format |
| `horizon-optifine`, `horizon-polytone` | platform | resource pack formats beyond vanilla |
| `horizon-hdr` | platform | HDR swapchain metadata |
| `horizon-dualsense` | platform | controller output |
| `horizon-theme` | platform | design tokens for Horizon-only screens |
| `horizon-mods`, `cargo-horizon-mod`, `horizon-mod-api-gen`, `horizon-mod-deps`, `horizon-port-pack` | platform | mod formats, the native mod API generator, the SDK, modpack porting |
| `horizon-anticheat`, `horizon-chat-filter` | platform | dedicated server options, off by default |
| `horizon-mcp` | platform | agent control protocol |
| `horizon-world`, `horizon-protocol`, `horizon-items`, `horizon-command`, `horizon-assets`, `horizon-prepare`, `horizon-server-files`, `horizon-narrator` | engine | vanilla |
| `horizon-client-core`, `horizon-client-input`, `horizon-client-render`, `horizon-client-lib`, `horizon-client` | engine | vanilla client and servers (mixed today: see below) |
| `horizon-cosmetics` | Horizon | cosmetics and emotes, `cosmetics.playhorizon.dev` |
| `horizon-friends`, `horizon-relay` | Horizon | Play with Friends, STUN servers, the relay |
| `horizon-telemetry`, `horizon-telemetry-server`, `horizon-bench` | Horizon | opt-in collection, its ingest server, benchmark reports for playhorizon.dev (bench reads telemetry's hardware probes) |
| `horizon-release-sign`, `horizon-server`, `horizon-mod-registry` | Horizon | signed releases and updates, the hosting supervisor (GitHub updates, Discord webhooks), the registry's submission service |
| `launcher/` | Horizon | the launcher (its own workspace) |
| `xtask`, `horizon-bots`, `horizon-mod-api-gen-tests` | tool | dev tools |

`horizon-bench` and `horizon-telemetry` are Horizon because they upload to playhorizon.dev.
Their hardware probes and the report format have no address and could split out into a platform
crate; that is one of the moves below.

### Modules of the mixed engine crates

The engine crates still hold platform and Horizon code. These are the modules that leave, or
that stay engine code tagged with their layer until they move. Paths are under
`crates/horizon-client/src/` (`G:` = `game/`) and `crates/horizon-client-render/src/` (`R:`).

| Layer | Modules |
|---|---|
| Horizon | G: `cosmetics.rs` + `cosmetics/`, `emotes.rs`, `emote_wheel.rs`, `names.rs` (Horizon marks), `discord.rs`, `friends.rs`, `native_client_services.rs` (relay); `discord_presence.rs`, `heartbeat.rs`, `friends_screen.rs`, `benchmark.rs`, `multiplayer.rs` (`add_official_server`, `withdraw_partner_servers`), `mod_registry.rs` (registry index), `no_resources.rs` (download link); R: `telemetry.rs`, `crash.rs` (report upload), `extras.rs` / `extras_ui.rs` (cosmetics settings), `tab_list.rs` (Horizon marks) |
| platform | G: `mods.rs`, `mod_api_host.rs`, `native_*.rs` (the native mod host), `agent*.rs`, `control.rs`, `extras.rs`, `tweaks.rs`, `pvp.rs`, `hud_editor.rs`; `mod_api.rs`, `mod_content*.rs`, `control.rs`, `extras_hud.rs`, `studio/`, `inv_sort.rs`, `reconnect.rs`, `perf_overlay.rs`; R: `options_ui.rs` (`Page::Horizon`, `Page::Mods`), `graphics_ui.rs`, `shader_*.rs`, `hdr_calibration.rs`, `pack_browser.rs`, `mod_browser.rs`, `bluemap*.rs`, `minimap.rs`, `pvp*.rs`, `split_*.rs`, `studio_ui.rs`, `cit*.rs`, `custom_sky.rs`, `dynamic_light.rs` |
| engine | everything else |

The `horizon-extras` feature ([forking.md](forking.md)) already gates the Horizon modules' entry
points, and [horizon-plain](https://github.com/horizon-rust/horizon-plain) builds the game without
them. The layers make that the shape of the code instead of a set of `cfg!` gates.

## Today's violations

Normal and build dependencies from a lower layer to a higher one, on main at d5504cd328
(2026-11-08): **11**.

| Crate (layer) | Depends on (Horizon) |
|---|---|
| `horizon-client-render` (engine) | `horizon-cosmetics`, `horizon-telemetry`, `horizon-bench` |
| `horizon-client-lib` (engine) | `horizon-cosmetics`, `horizon-friends`, `horizon-telemetry`, `horizon-bench` |
| `horizon-client` (engine) | `horizon-cosmetics`, `horizon-friends`, `horizon-telemetry`, `horizon-bench` |

Every one of them is the engine reaching up into Horizon; no platform crate depends on the
engine. The check (`tools/check_layers.py`, next PR) keeps this list in an allowlist that may
only shrink: a new upward edge fails CI, and an edge that disappears must be removed from the
list in the same PR.

## Built-in addresses

An address is built in when the code contacts it without the player typing it. Engine code may
hold vanilla's own endpoints; every other built-in address belongs to the Horizon layer, in one
place per workspace (the game's, then the launcher's), listed here. Today they are spread over
the files below; the address check (after the layer check) lists them in an allowlist that may
only shrink, and the moves bring them into one module.

Vanilla's endpoints (engine, same as Java Edition):

| Address | For | Where |
|---|---|---|
| `discovery.minecraftservices.com`, `api.minecraftservices.com` | services discovery, profile keys, player certificates | R: `auth.rs`, `horizon-server-files/src/profiles.rs` |
| `textures.minecraft.net` | skins and capes | R: `skins.rs`, G: `remote.rs` |
| `pc.realms.minecraft.net`, `java.frontend.realms.minecraft-services.net` | Realms | `realms/client.rs` |
| `aka.ms/…` | vanilla's help links (accessibility, moderation, EULA, Realms terms), opened only on a click | G: `options.rs`, `screens.rs`, R: `ban_notice.rs`, `realms.rs`, `horizon-server-files/src/eula.rs` |
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
| `api.modrinth.com`, `cdn.modrinth.com` | the in-game resource pack browser, modpack porting | R: `pack_browser.rs`, `horizon-port-pack` |
| `github.com`, `raw.githubusercontent.com`, `gitlab.com`, `maven.neoforged.net` | modpack porting's sources | `horizon-port-pack` |
| `playhorizon.dev` | download link shown with no resources, LAN refusal text | `no_resources.rs`, `lan/mod.rs` |
| `api.github.com`, `discord.com` | hosting: updates, Discord webhooks | `horizon-server` |
| `api.github.com`, `uploads.github.com`, `playhorizon.dev` | the registry's submission service | `horizon-mod-registry` |
| Modrinth, CurseForge, Fabric, Quilt, Forge, NeoForge, GitHub, Microsoft and Xbox sign-in, Tailscale, `discord.gg`, Sculk Hosting, `static.rust-lang.org` | the launcher's browsers, loaders, sign-in, hosting and partner pages | `launcher/src/` |

Not addresses: `bluemap_detect.rs` lists dynamic-DNS suffixes to guess a server's own map host;
`horizon-server/src/panel.rs` matches `.ts.net`; Windows and Apple schema URLs in manifests.

The address check reads string literals in shipped Rust, skips tests and examples, and fails on
a host outside its allowlist. An address that needs to stay outside the address module (a
vanilla endpoint in engine code, a matcher like the ones above) is listed with its reason.

## Plan

One small PR at a time, in this order:

1. This page.
2. `tools/check_layers.py` and a CI step: the crate table above as data, the violations as an
   allowlist that only shrinks, with tests.
3. The address check: every built-in address above in one module per workspace, enforced by CI.
4. Moves, most contributor value first: the extras (cosmetics, emotes, Discord, the official
   server, telemetry and the ping) out of the engine crates into Horizon-layer crates behind the
   `horizon-extras` entry points, then the renderer's Horizon modules. Each move keeps the
   default build the same for players, costs nothing at run time and keeps
   `--no-default-features` and horizon-plain building. Each one removes its lines from the
   allowlists.
5. Optionally, a lines-of-code check per file.

arismoko's API boundary work (the mod API with its own types, `horizon-mod-runtime`, a CI
property check that `horizon-mod-api` depends only on `horizon-core`) lands upstream from
arismoko/horizon; with it the mod API becomes a platform crate under this page's rule, and its
own stricter check stays.
