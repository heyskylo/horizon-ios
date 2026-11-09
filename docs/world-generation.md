# World generation

`crates/horizon-world/src/worldgen/` ports vanilla 26.3's generator stage by stage, in vanilla's order: structure
starts and references (`structure/`), biomes (`biome_source/`, `climate/`), noise and density (`router.rs`,
`terrain.rs`, `aquifer/`, on `horizon_core::density` and `horizon_core::noise`), surface rules (`material_*`), carvers
(`carver/`), then decoration (`feature_graph.rs`, `decoration*.rs`, `placement/`) and structure pieces. The entry point
is `generator*.rs`; `crates/horizon-client/src/vanillagen.rs` binds it to the streaming workers.

Each stage has its own harness against vanilla (`tools/test_<stage>_parity.py`; `python3 tools/parity.py --list`
names them), and the whole-chunk harness below checks the stages together. `python3 tools/port_ledger.py where
Class.method` finds the Rust port of a vanilla method.

## Whole-chunk differential harness

`tools/test_chunk_generation_parity.py` generates chunks in actual vanilla classes (`VanillaChunkGenerationOracle`: structure starts and
references, biomes, noise, surface, carvers, then biome decoration and structure pieces through real `WorldGenRegion`s) and with
`crates/horizon-world/examples/probe/chunk_dump.rs`, and compares every block state, quart biome and block entity (canonical NBT) of each target
chunk, and the scheduled block and fluid ticks each proto chunk carries into the live world. Cases are terrain sites for several seeds
across the overworld, Nether and End, plus rectangles around every structure kind found by scanning the seed. `--phase structures`
isolates terrain plus structure pieces, `--phase terrain` stops before features. `HORIZON_ORDER` (a file of `x z` lines) makes both sides
decorate in that order instead of row-major.

### Data packs

A world's enabled data packs (`<world>/datapacks`, in `datapacks.json`'s order) change its generation as vanilla's registries do:
`worldgen::overrides` maps each pack file to the bundle it joins (noises, density functions, noise settings, material rules and
conditions, carvers, configured and placed features, structures, structure sets, template pools, processor lists, biomes, level stems), a
higher pack's file for an id replacing a lower one's, and its block, fluid and biome tag files to the tag bundles in pack order.
`GeneratorData::load_with` reads the bundles with them; the client caches the result per set of pack entries
(`vanillagen::shared_data_with`). What Horizon leaves out is logged: a pack's new biomes (the game has no id, colours or sounds for them;
their tag entries and climate points go with them) and folders the generator does not read. A pack's dimension types set each level's
height ([world-height.md](world-height.md)); noise settings taller than the level generate only inside it, as `clampToHeightAccessor`. Sapling growth and bone
meal still place vanilla's features.

`tools/test_datapack_worldgen_parity.py` runs the whole-chunk harness with `HORIZON_DATAPACKS=tools/fixtures/datapacks` on both sides
(the oracle selects every pack of the folder after vanilla through vanilla's own pack repository): its pack overrides a noise, a density
function, the overworld noise settings (sea level 70, no aquifers, a new final density with a new noise added), a placed feature and a
biome tag. Every site must match vanilla with the pack and differ from Horizon without it.

Packs must be written for 26.3: its worldgen formats differ from 26.2's (density functions take `left`/`right`, `gradient` replaces
`y_clamped_gradient`, noises take `base_octave`/`octave_count`, block states are strings, surface rules became the `material_rule` and
`material_condition` registries, configured features lost `config`). Vanilla 26.3 refuses a 26.2 worldgen pack as Horizon does.

### Decoration order

Biome decoration is order dependent: a chunk's features write into its eight neighbours, so a tree, ore blob, sculk patch or geode placed
first changes where the next chunk's features fit (and how many random draws they take), and a neighbour's geode can cut through a
stronghold room or leave it whole. Vanilla decorates on one `worldgen` executor in the order its chunk scheduler produces around a player:
loading tickets ring by ring in fastutil hash order, four in flight, FULL tasks decorating their 3x3 squares x-outer, z-inner by ticket
level. Two vanilla servers decorating around a player teleported to the same spot agree on all but about 6% of the overlapping chunk pairs
(measured from JFR `minecraft.ChunkGeneration` events) and a few hundred blocks.

`worldgen::decoration_order` replays that scheduler for a standing player and the game's streamer decorates in its order (a chunk waits
only for lower-ranked chunks within two of it, so distant chunks still decorate in parallel). Fed vanilla's recorded order, the oracle
reproduces a vanilla server's world exactly; the model itself swaps as many pairs as a second vanilla run does.

`tools/test_decorated_region_parity.py` decorates a 13x13-chunk area around each site in that order on both sides and compares everything
the whole-chunk harness compares: the two reported sites on seed 1891623539983467248 (an old-growth taiga at 204 -239, a stronghold with
sculk and a geode at 4870 401), eleven tree-heavy biomes (taigas, dark forest, birch, jungles, savanna, cherry grove, mangrove swamp, pale
garden) and the nearest mineshaft, village, ancient city and trial chambers of two seeds: 21 sites, 3,549 chunks, 0 differences.

Result on seed 12345 (overworld 42 sites / 372 chunks, Nether, End): structure pieces 0 differences on all 35 overworld structure sites; full
generation 4 differing chunks (3 blocks, 1 block entity) out of 372; Nether and End terrain sites 0 differences.

Harness normalisations (not generation differences): sections above the noise height (Nether roof, End sky) are all air in vanilla and
absent in Horizon; structure templates keep their stored NBT integer width; a vanilla block entity left as a `DUMMY` default is equal to no
Horizon entry; `LootTableSeed` without a `LootTable` and a vault's default reward table are ignored.

Remaining known differences: a handful of single blocks near structure pieces in full generation (abandoned camps, shipwreck, villages) that
need per-feature tracing, and tree features only through `tools/test_tree_feature_parity.py` (8,898 fixtures, all equal).
