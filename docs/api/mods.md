# Mod API

Mods are Rust crates compiled to sandboxed WebAssembly components. Client mods run in the player's game; server mods
(experimental) run wherever a world is simulated: single player, a LAN host, the dedicated server. Design and the runtime: [mods.md](../mods.md).

## Overview

A mod today can read the local player and the world clock, show chat lines only that player sees, draw a text panel
on the HUD, keep small files of its own, register chat commands, and run on every tick, frame or chat line it asks
for. A content mod adds blocks and items from JSON in its `.hmod`, copying vanilla templates, plus tags, recipes,
loot, models, textures and structures in its own namespaces. Every capability is declared in the manifest, shown to
the player before the mod is turned on, and checked against the component's real imports.

| | Count |
| --- | --- |
| Interfaces the game offers | 6: `log`, `game`, `chat`, `hud`, `storage`; `players` for server mods |
| Functions in them | 12 |
| Hooks a mod exports | 5: `init`, `tick`, `frame`, `command`, `chat`; a server mod 8: `init`, `tick`, `player-joined`, `player-left`, `chat`, `command`, `block-broken`, `block-placed` |
| Capabilities | 6: `game-read`, `chat`, `hud`, `storage`, `registries`, `players` |
| Manifest `api` lines served | 3: `0.1` (code), `0.2` (content), `0.3` (server mods, experimental) |
| SDK | the `Mod` trait (5 methods), `Registration` (4 builders), `export_mod!`, log helpers; `server::ServerMod` (8 methods), `server::Registration` (6 builders), `server::Argument`, `server::Args`, `export_server_mod!` |
| `cargo horizon-mod` | `new` (templates `code` and `content`), `build`, `run`, `test`, `check` |
| Example mods | `coords-hud` (code), `ruby-content` (content), `welcome` (server) |

## Guide

The step-by-step guide, from installing the tools to publishing on the registry, is
[Make a mod](https://playhorizon.dev/docs/make-a-mod) on the website. In short:

1. `rustup target add wasm32-unknown-unknown`, `cargo install --git https://github.com/horizon-rust/horizon
   cargo-horizon-mod`, then `cargo horizon-mod new my-mod` ([Making a mod](../mods.md#making-a-mod)).
2. Implement `Mod` for your type, list the capabilities you import in `horizon-mod.toml`
   ([The manifest](../mods.md#the-manifest)).
3. `cargo horizon-mod run` builds the `.hmod` and starts the game with it as a dev mod; after an edit,
   `cargo horizon-mod build` and the running game reloads it.
4. `cargo horizon-mod test` runs the mod's `tests/*.toml` scripts in the sandbox, with no game: ticks, commands,
   chat lines and the player's position in, chat, feedback and the HUD panel checked
   ([Testing a mod](../mods.md#testing-a-mod)).
5. Content mods: [Content mods (API 0.2)](../mods.md#content-mods-api-02).

## Reference

[mods-reference.md](mods-reference.md), generated from `crates/horizon-mod-sdk/wit/horizon.wit`: every capability,
interface, type, function and hook, with the limits the guide lists (16 HUD lines of 256 characters, 8 chat lines per
call, 4 MiB of storage, fuel per call).

## Examples

- `crates/horizon-mod-sdk/examples/coords-hud`: coordinates, facing and time on the HUD; `/where`, `/coords`; storage.
- `crates/horizon-mod-sdk/examples/ruby-content`: no code; a gem and a block of ruby from vanilla templates, with
  their recipes, loot, tags, models, textures and names.
- `crates/horizon-mod-sdk/examples/welcome`: a server mod; greets players who join, announces newcomers (kept in
  storage), `/online` and `/mined`.
- `crates/horizon-mods/tests/guest`: a mod that misbehaves on command (spins, panics, grows its memory), for the
  sandbox tests.

## Ways in

One API (the WIT world) and one sandbox (WebAssembly), whatever the mod is written in. Every way in produces a
component for the same world, checked against the same capabilities and given the same limits; there is no second
scripting runtime in the game.

| Way in | For | Status | Template and example |
| --- | --- | --- | --- |
| Rust | most mods; the fastest | first-class: the SDK, `cargo horizon-mod` | `cargo horizon-mod new`, `coords-hud` |
| Data only | blocks and items with no code | first-class: content mods (API 0.2) | `cargo horizon-mod new --template content`, `ruby-content` |
| Lua scripts | people who don't want a compiler | planned: a script mod whose code is a `.lua` file, run by Horizon's own interpreter mod | planned, with the same example as Rust |
| C, C++, Zig | authors of those languages | to evaluate: bindings generated from the WIT, built without WASI | one each if it passes |
| Go (TinyGo), AssemblyScript, JavaScript / TypeScript | authors of those languages | to evaluate: each needs its toolchain to emit a component without WASI imports, or an interpreter mod as for Lua | one each if it passes |

- **Scripts run inside the sandbox.** The Lua interpreter is itself a WebAssembly mod, built in Rust with the SDK and
  shipped by Horizon. A script mod's manifest names the interpreter and its own capabilities; the game links the
  interpreter with only those, so a script can do exactly what a compiled mod with the same manifest can.
- **Hot reload.** The game keeps the interpreter compiled and loads the script into a fresh instance when the file
  changes, without restarting the game; what it saved with `storage` survives, as for dev mods.
- **Measured, not guessed.** Each way in is listed with its measured cost on the same tick-heavy example (time and
  fuel per tick, memory, load time) against Rust, and a language is supported only with a template, an example and a
  test that builds and loads it in CI.

## Stability

Preview ([stability.md](stability.md)): API 0.1 and content 0.2 are served together; a mod built for 0.1.0 keeps
loading on every 0.1.x game. Server mods are their own line, `api = "0.3"`, experimental: they may change in any
release.

## Not there yet

What real mods need next, in the order modpacks and servers need it:

- **More for server mods** (`api = "0.3"`, [Server mods](../mods.md#server-mods-api-03-experimental)): they run
  on the server thread, a LAN host and the dedicated server today, with joins, chat, typed commands in players'
  command trees, players' block breaking and placing, and `world` (read blocks and biomes, set blocks as `/setblock`
  does); next entity events, game rules, entity, item and block-state arguments.
- **World and blocks**: blocks, block states and biomes are there for server mods (`world`); next block entities,
  and `world` for client mods (raycasts, single player and LAN hosts).
- **Items and inventories**: the player's inventory, held item and open container; item stacks with components.
- **Entities**: query and change entities near a position; spawn from a mod's own entity types (data first).
- **Block entities and menus**: ticking machines with slots and their screens.
- **Screens and UI**: widgets built from vanilla's, a screen the mod owns, a settings page under Options > Mods.
- **Networking**: a channel between a mod's two halves, client and server, typed by the mod's own WIT.
- **Config**: typed settings the game shows and stores for the mod, on the server too.
- **Keys and the crosshair target**: keybinds in Controls; what the player is looking at.
- **Script mods and other languages**: see [Ways in](#ways-in).
- **SDK**: templates per kind of mod, typed wrappers in the SDK for every interface. Dev mods already reload when
  rebuilt, and `cargo horizon-mod test` runs a mod headless.

## Changelog

- API 0.3: typed server commands: `registration.commands` lists each command's arguments (`argument-kind`: word,
  text, integer, number, boolean, player, block-pos), sent in players' command trees and parsed by the server;
  `command` gets their values. SDK `Registration::command_with`, `Argument`, `Args`.
- API 0.3: capability `world` (`get-block`, `get-biome`, `set-blocks`); example `beacons`; test scripts' `blocks` and
  `expect.blocks`.
- Server mods run in the game: single player (server thread and in-frame), LAN hosts, the dedicated server.
- API 0.3 (server mods, experimental): world `horizon-server-mod`, capability `players`, the `server-events`
  hooks; SDK `server::ServerMod`; example `welcome`.
- SDK: `cargo horizon-mod test` and `horizon_mods::testing`, test scripts in TOML; the template has one.
- Experimental: `ui` button widget; `render.set-corner`, `text-width`, `frame-stats`; `input.mapping`,
  `clicks-per-second` (what the official FPS, keystrokes and zoom mods need).
- Experimental `input` (`experimental = ["input"]`): `claim-items`, `register-key`, `held`, `poll` (use-start,
  use-stop, key-down, key-up).
- Experimental `render` (`experimental = ["render"]`): HUD shapes (fill, quad, sprite, item, text), `gui-size`, camera
  `shake`, drawn by the game's own renderer.
- API 0.2 (content): data-driven blocks and items, tags, `horizon:add_to`, terrain adaptation.
- API 0.1.0: `log`, `game`, `chat`, `hud`, `storage`; `init`, `tick`, `frame`, `command`, `chat`.
