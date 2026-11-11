# Horizon documentation

To play, use [the launcher](../launcher/README.md); the project site is [playhorizon.dev](https://playhorizon.dev). These pages are for developers. What is being done now and next, the work board and how to help: [the hub](https://playhorizon.dev/hub) ([ROADMAP.md](../ROADMAP.md), [roadmap/README.md](../roadmap/README.md) for keeping it current).

## Build and run

- [AGENTS.md](../AGENTS.md) and [Architecture](architecture.md): where each part of the code lives, how `Game` is organised, which harness checks which area.
- [How Horizon is built](how-horizon-is-built.md): who writes Horizon, how changes are checked, and what is safe to run.
- [Release flow](release-flow.md): pull request checks, merging and the automatic Nightly releases that reach players.
- [Operations](operations.md): running Horizon by hand when no agent runs and the maintainer's machines are off: merging, releases, offline runners, the mods registry, Kira, the official server, security.
- [Build from source](../README.md#build-from-source): your own copy of the game, resource preparation, `cargo run`.
- [Build times](build-times.md): `tools/build_times.py` measures cold and incremental builds per crate and the critical path, writes a JSON report and a table, and compares two reports; the first numbers, Windows and Linux.
- [Native client](native-client.md): options, controls, the dedicated server, `horizon-ctl` scripting, packaging, architecture.
- [Controllers](controllers.md): playing with PlayStation, Xbox and Nintendo Switch Pro controllers, Steam Deck built-in controls, analog movement, and the input layer.
- [Host a server](hosting.md): install `horizon-server`, prepare the game files, first run, `server.toml`, the commands, the web panel, backups, services and Docker; what works today and what doesn't yet.
- [A server's levels](server-levels.md): how the dedicated server and single player's server thread tick, save and travel between dimensions.
- [Velocity forwarding](velocity-forwarding.md): the player-information exchange behind a Velocity proxy and how it was checked.
- [The anticheat](anticheat.md): the dedicated server's own checks, off unless `server.toml`'s `[anticheat]` turns them on: violation levels, setback, alerts, kicks, the log, and how to write a check.
- [The chat filter](chat-filter.md): the dedicated server refuses hate slurs in chat, message commands, signs, anvil names and book titles: the list and its normalising, mutes, temporary bans, `[chat_filter]`.
- [Nix and NixOS](nix.md): the flake's packages: server, engine, prepare, client and launcher.
- [Hosting a server: design](server.md): `horizon-server`, the design and the plan: install, `server.toml`, the CLI, the web panel, backups, services, Docker, and the bot benchmark against vanilla.
- [The next server: design](server-next.md): Horizon's dedicated server against vanilla and Pumpkin (measured), the targets, parallel ticking that keeps vanilla's order, the network front, the Horizon fast path, the plugin API and the plan.
- [Survival multiplayer](smp.md): the official server's `smp` native mod (economy, market, trades, games of chance, spawn, homes, teleport requests, warps, land claims) and the server hook points it needs; not part of Horizon's own server.
- [For server owners](owners.md): every `server.properties` key next to its `server.toml` field.
- [Agent control](agent-control.md): what AI agents can drive in the launcher and the game, and the design of the control socket, `horizon-ctl` and MCP.
- [horizon-ctl commands](horizon-ctl-commands.md): every command `horizon-ctl` takes, its CLI arguments, and the corresponding MCP tools for driving the game.
- [Graphics](graphics.md): the graphics settings screen (pages, search, descriptions), the graphics benchmark and what it measured.
- [HDR and render passes](hdr-passes.md): what a pass on the world's picture (upscaling, anti-aliasing, frame generation, post effects, captures) keeps so HDR works, and how to check it.
- [Server against Pumpkin](performance/server-vs-pumpkin.md): Horizon's server and Pumpkin (Rust) under the same bots at 0, 20, 60 and 100 players: tick time, CPU, memory, chunks, join; where Horizon loses, and how to run it again.
- [Benchmark](benchmark.md): Horizon against vanilla 26.3 and vanilla with performance mods, on the same machine, world, path and settings: what is measured, how it is kept fair, how to reproduce it.
- [Extras](extras.md): optional zoom, minimap, FPS and keystroke overlays and auto-reconnect, all off by default.
- [PvP & HUD](pvp.md): optional client settings for PvP players (custom crosshair, block outline, hit color, HUD widgets), fair-play boundaries, and why each is off by default.
- [Cosmetics](cosmetics.md): wearable items drawn only by Horizon clients (hats, back items, shoulder pets, name tag styles), with no gameplay effect, client-side visibility toggles and the JSON item format.
- [Layers](layers.md): engine, platform and Horizon: what belongs to each, where a new feature goes, the dependency rule, every built-in outside address.
- [Forking](forking.md): the `horizon-extras` feature that holds Horizon's own extras (cosmetics, Friends, its settings pages...), and building the game without them.
- [Translations](translations.md): where Horizon's own text and its ~85 languages live, the unreviewed mark on machine-written strings, and how a native speaker reviews a language in #translations on Discord.
- [Narrator](narrator.md): vanilla's text-to-speech (Ctrl+B): the voices per platform, how screens record what they narrate, `horizon-ctl narration`, and where it still differs from vanilla.
- [Shader packs](shaders.md): Horizon's own WGSL shader packs, how to make and check one (`horizon-shader`).
- [Shader-pack clean room](cleanroom/README.md): how Java shader-pack support was rebuilt without another loader's code: the [format specification](cleanroom/shader-packs-spec.md) and the [provenance log](cleanroom/provenance-log.md) of every source read.
- [Mods](mods.md): content mods (data) and native mods (Rust), how to make, install and publish them. [Native mods](native-mods.md): how the launcher builds them. [Native hook points modelled on NeoForge](native-hooks-neoforge.md): the proposal for new hooks.
- [Upgrading a 26.2 data pack](datapack-upgrade.md): `horizon-port-pack upgrade-datapack` rewrites a 26.2 worldgen pack in 26.3's formats, and how the rewrite is checked against vanilla.
- [Modpacks](modpacks.md): porting whole packs (ATM10 and smaller): import, inventory, carry-over, an agent per mod with a harness against the Java original, coverage, and how "10x" is measured.
- [Launcher](../launcher/README.md): installs, updates and starts builds; its CLI and MCP server; releasing it.
- [Versioning](versioning.md): how releases are numbered from 0.2 (`v0.2.x`, `launcher@0.2.x`), and how installs from before 0.2 get there.
- [Resource packs](resource-packs.md): everything a 26.3 resource pack can change, whether Horizon honours it, and the proof.
- [Opt-in data collection](telemetry.md): the consent, exactly what is sent, how it is stored and sent.
- [Security](security.md): how Horizon protects code, CI and automatic releases against untrusted input while work is performed by AI agents.
- [Known issues](known-issues.md): player-facing issues listed in each game release and removed as pull requests fix them.
- [Benchmark report format](benchmark-report.md): the versioned JSON every benchmark writes (game, launcher, `tools/perf_bench.py`), the local log and, with consent, the upload.
- [Horizon APIs](api/README.md): every surface other programs build on (mods, shaders, agent control, servers, the launcher, data formats), each with its guide, generated reference, stability and changelog, and the [stability and versioning policy](api/stability.md).
- Menus: every screen is vanilla's (title, world list, create world, multiplayer, options, pause), except Horizon's graphics settings, drawn in the design system's Blocks palette. Horizon's own settings hang off vanilla's Options the way mods add theirs, each in one place: **Video Settings...** opens Horizon's graphics settings screen (every video option, the graphics extras and the shader packs), **Controls → Controller Settings...** the controller settings screen in the same style, **Mods...** and **Horizon Settings...** take the last two slots of the Options grid (Horizon Settings opens Extras, PvP & HUD, Splitscreen, Replay Studio and AI Agent, each a vanilla options list). The title screen's and pause menu's Friends buttons open Join a Friend and Play with Friends ([play-with-friends.md](play-with-friends.md)).

## Launcher and platforms

- [The launcher's mod browser](mod-browser.md): searching Modrinth and the Horizon registry for mods, modpacks, resource packs and shader packs, compatibility checks before install, and checksum verification.
- [Launcher commands](launcher-commands.md): every command of `horizon-launcher`, CLI arguments, and the tools `horizon-launcher mcp` offers for driving launcher actions.
- [Launcher resource preparation](launcher-preparation.md): compiling Java exporter classes into release bundles, verifying local Minecraft assets against `research-lock.json`, and multi-lane export workflows.
- [Microsoft account sign-in](microsoft-account.md): signing players in with their Microsoft account in the launcher, joining online-mode servers, the owner gate, and the OAuth device-code flow.
- [Windows allocators](windows-allocators.md): mimalloc defaults, the optional `windows-segment-heap` Cargo feature for Win32 heap APIs, and how to validate the executable manifest.
- [Windows ARM64](windows-arm64.md): building and running native Windows ARM64 executables for `aarch64-pc-windows-msvc`, cross-compilation, and release packaging.

## Parity

- [Rust rewrite status](rust-parity.md): where the state of the port lives (harnesses, port ledger, known issues) and the rules every port keeps.
- [Parity workflow](parity-workflow.md): `tools/parity.py`, the gate, other machines, merging.
- [CI runners](ci-runners.md): which jobs run on GitHub's runners and which on ours (the desktop, playhorizon, the Mac mini), the `SELF_HOSTED_CI` switch, and where a fork's pull request may run.
- Area notes: [block behaviours and ticking](parity/blocks.md), [section meshing](parity/meshing.md), [redstone machines tick by tick](parity/redstone-diff.md), [the same seed in vanilla and Horizon](parity/world-compare.md).
- [Port ledger](port-ledger.md): `tools/port_ledger.py`, every vanilla method next to the Rust functions that port it, both ways, with the gaps where a port isn't wired like vanilla.
- [Version ports](version-ports.md): how a new Minecraft release is detected, diffed against the pinned one and ported, and what stays manual.
- [Reading vanilla](parity-workflow.md#reading-vanilla): the local decompiled sources and bytecode queries the harnesses build on.

## Design notes

- [Design system](design-system.md): shared tokens and interface rules for the launcher, website and Horizon-only game overlays.
- [HUD kit](ui-kit.md): the pieces Horizon's own HUD overlays share: `Room`, which keeps the controller notice and splitscreen's join strip below the boss bars, `Plate`, the box behind their text, and controller glyphs asked for by what the button does, with the Nintendo letter rule.
- Splitscreen plumbing (#3541): the renderer hands its window surface to a compositor that places each view (`cargo run --profile fast -p horizon-client-render --example compositor [-- --smoke]`), and `Audio::share(owner)` gives each view its own sound engine on one output, playing a world sound once from the nearest camera (`cargo test -p horizon-client-render --lib audio::tests`).
- [Linear light for HDR and shader packs](design/hdr-linear-light.md): the proposed shader API 0.2 colour contract and frame format for HDR output.
- [Live mod reloading](design/mod-hotload.md): loading, enabling and updating mods from inside the game without a restart.
- [Mod registry v2](design/registry-v2.md): project kinds, metadata, installation locations and the registry rollout.

## Architecture: world generation

- [World generation](world-generation.md): the pipeline from noise to features. Each stage's vanilla comparison is its harness (`tools/test_<name>_parity.py`); `python3 tools/port_ledger.py where Class.method` finds the Rust port of a vanilla method.
- [World height](world-height.md): each level's shape from its dimension type, data packs' taller levels.
- [Pregeneration](pregeneration.md): `/pregen`, generating, lighting and saving an area ahead of the players.

## Architecture: world data and simulation

- [World data](world-data.md): chunk storage, palettes, region files.

## Java comparison profiling

- [JFR analysis for Java comparisons](profiling.md).
