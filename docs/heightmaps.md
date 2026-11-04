# Heightmaps

Horizon implements the six heightmap types in the pinned Minecraft Java Edition
**26.3**. Each map stores 256 relative column heights in padded 64-bit words.
World-surface predicates use `isAir`; ocean-floor predicates use the version's
`blocks_motion_in_heightmap` block tag. Motion-blocking maps also include nonempty
fluid states, with the separate no-leaves tag for that variant.

`HeightmapState` supplies block behavior and tag membership. `HeightmapChunk`
supplies the vertical extent, highest occupied section and block reads. The map
owns its packed data and checks that a supplied chunk has the same bounds. Its
current constructor accepts positive, aligned ranges within the vanilla noise
settings codec domain; zero-height and arbitrary runtime accessors are outside
this bounded API.

Updates retain the early return for blocks below the top two positions. Removing
a qualifying surface scans downward until another qualifying block is found.
Priming scans X, then Z, then descending Y, sharing each block read across the
selected maps. It skips exact AIR blocks and stops a column when all selected
maps have found a surface. If a column finds nothing, its existing value remains.
This also applies to re-priming after a raw-data length mismatch.

Raw imports preserve every bit when the word count matches, including padding,
unused slots and values above the build height. Column addressing retains Java's
wrapped `x + z * 16` calculation; invalid final indices return typed errors. The
Rust API does not expose a mutable alias to its internal words.

Run the differential suite with:

```sh
python3 tools/test_heightmap_parity.py
```

The [local metadata report](../benchmarks/rust-heightmap-2026-10-06/scoped-parity.json)
records **1,671,668 matching output lines per reference JAR**, checked separately
against the authenticated client and server. It covers all **35,723 block states**,
all six predicate and persistence/client flags, and **100 chunk fixtures** with
86,806 state writes, 54,306 column queries, 516 priming calls, 378 raw-data loads
and additional update-only error cases. Selected fixtures compare exact ordered
block reads. Cases include section and vertical bounds, leaves, fluid-bearing
states, air variants, removal rescans, empty-column retention, packed padding,
invalid raw lengths and wrapped column indices.

Air/fluid facts and state identities are exported independently from each JAR and
required to agree. These are private test inputs, not ports of block behavioral
methods. Rust resolves heightmap tag membership using its tag loader and the
300 bundled block-tag resources. The reference binds those resources to the
actual vanilla block registry and invokes `Heightmap` on `ProtoChunk` instances.
Only in-memory section storage is used; placeholder biome holders do not exercise
biome properties or section codecs.

Source and fixture changes during a run invalidate its report. Raw resources,
state exports and comparison traces stay under ignored `.local`; the checked-in
report contains metadata and hashes. CI runs this suite on its configured
platforms; the local result does not establish those remote results. Chunk
lifecycle integration, generated terrain, lighting and playable gameplay remain
pending.

## Related docs

- [World generation](world-generation.md)
- [Terrain filling](terrain-filling.md)
- [Collision geometry](collision-geometry.md)
