# Noise-stage terrain filling

Horizon now connects final-density sampling, aquifer selection, resident block
sections, world-generation heightmaps and fluid postprocessing for the pinned
Minecraft Java Edition **26.3**. This implements the initial noise fill, before
the [surface stage](material-surfaces.md#chunk-surface-stage), features and
structure placement.

`TerrainChunk` owns registry-backed `BlockSection` palettes, non-air counts,
optional heightmaps and per-section postprocessing offsets. Raw section reads
retain stored cave/void air; chunk reads collapse an all-air section to AIR, as
vanilla `ProtoChunk` does. Block air/fluid behavior and tag membership come from a
`TerrainBlockStates` provider tied to the same immutable registry.

`fill_noise` samples the entire final-density volume before writing blocks. It
then visits increasing Z, increasing X and decreasing Y. Aquifer results use the
default solid block when absent. Exact default AIR results skip writes, preserving
any prior block at that position. Other states are written before updating
`OCEAN_FLOOR_WG`, then `WORLD_SURFACE_WG`. A scheduled aquifer result with nonempty
fluid appends its packed offset; repeated offsets are retained in insertion order.

The aquifer and final-density sampler share a context. The caller constructs the
aquifer first, then compiles final density through the same compiler. The Rust
fill API accepts a full 16-by-16 chunk footprint with unit steps and a positive
vertical subrange inside its bounded chunk. Typed failures retain earlier writes
and sampler/aquifer state. Developer diagnostic surface markers are outside this
regular-gameplay path.

Run the differential comparison with:

```sh
python3 tools/test_terrain_parity.py
```

The [local report](../benchmarks/rust-terrain-2026-10-06/scoped-parity.json) records
**7,950,873 matching output lines per reference JAR**, checked against both client
and server. **114 fixtures** perform **200 fills**; **61 fixtures** enable noise-based
aquifers. Cases include all seven bundled routers, their aquifer configurations,
both random algorithms, multiple seeds and positive/negative chunk coordinates,
synthetic density roots, nondefault fluid-bearing states, prefilled chunks,
partial-height fills and repeated fills using the same caches. An additional
24 fixtures bind rigid-piece and junction density through the actual noise chunk,
including synthetic final-density and aquifer roots that consume that field.

After every fill, the suite compares every raw section state, both packed
heightmaps, the final fluid-scheduling flag and every ordered postprocessing
offset. The reference invokes actual vanilla `NoiseChunk` and
`NoiseBasedChunkGenerator.doFill`, with empty blending and empty or supplied structure-density fields.
Rust resolves bundled tags and compiles density and aquifer roots itself.
Block identities/properties and air/fluid facts are private exports, generated
independently by both authenticated JARs and required to agree. Importing these
facts does not implement the underlying block behaviors. Placeholder material
and biome holders are never evaluated in this fill-only comparison.

Reports capture source and fixture hashes, and the runner rejects changes during
validation. CI includes the comparison on its configured platforms; the local
report does not establish remote results. Complete generated terrain still needs
biome assignment, structures, features, nonempty
blending and live structure discovery, lighting and chunk lifecycle integration. These
results do not establish a playable client or whole-game performance improvements.

The [combined surface-stage suite](material-surfaces.md#chunk-surface-stage) now
checks noise filling followed by surface application through shared density
contexts for all seven bundled terrain settings.


## Block writes across generation stages

`PreLightChunkStage` represents the six statuses from empty through features.
`TerrainChunk::set_block_worldgen` maintains the two world-generation maps through
biomes, then the four final maps at terrain/features. Assigning a stage alone
neither clears nor primes maps. Writes update existing selected maps first and
then prime missing selected maps; maps belonging to earlier stages retain their
stored values. This also preserves native behavior when an earlier stage is
assigned again.

Run `python3 tools/test_chunk_stage_parity.py`. The
[stage report](../benchmarks/rust-chunk-stage-2026-10-06/scoped-parity.json) records
**8,090,090 matching lines per reference JAR** across 32 fixtures. Every pinned
block state is inserted and removed under both map groups. The comparison uses
actual `ProtoChunk.setBlockState` and persisted-status assignment, checking returned
old states, chunk reads, map presence/heights, packed map words and raw sections.
It also covers forward/backward transitions, explicit priming, empty-section AIR
writes, out-of-height writes and wrapping X/Z coordinates.

This implements stage-dependent storage mutation. Scheduling, saved status
metadata, retrogen, lighting, block entities and complete region behavior remain
pending.

## Related docs

- [World generation](world-generation.md)
- [Heightmaps](heightmaps.md)
- [Material surfaces](material-surfaces.md)
