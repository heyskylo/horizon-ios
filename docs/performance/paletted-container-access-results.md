# Paletted container access

The `unpack`/`get` cell reader and `BlockSection::get`/`set` are hot in the
chunk workers: light and mesh workers unpack a section's 4,096 cells before
propagating or meshing, and world-generation workers write one block at a time
through `BlockSection::set`.

The write path resolved the old cell and probed the palette three times; the
bulk readers dispatched the palette per cell; and the storage layer revalidated
indices and values a section had already checked. All three are addressed.

## Changes

`BlockSection::set` (`crates/horizon-world/src/chunk.rs`) previously resolved the
old cell through `get` and then probed the section's palette map three times for
one non-trivial write: once to key the old state by id (`local_ids[&old]`), once
to test the incoming state (`contains_key`), and once to find it (`get`). The
slot at the written index already *is* the old state's id, so the old state is
now read straight from storage, and the incoming state is resolved with a single
map probe. The palette-reuse, free-slot and growth paths are unchanged, so the
encoded palette, counts, free list and NBT output stay identical.

`BlockSection::get` and `set` validate the section index and the state id once,
then read and write through `PackedStorage::read`/`write`
(`crates/horizon-core/src/storage.rs`). These skip the storage-level bounds and
value checks and the `Result` that `PackedStorage::get`/`set` keep for external
callers: a section index is below 4,096 and a palette id always fits the width
`bits_for_palette` chose. Both new methods `debug_assert` their contract and are
used for the hot read, the hot write and the palette-growth re-encode.

`PalettedContainer::unpack` and `unpack_u16`
(`crates/horizon-core/src/paletted.rs`) matched on the palette variant inside the
per-cell loop. `Single`, `List` and `Global` are now dispatched once before the
loop, leaving each arm a branch-free indexed loop.

No allocation, layout or wire format changes.

## Reproduce

```sh
cargo test -p horizon-world --lib --profile fast benchmark_section_get_set -- --ignored --nocapture
cargo test -p horizon-core  --lib --profile fast benchmark_unpack            -- --ignored --nocapture
```

The ignored tests compare the new code against copies of the pre-optimization
implementations, kept in the test modules (`baseline_get`/`baseline_set`,
`baseline_unpack`). They check every resulting palette, count, free list, cell
and NBT before timing. Rounds alternate baseline/optimized order with `black_box`
and report the median and best nanosecond- or microsecond-per-operation; the best
round is the estimate of uninterrupted CPU time on a contended machine.

`benchmark_section_get_set` uses a 32-state registry and 50,000 random writes and
reads per round over a section whose palette grows through the 4- and 5-bit
widths (24 distinct states); 51 rounds. `benchmark_unpack` fills a 4,096-entry
list palette (12 states) and the global palette, 2,000 calls per round, 25
rounds.

AMD Ryzen 5 5600X, Linux x86_64, rustc 1.99.0, optimized `fast` profile,
nonincremental, no LTO:

| Operation | Original main | New | Speedup |
| --- | ---: | ---: | ---: |
| `BlockSection::set` | 13.15 ns/write | 9.12 ns/write | 1.44x |
| `BlockSection::get` | 2.33 ns/read | 1.45 ns/read | 1.61x |
| `unpack`, list palette | 1.910 us/call | 1.834 us/call | 1.04x |
| `unpack`, global palette | 2.270 us/call | 2.179 us/call | 1.04x |

The distinct-state differential test
`chunk::tests::set_matches_baseline_under_random_edits` drives the optimized
`set` and `get` and the originals over the same 50,000-edit streams for 1, 2, 16,
17, 24 and 64 distinct states, including periodic whole-section collapses that
clear the storage and reclaim palette slots, and asserts equal results, bits,
palette, counts, free list, `local_ids` and NBT throughout.
