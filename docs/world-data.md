# Horizon world data

`crates/horizon-world` implements part of the Rust rewrite's storage layer for
the repository-pinned Java Edition **26.3**, data version **5023**. It reads and
exports region files and provides block-section access. Lighting, world generation,
networking and rendering remain unimplemented. Scheduled block/fluid queues now
integrate with saved tick lists, but their gameplay callbacks remain unimplemented.

## Commands

Build the standalone tools with `cargo build --release -p horizon-world`.
The binary is `target/release/horizon-world` (`.exe` on Windows).

```sh
target/release/horizon-world inspect /path/to/world/region/r.0.0.mca
target/release/horizon-world get-block /path/to/world/region/r.0.0.mca 0 64 0
target/release/horizon-world copy-region /path/to/world/region/r.0.0.mca /path/to/export/r.0.0.mca
target/release/horizon-world copy-region /path/to/world/region/r.0.0.mca /path/to/lz4-export/r.0.0.mca lz4
target/release/horizon-world edit-block-data /path/to/world/region/r.0.0.mca /path/to/edited/r.0.0.mca 0 64 0 minecraft:oak_log axis=y
target/release/horizon-world nbt-copy /path/to/level.dat /path/to/level-uncompressed.nbt gzip none
target/release/horizon-world --help
```

Use offline input and a separate export directory. Output filenames must retain
the source region coordinates. Existing destination regions and external chunk
files are refused. An unchanged export preserves chunk payloads and timestamps;
an explicitly recompressed export preserves decoded NBT values, subject to
vanilla's canonicalization. Region allocation layout may change.

`get-block` reports the stored descriptor; string descriptors can omit default
properties. `edit-block-data` changes an existing saved section's block data and
preserves other chunk fields. It does **not** perform a gameplay placement:
heightmaps, light, block entities, ticks and neighbor updates are not updated.
The raw command accepts descriptors without validating them against a loaded
registry. The library's `BlockRegistry::resolve` validates properties and
supplies defaults, and `BlockSection` uses that registry for resident storage.

## Implementation

NBT handles all tag types, named and unnamed network roots, Java modified UTF-8
including unpaired UTF-16 surrogates, and 26.3 heterogeneous list wrappers.
Compound key order is not preserved; duplicate keys resolve to their last
value. Java's negative-zero normalization on loading and NaN canonicalization
on writing are reproduced. Defaults bound encoded/decoded NBT to 64 MiB,
one million wire nodes and 512 container levels. These are application limits,
not a reproduction of Java's allocation accounting or exception behavior.

Region codecs are gzip, zlib, uncompressed and **lz4-java block framing**.
The latter includes the pinned library's 28-bit xxHash checksum adapter.
Region headers, chunk lengths, sector overlap and external `.mcc` chunks are
validated. Chunk coordinates use floor division at negative boundaries.

Exports build a temporary region, sync file content, then publish using a
hard link that fails if the destination exists. Failed writers cannot publish;
normal error cleanup removes their temporary and newly created external files.
The filesystem must support hard links. This is an offline export mechanism:
concurrent source modification, crash recovery, directory fsync and in-place
save transactions are not implemented.

Saved palettes support uniform zero-bit sections and padded packed words for
4,096 block entries or 64 biome entries. Resident `BlockSection` stores numeric
state IDs through a shared, borrowed registry. Uniform sections have no word
array. Changes reuse unused palette slots; replacing a unique value can update
its slot without repacking 4,096 entries. Width changes repack only when needed.
Explicit compaction removes unused slots and reduces the packed width.

The registry loader checks data version, duplicate states, default IDs and
block-name coverage. The local oracle exports all 35,723 state identities and
defaults from vanilla. This imports identities, not block implementations,
shapes or behavior. Registry exports and game-derived fixtures stay in `.local/`.
Chunk access requires data version 5023. A world saved by an older version is upgraded when it is opened, as 26.3 does: the file fixes (`FileFixerUpper`) and level.dat, then vanilla's DataFixers over every chunk, entity chunk, POI chunk, player, saved data, advancements and statistics file (`horizon_world::datafix`; vanilla upgrades each file as it reads it, Horizon all of them on opening). `tools/test_datafix_parity.py` compares the result with vanilla's own upgrade over worlds written by older server jars. Every server data fix is ported, from the saves of 1.8 (no DataVersion) on; only the fixes of client files (options, debug profiles) are not. `BlockRegistry::resolve` rejects invalid property descriptors. The separate
[block-state JSON codec](block-state-codecs.md) reproduces vanilla property
fallback for compact and full JSON forms.

`ChunkData::saved_ticks` reads `block_ticks` or `fluid_ticks`, resolves type
identities through a caller-supplied registry lookup, and filters positions
outside the chunk as vanilla does. `set_saved_ticks` writes a queue's packed
relative delays while preserving other root fields. IDs here identify block or
fluid types, not block states. Missing tick lists are empty. Numeric fields use
vanilla's integer narrowing and floating-point conversion. Unknown types and
malformed entries return errors; vanilla's logging/partial-result paths are not
reproduced. [Scheduler integration and its separate suite](scheduled-ticks.md).

## Validation

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
python3 tools/test_rust_parity.py --prepare
python3 tools/test_world_parity.py
python3 tools/test_tick_parity.py
```

The world runner verifies the pinned artifacts, generates fixtures by invoking
actual vanilla classes, processes them with Rust, and reads the results with
both official client and server code. Each reference checks:

- 32 NBT format/codec conversions covering every tag type, UTF-16 edge cases,
  mixed lists, floating-point canonicalization and oversized byte arrays.
- 44 edited/compacted palette cases, comparing 123,776 individual values across
  uniform, local and global vanilla palette transitions.
- 48 region chunk reads covering four codecs, copying, recompression, a stored
  block edit, negative positions and oversized external chunks.
- All 35,723 exported block-state IDs and their serialized registry data.
- 8,192 resident values: a section with 4,096 distinct states, followed by the
  result of 100,000 mixed block replacements checked against an independent
  array of expected IDs. Rust additionally checks each returned old value,
  periodic compaction, every registry ID through one slot, and uniform collapse.

Offline tests cover malformed lengths, truncation, NBT limits, corrupt checksums,
invalid palette indices, sector corruption, the inline/external size boundary,
unknown-field preservation, failed-writer cleanup and destination collisions.
CI runs both differential suites. Run artifacts and logs stay under
`.local/rust-world-parity/`; a stale top-level success report is removed before
each run. The checked-in [report](../benchmarks/rust-world-2026-10-05/scoped-parity.json)
contains metadata and hashes only.

These are synthetic storage fixtures, not a generated world's full save/load
lifecycle or playable integration test. They establish the reported tested
scope; they do not establish complete vanilla parity or any FPS improvement.


## Contained fluid states

`FluidStateTable` derives the fluid carried by each pinned block state: empty,
water or lava, source/flowing identity, amount and falling status. Waterlogged
copper grates carry falling source water. `FluidBlockBindings` binds constant-time
conversion back to liquid block identities, including the collapse of liquid
levels 8–15 to their shared falling-fluid block representation.

`FluidHeightWorld` supplies above-fluid and downward-face support queries.
Ordinary height returns a full block when matching fluid is above; camera height
first checks downward support above a source. Empty fluids skip world queries.
Coordinates wrap as Java integers, and query failures preserve evaluation order.
The adapter owns block-support geometry; this interface does not implement every
block's support shape.

Run `python3 tools/test_fluid_parity.py`. The
[fluid report](../benchmarks/rust-fluid-2026-10-06/scoped-parity.json) records
**243,331 matching lines per JAR**. It compares all 35,723 block states, five fluid
identities, every fluid-family pair, own-height bits, source/falling/full flags and
legacy block conversion. An additional 2,432 fluid-state JSON fixtures compare
all 37 fluid states, native defaults, malformed properties and encoding. The 40,344 world fixtures request 80,688 surface/camera
height queries, including coordinate overflow and injected failures. The support
fixtures use actual vanilla stone and air states. Fluid spreading, scheduled
updates, entity interactions and rendering remain pending. Workspace tests
and all-target Clippy pass.
