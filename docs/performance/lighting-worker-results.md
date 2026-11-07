# Lighting worker result packaging

`light_chunk` now returns only `Vec<Light>`. `apply_lighting` moves each light
into the current section through `Arc::make_mut`, preserving current block and
biome edits and snapshots held by other workers. Streaming still rejects stale
sequences before applying results and replays edits after applying valid light.
All direct examples use the same application function.

## Reproduce

```sh
CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=/home/bob/Documents/horizon/target cargo test -p horizon-client --lib --profile fast benchmark_worker_light_results -- --ignored --nocapture
```

The ignored test invokes the production `pack_lighting` and `apply_lighting`
functions and compares them to original main's packaging/application loops.
It checks every resulting block, biome, and light value before timing. Each
measurement uses 500 columns per batch, one discarded warm-up batch, and ten
measured batches with alternating baseline/optimized order and `black_box`.
The reported median is the upper middle of the ten samples. Both implementations
receive identical precomputed propagation arrays. The fixture contains 24
sections with 16-state block palettes, four-biome palettes, and existing full
light arrays. A second case retains a fresh snapshot each iteration to force
copy-on-write during application. Fixtures are synthetic, not loaded saves.

AMD Ryzen 5 5600X, Linux x86_64, rustc 1.99.0, optimized `fast` profile,
nonincremental, no LTO:

| Packaging + application | Original main | New | Speedup |
| --- | ---: | ---: | ---: |
| Sections uniquely owned | 63.948 us/column | 44.397 us/column | 1.44x |
| Snapshot retained | 65.332 us/column | 53.025 us/column | 1.23x |

These measurements exclude propagation, region snapshot creation, outer chunk
copy-on-write, channels, replay, and remeshing. They establish savings in the
actual worker output/application paths, not a whole lighting runtime or FPS gain.

## Copy and allocation scope

`Section::heap_bytes` reports 149,760 bytes of fixture payload per column.
Original packaging clones that payload and allocates 24 section Arcs. Original
application clones the current section payload, allocates another 24 section
Arcs, and clones 98,304 bytes of returned light. New packaging clones no section
payload and allocates no section Arcs. New application moves the returned light;
with unique sections it clones no payload and allocates no section Arcs. With a
retained snapshot it still copies the current section payload and allocates 24
Arcs, as required to preserve the snapshot. New light arrays and the result Vec
still allocate. These are structural copy/allocation observations, not global
allocator instrumentation; byte figures exclude Arc headers, Vec headers, and
allocator overhead.

Focused tests cover moved light buffer identity, current block/biome edits,
retained snapshots, forced replay versus fresh full relighting, empty chunks,
missing neighbours, no-sky dimensions, light attenuation, occlusion, and
incremental/full equivalence. The stale-sequence guard and missing-current-chunk
handling in streaming are unchanged.
