# Horizon world data

`crates/horizon-world` holds the storage layer for the pinned Java Edition **26.3**, data version **5023**: NBT,
region files, saved palettes, resident block sections, saved tick lists and the data fixers that upgrade older
worlds. It also has a small offline tool for inspecting and copying region files.

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
block-state JSON codec reproduces vanilla property
fallback for compact and full JSON forms.

`ChunkData::saved_ticks` reads `block_ticks` or `fluid_ticks`, resolves type
identities through a caller-supplied registry lookup, and filters positions
outside the chunk as vanilla does. `set_saved_ticks` writes a queue's packed
relative delays while preserving other root fields. IDs here identify block or
fluid types, not block states. Missing tick lists are empty. Numeric fields use
vanilla's integer narrowing and floating-point conversion. Unknown types and
malformed entries return errors; vanilla's logging/partial-result paths are not
reproduced. Scheduler integration and its separate suite.

## Validation

`tools/test_world_parity.py` (NBT, palettes, regions, block-state registry, resident sections), `test_tick_parity.py`
(saved tick lists), `test_fluid_parity.py` (the fluid each block state carries) and `test_datafix_parity.py` (upgrading
worlds written by older server jars) compare Rust with vanilla's own codecs on both reference jars.
