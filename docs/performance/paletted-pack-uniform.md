# Uniform chunk sections when saving

`PalettedContainer::pack` (`crates/horizon-core/src/paletted.rs`) builds the
saved form of a chunk section: the distinct block states in index order, then
their indices re-packed at the saved bit width. It did that by unpacking all
4,096 cells into a scratch `Vec<u32>` (16 KiB, allocated once per section per
save) and handing the cells to `palette_ids`.

A section that holds one value — every air section above the terrain, every
stone, deepslate or water section, anything filled by one block — saves as one
palette entry and no indices (`bits: 0`, `data: None`). Learning that after
unpacking 4,096 cells was most of the cost of saving it, and most sections of a
column hold one value.

## Change

`pack` asks `uniform()` first, which answers from the stored words without
touching the cells: `Palette::Single` and a one-entry list answer directly, and
a list or global palette compares each stored word with the repeated pattern of
its first cell — 256 to 342 `u64` compares at the 4- and 5-bit widths a section
uses, and no allocation. A section that answers returns
`Packed { palette: vec![value], bits: 0, data: None }` at once; any other
section takes the old path unchanged.

The output is byte-identical. The old path fed `palette_ids` the cells of a
section holding one distinct value, so `ceil_log2(1) == 0`,
`Strategy::config(0) == Config::Single`, and the result was the same one-entry
palette with `bits: 0` and no data. A section whose palette kept an unused entry
(a state that was placed and overwritten) is uniform in its stored words too, and
packs to the same single entry. Non-uniform sections pay one extra scan that
stops at the first mismatching word, which measures as nothing.

`encode_with` (`crates/horizon-client/src/save.rs`) packs every section's blocks
and biomes, on the eager save path (up to 20 changed chunks per level tick) and
on the autosave and world-close batches.

## Reproduce

```sh
cargo test -p horizon-core --lib --profile fast benchmark_pack -- --ignored --nocapture
cargo build --profile fast -p horizon-client-lib --example save_batch_bench
target/fast/examples/save_batch_bench --nbt   # .exe on Windows
```

`benchmark_pack` compares `pack` with `baseline_pack`, the pre-change
implementation kept in the test module, over six containers: one value in a
`Palette::Single`; a list palette whose second entry was overwritten (a dead
slot); a global-palette section holding one value; a random 12-value section; a
terrain-shaped section; and a 64-cell biome section holding six values.
`pack_matches_unpacking_every_section` asserts both implementations agree before
any timing. Rounds alternate the two in 25 passes after a warm-up pass and report
the median microseconds per call.

`save_batch_bench --nbt` is the end-to-end check: 256 synthetic columns, NBT
encoded per chunk through the same `encode_with` a save uses, the `snapshot+nbt`
rows. Its columns are mostly uniform sections (air and stone), so it leans
towards the shortcut; the mixed and terrain rows of the microbenchmark are the
other end.

AMD Ryzen 7 8700F, Windows x86_64, rustc 1.99.1, optimized `fast` profile, three
runs each, median:

| `pack` case | Original | New | Speedup |
| --- | ---: | ---: | ---: |
| one value, `Palette::Single` | 3.85 us | 0.03 us | 116x |
| dead palette slot (list of 2, one used) | 5.40 us | 0.09 us | 58x |
| global palette holding one value | 5.67 us | 0.26 us | 22x |
| mixed section, 12 values | 18.13 us | 18.08 us | 1.00x |
| terrain section | 12.06 us | 12.13 us | 1.00x |
| biomes, 6 values | 0.37 us | 0.38 us | 0.96x |

Two of the six cases are unchanged within noise, and the 0.96x on the 64-cell
biome section is noise as well: a scan of four words against an unpack of 64
cells cannot pay for itself either way.

End to end, two runs each, median of the `snapshot+nbt` rows: the chunk
snapshot, its block entities' NBT, and the binary chunk NBT written in memory
(no disk I/O).

| Columns | Original | New | Speedup |
| --- | ---: | ---: | ---: |
| village, 1 | 635 us | 554 us | 1.15x |
| village, 16 | 11,756 us | 10,672 us | 1.10x |
| village, 64 | 50,373 us | 44,816 us | 1.12x |
| village, 256 | 205,375 us | 180,162 us | 1.14x |
| storage, 1 | 2,321 us | 2,232 us | 1.04x |
| storage, 16 | 44,705 us | 42,678 us | 1.05x |
| storage, 64 | 189,602 us | 175,655 us | 1.08x |
| storage, 256 | 733,277 us | 703,558 us | 1.04x |

The village fixture is 12 block entities per column, the storage one 48, whose
NBT is most of the remaining time. Per column at 256 chunks that is 802 to 704 us
for the village: about 100 us less per saved chunk, against 88 us predicted by
23 uniform block sections at 3.85 us each.
