# Horizon documentation

To play, use [the launcher](../launcher/README.md); the project site is [playhorizon.dev](https://playhorizon.dev). These pages are for developers. What is being done now and next, the work board and how to help: [the hub](https://playhorizon.dev/hub) ([ROADMAP.md](../ROADMAP.md), [roadmap/README.md](../roadmap/README.md) for keeping it current).

## Build and run

- [AGENTS.md](../AGENTS.md) and [Architecture](architecture.md): where each part of the code lives, how `Game` is organised, which harness checks which area.
- [How Horizon is built](how-horizon-is-built.md): who writes Horizon, how changes are checked, and what is safe to run.
- [Release flow](release-flow.md): pull request checks, merging and the automatic Nightly releases that reach players.
- [Resource preparation and filesystem work](prepare-performance.md): shared asset indexes, verified imports, staging and filesystem work that reduce preparation time.
- [Build from source](../README.md#build-from-source): your own copy of the game, resource preparation, `cargo run`.
- [Build times](build-times.md): `tools/build_times.py` measures cold and incremental builds per crate and the critical path, writes a JSON report and a table, and compares two reports; the first numbers, Windows and Linux.
- [Native client](native-client.md): options, controls, the dedicated server, `horizon-ctl` scripting, packaging, architecture.
- [Controllers](controllers.md): playing with PlayStation, Xbox and Nintendo Switch Pro controllers, Steam Deck built-in controls, analog movement, and the input layer.
- [Host a server](hosting.md): install `horizon-server`, prepare the game files, first run, `server.toml`, the commands, the web panel, backups, services and Docker; what works today and what doesn't yet.
- [A server's levels](server-levels.md): how the dedicated server and single player's server thread tick, save and travel between dimensions.
- [Velocity forwarding](velocity-forwarding.md): the player-information exchange behind a Velocity proxy and how it was checked.
- [The anticheat](anticheat.md): the dedicated server's own checks, off unless `server.toml`'s `[anticheat]` turns them on: violation levels, setback, alerts, kicks, the log, and how to write a check.
- [The chat filter](chat-filter.md): the dedicated server refuses hate slurs in chat, message commands, signs, anvil names and book titles: the list and its normalising, mutes, temporary bans, `[chat_filter]`.
- [Hosting a server: design](server.md): `horizon-server`, the design and the plan: install, `server.toml`, the CLI, the web panel, backups, services, Docker, and the bot benchmark against vanilla.
- [The next server: design](server-next.md): Horizon's dedicated server against vanilla and Pumpkin (measured), the targets, parallel ticking that keeps vanilla's order, the network front, the Horizon fast path, the plugin API and the plan.
- [Survival multiplayer](smp.md): the official server's `smp` native mod (economy, market, trades, games of chance, spawn, homes, teleport requests, warps, land claims) and the server hook points it needs; not part of Horizon's own server.
- [For server owners](owners.md): every `server.properties` key next to its `server.toml` field.
- [Agent control](agent-control.md): what AI agents can drive in the launcher and the game, and the design of the control socket, `horizon-ctl` and MCP.
- [horizon-ctl commands](horizon-ctl-commands.md): every command `horizon-ctl` takes, its CLI arguments, and the corresponding MCP tools for driving the game.
- [Graphics](graphics.md): the graphics settings screen (pages, search, descriptions), the graphics benchmark and what it measured.
- [Server against Pumpkin](performance/server-vs-pumpkin.md): Horizon's server and Pumpkin (Rust) under the same bots at 0, 20, 60 and 100 players: tick time, CPU, memory, chunks, join; where Horizon loses, and how to run it again.
- [Benchmark](benchmark.md): Horizon against vanilla 26.3 and vanilla with performance mods, on the same machine, world, path and settings: what is measured, how it is kept fair, how to reproduce it.
- [Extras](extras.md): optional zoom, minimap, FPS and keystroke overlays and auto-reconnect, all off by default.
- [PvP & HUD](pvp.md): optional client settings for PvP players (custom crosshair, block outline, hit color, HUD widgets), fair-play boundaries, and why each is off by default.
- [Cosmetics](cosmetics.md): wearable items drawn only by Horizon clients (hats, back items, shoulder pets, name tag styles), with no gameplay effect, client-side visibility toggles and the JSON item format.
- [Forking](forking.md): the `horizon-extras` feature that holds Horizon's own extras (cosmetics, Friends, its settings pages...), and building the game without them.
- [Narrator](narrator.md): vanilla's text-to-speech (Ctrl+B): the voices per platform, how screens record what they narrate, `horizon-ctl narration`, and where it still differs from vanilla.
- [Shader packs](shaders.md): Horizon's own WGSL shader packs, how to make and check one (`horizon-shader`).
- [Mods](mods.md): content mods (data) and native mods (Rust), how to make, install and publish them. [Native mods](native-mods.md): how the launcher builds them. [Native hook points modelled on NeoForge](native-hooks-neoforge.md): the proposal for new hooks.
- [Upgrading a 26.2 data pack](datapack-upgrade.md): `horizon-port-pack upgrade-datapack` rewrites a 26.2 worldgen pack in 26.3's formats, and how the rewrite is checked against vanilla.
- [Modpacks](modpacks.md): porting whole packs (ATM10 and smaller): import, inventory, carry-over, an agent per mod with a harness against the Java original, coverage, and how "10x" is measured.
- [Launcher](../launcher/README.md): installs, updates and starts builds; its CLI and MCP server; releasing it.
- [Versioning](versioning.md): how releases are numbered from 0.2 (`v0.2.x`, `launcher@0.2.x`), how installs from before 0.2 get there, and the steps for the day 0.2 ships.
- [Reusing pack overlays](reuse-pack-overlays.md): caching completed pack overlays, invalidating changed inputs and measuring reload costs.
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

- [Rust rewrite status](rust-parity.md): completion criteria, how to reproduce the validation, and per-area scope.
- [Parity workflow](parity-workflow.md): `tools/parity.py`, the gate, other machines, merging.
- [CI runners](ci-runners.md): which jobs run on GitHub's runners and which on ours (the desktop, playhorizon, the Mac mini), the `SELF_HOSTED_CI` switch, and where a fork's pull request may run.
- Area notes: [block behaviours and ticking](parity/blocks.md), [section meshing](parity/meshing.md), [redstone machines tick by tick](parity/redstone-diff.md), [the same seed in vanilla and Horizon](parity/world-compare.md).
- [Port ledger](port-ledger.md): `tools/port_ledger.py`, every vanilla method next to the Rust functions that port it, both ways, with the gaps where a port isn't wired like vanilla.
- [Version ports](version-ports.md): how a new Minecraft release is detected, diffed against the pinned one and ported, and what stays manual.
- [Decompilation and research lab](research-lab.md) and [decompilation status](decompilation-status.md): the local game inspection workspace the harnesses build on.

## Design notes

- [Design system](design-system.md): shared tokens and interface rules for the launcher, website and Horizon-only game overlays.
- [HUD kit](ui-kit.md): the pieces Horizon's own HUD overlays share: `Room`, which keeps the controller notice and splitscreen's join strip below the boss bars, `Plate`, the box behind their text, and controller glyphs asked for by what the button does, with the Nintendo letter rule.
- [Offscreen frame compositor](design/compositor.md): sharing a window surface between views, clipping and the standalone splitscreen compositor example.
- [Linear light for HDR and shader packs](design/hdr-linear-light.md): the proposed shader API 0.2 colour contract and frame format for HDR output.
- [Live mod reloading](design/mod-hotload.md): loading, enabling and updating mods from inside the game without a restart.
- [Mod registry v2](design/registry-v2.md): project kinds, metadata, installation locations and the registry rollout.
- [Shared audio output](design/shared-audio.md): sound engines on one output, listener ownership and duplicate world-sound handling.

## Architecture: world generation

- [World generation](world-generation.md): the pipeline from noise to features.
- [World height](world-height.md): each level's shape from its dimension type, data packs' taller levels.
- [Pregeneration](pregeneration.md): `/pregen`, generating, lighting and saving an area ahead of the players.
- [Noise generation](noise-generation.md), [compiled density functions](density-functions.md), [structure density](structure-density.md), [terrain filling](terrain-filling.md).
- Biomes: [lookup](biome-lookup.md), [temperatures](biome-temperatures.md).
- Surfaces: [material surfaces](material-surfaces.md), [material conditions](material-conditions.md), [rule libraries](material-rule-libraries.md), [vertical anchors](vertical-anchors.md), [value providers](value-providers.md).
- [Carving](carving.md), [heightmaps](heightmaps.md), [feature placement](feature-placement.md).

## Architecture: world data and simulation

- [World data](world-data.md): chunk storage, palettes, region files.
- [Block entity save batches](save-batch-entities.md): grouping block entities across save batches while preserving chunk data and save ordering.
- [Block-state codecs](block-state-codecs.md), [scheduled ticks](scheduled-ticks.md).
- [Collision geometry](collision-geometry.md): shapes, collision, movement and fluids.
- [Face visibility](face-visibility.md): which model faces the mesher emits.

## Java comparison profiling

- [JFR analysis for Java comparisons](profiling.md).

## Benchmarks and results

- [Animated sprite mip cache](animated-sprite-mips-benchmark.md): caching full mip chains for displayed source frames cut repeated tick cost by 53.34% (3.627 to 1.692 ms) and repeated render by 53.33%, within an 8 MiB budget.
- [Generation scan gate benchmark](generation-scan-benchmark.md): reusing an exhausted generation scan made the stationary case 1,528.99x faster (15,441.3 to 10.1 ns/frame) and a dynamic remove/load cycle 14.72x faster.
- [Bulk density and noise generation benchmark](noise-aquifer-benchmark.md): one-time dispatch, kernel reuse and X-column reuse cut Overworld terrain time by 20.62% with one worker (12.685 to 10.069 ms/input chunk) and 16.96% with four.
- [Nether-portal section-scan benchmark](portal-scan-benchmark.md): a resident portal-section cache made the steady dense scan 11.19x faster (244.610 to 21.869 us/tick) and dense_portal 12.19x faster, while rescans after every-chunk edits were slower.
- [Random-tick eligibility benchmark](random-tick-eligibility-benchmark.md): caching section eligibility made the steady dense selection 298.37x faster (1,311.924 to 4.397 us/tick) and 1% edits 141.74x faster.
- [Shared model library benchmark](shared-model-library-benchmark.md): sharing one loaded model library cut combined asset and item preparation by 18.50 ms (3.47%), with item preparation down from 83.12 to 36.30 ms, for 1,578 KiB more peak RSS.
- [Constant surface-rule specialization benchmark](surface-generation-benchmark.md): specializing constant surface rules improved primary Overworld terrain by 3.43% with one worker (10.664 to 10.298 ms/input chunk).
- [Section visibility shortcuts](visibility-benchmark.md): the shortcut helper was 3.05x to 3.44x faster on random 1-50% opaque sections (27,657.9 to 9,061.1 ns at 1%) and no case regressed.
- [Entity culling benchmark](media/entity-culling-benchmark.md): asynchronous entity culling raised average FPS by 19.2% (83.59 to 99.67) and synchronous culling by 15.9% in a 300-armor-stand wall scene.
- [Lighting worker result packaging](performance/lighting-worker-results.md): moving light results instead of cloning them cut packaging and application from 63.948 to 44.397 us/column (1.44x) when sections are uniquely owned and from 65.332 to 53.025 us/column (1.23x) with a retained snapshot.
- [Paletted container access](performance/paletted-container-access-results.md): one-pass section validation and per-palette unpack dispatch made `BlockSection::set` 1.44x faster (13.15 to 9.12 ns/write), `get` 1.61x faster (2.33 to 1.45 ns/read) and bulk `unpack` 1.04x faster.
- [Progressive world resources](performance/startup-stage-3.md): stage 3 makes the title screens interactive before core resources finish; an unblocked launch reached core at 4,776 ms, renderer at 5,260 ms and rest at 6,282 ms, with progressive-title RSS about 11% higher in a warm run.
