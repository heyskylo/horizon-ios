# Decoding an item's components in one pass

`decode_components` (`crates/horizon-items/src/item_data.rs`) turns a stack's
saved `components` compound into the `StackData` the game reads. Loading a chunk
calls it for every slot of every block entity — a chest is 27 stacks — and it
runs again for player inventories, bundles and shulker boxes.

It read the 23 component IDs it models one at a time with `Tag::get(&str)`.
Each call encodes the ID to UTF-16 in a thread-local buffer and walks the
compound's `BTreeMap`, whether or not the stack carries that component. Most
stacks carry one or two, so most of those lookups were pure overhead: an empty
compound cost 0.56 us, and the one-component stack the save batch uses cost
0.93 us, of which the lookups are about 0.57 us.

## Change

`Seen::read` walks the compound once and dispatches each key against the 23
IDs, copying the key's units into a 64-byte stack buffer when they are ASCII. A
key with any other unit cannot be a component ID, so it falls through to `other`
exactly as before, and no key is ever decoded twice.

The fields then decode from those `Option<&Tag>`s with the expressions they
used before; the second pass that sorts unmodelled components into `other` is
untouched. No branch of the decode changed, nothing was added to the stored
form, and the pass allocates nothing.

## Reproduce

```sh
cargo test -p horizon-items --lib --profile fast benchmark_decode_components -- --ignored --nocapture
cargo build --profile fast -p horizon-client-lib --example save_batch_bench
target/fast/examples/save_batch_bench --split --density 12   # .exe on Windows
target/fast/examples/save_batch_bench --split --density 48
```

`benchmark_decode_components` compares `decode_components` with
`baseline_decode_components`, the per-ID lookups kept in the test module, over
five compounds: one that is not a compound, an empty one, one kept ID
(`minecraft:custom_data`), every modelled ID at once, and two keys the model
keeps in `other`, one of them non-ASCII. `one_pass_decode_matches_the_per_id_lookups`
asserts both implementations agree on all five before any timing. Rounds
alternate the two in 25 passes after a warm-up pass and report the median
microseconds per call.

`--split` times the four stages of a 256-column save and load batch apart: the
snapshot and chunk NBT built (`encode`), the binary NBT written (`write`), read
back (`read`) and decoded (`decode`). `--density` picks the block entities per
column, 12 (village) or 48 (storage); each batch clones the same synthetic
stack, so the item decode path is what the `decode` figure moves.

AMD Ryzen 7 8700F, Windows x86_64, rustc 1.99.1, optimized `fast` profile,
median of 25 rounds; a repeat run differs by a few percent:

| `decode_components` case | Original | New | Speedup |
| --- | ---: | ---: | ---: |
| empty compound | 0.562 us | 0.089 us | 6.3x |
| one kept ID (`minecraft:custom_data`) | 0.928 us | 0.356 us | 2.6x |
| unknown and non-ASCII IDs | 1.394 us | 0.710 us | 2.0x |
| every modelled ID | 9.181 us | 8.601 us | 1.07x |
| not a compound | 0.090 us | 0.086 us | 1.05x |

The common case is the second row: a stack with one component drops to 38% of
its decode cost. With every modelled ID present the decoding of the fields
themselves dominates, so removing the lookups only takes 7% off that shape.

End to end, 256 columns, best of 15 rounds, medians of six runs each way:

| Fixture | Original `decode` | New `decode` | Speedup |
| --- | ---: | ---: | ---: |
| village, 12 block entities/column | 106 ms | 81 ms | 1.31x |
| storage, 48 block entities/column | 450 ms | 330 ms | 1.35x |

The village batch holds 165 stacks per column (six 27-slot chests and three
furnaces), 42,240 stacks in all; at 0.57 us saved per one-component stack that
predicts 24 ms, and the decode stage drops 25 ms. The storage batch decodes
about 169,000 stacks and swings by roughly 10% between runs. `encode`, `write`
and `read` are unchanged within noise, so the whole village batch (snapshot, NBT
encode, write, read, decode) goes from 320 to 299 ms (1.07x).
