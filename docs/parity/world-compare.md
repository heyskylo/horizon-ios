# The same seed in vanilla and in Horizon

`tools/world_compare.py` compares two saved worlds of one seed, the one vanilla 26.3 wrote and the one Horizon
wrote, chunk by chunk: the seed and spawn, the biome of every cell, every structure start, and the blocks. It is
for the question "does this seed give the same world", the whole game answering it, where the generator harnesses
(`tools/test_chunk_generation_parity.py`, `test_biome_generation_parity.py`, `test_decoration_region_parity.py` and
the others) answer it a stage at a time against the pinned bytecode.

## Making the two worlds

```sh
python3 tools/perf_bench.py --game vanilla --seed 12345 --distances 8 --scenes village --repeats 1 --seconds 3 --sizes 1280x720 --label seed-vanilla
cp -r .local/perf-bench/vanilla/saves/PerfBench /tmp/world-vanilla     # the next run deletes it
python3 tools/perf_bench.py --game horizon --build target/release --seed 12345 --distances 8 --scenes village --repeats 1 --seconds 3 --sizes 1280x720 --label seed-horizon
cp -r .local/perf-bench/horizon/saves/PerfBench /tmp/world-horizon
python3 tools/world_compare.py /tmp/world-vanilla /tmp/world-horizon
```

Each game creates the world itself (vanilla through `WorldOpenFlows.createFreshLevel`, Horizon through `open`),
generates the chunks around the flight and writes them when it quits, with no EULA to accept and no server to run.
`tools/vanilla_world_check.py` makes vanilla's world from the dedicated server instead.

## What must match, and what can't

The two games write chunks at different stages and in different orders: one that is `full` in both has had its
features placed in whatever order its neighbours finished. Terrain doesn't depend on that, decoration does where it
spills across a chunk border, and two runs of vanilla itself differ there. So the tool reports three things apart:

- **Must be identical:** the spawn, the biome of every cell (chunks that have no biomes yet are left out), every
  structure start (type and position).
- **Should be near 100%:** the terrain blocks (stone, deepslate, water, lava, the grass on top, gravel, sand, tuff).
- **Follows decoration order:** trees, sculk, moss, grass. The floor for these is what vanilla gives against itself:
  `--noise WORLD_C`, a second vanilla world of the same seed, prints it beside the comparison.

## Results

Horizon `main` of 2026-11-08 (a `fast` build with a render-only change on top, which doesn't touch generation) against
vanilla 26.3, render distance 8 around the benchmark's village flight, on Windows. Share of
the positions holding a block in either world that hold it in both, over the chunks both saved as `full`:

| | seed 12345, A vs B | seed 12345, vanilla vs vanilla | seed -4172144997902289642, A vs B |
| --- | ---: | ---: | ---: |
| spawn | identical (96, 145, -32) | identical | identical (-448, 71, -432) |
| biomes | 1457 of 1457 chunks identical | | 1525 of 1525 identical |
| structure starts | 13 of 13 | | 21 of 21 |
| full chunks compared | 992 | 972 | 1071 |
| stone | 99.7% | 99.7% | 99.9% |
| deepslate | 99.5% | 99.5% | 99.9% |
| water | 99.3% | 99.4% | 99.6% |
| lava | 99.4% | 99.9% | 99.7% |
| grass block | 98.9% | 99.1% | 99.9% |
| gravel | 99.6% | 99.7% | 99.8% |
| tuff / granite / diorite | 99.5 / 98.9 / 99.2% | 99.4 / 99.1 / 99.2% | 99.8 / 99.2 / 99.2% |
| coal / iron / copper ore | 89.0 / 99.6 / 99.7% | 90.7 / 99.7 / 99.8% | 98.6 / 99.8 / 99.8% |
| oak leaves / oak logs | 56.0 / 52.7% | 62.7 / 59.7% | 91.1 / 89.3% |
| dark oak leaves | 52.5% | 69.0% | none in the chunks |
| sculk / moss / short grass | 68.8 / 71.2 / 72.2% | 68.0 / 69.4 / 72.4% | none / 75.6 / 69.3% |

Terrain, biomes, structures and spawn are the same for both seeds, a large negative one included, and ores and
sculk, moss and grass are as close to vanilla as vanilla is to itself. **Trees are the one thing that sits below
that floor**: oak 56% against 63%, dark oak 52% against 69% (dark oak is rare, so its sample is small). Where the
overlap of two vanilla runs is 60 to 70%, a gap of a few points may be the order Horizon decorates in
(`DecorationOrder`) and not a different tree: that is the next thing to look at with trees alone, on a flat run of
chunks generated in one fixed order by both.

## What the tool can't tell

- It reads what the games saved. A chunk one game saved at an earlier stage than the other is not compared (only
  `full` in both is), and a block in a chunk's border that a neighbour's decoration reached later may differ for
  that reason alone.
- Heightmaps are not compared: a saved chunk keeps them only for some stages.
- Entities, block entity contents and fluid and block ticks are not compared.
