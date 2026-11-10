# Memory

Where the game's memory goes, and the tools that say so. This page describes what exists today; each new tool adds its section when it lands.

Start with `horizon-ctl memory` for what holds memory now, count allocations to see how much a frame churns, add the heap census to see who allocated what is held, and fall back to an external heap profiler only on small probes.

## What the game holds now: `horizon-ctl memory`

```sh
horizon-ctl memory            # one JSON object, from the running game
horizon-ctl memory > a.json   # keep it to compare with a later one
```

The reply is built in `crates/horizon-client/src/game/memory.rs`. Sizes are MiB with one decimal and end in `_mb`.

| Key | What it counts |
| --- | --- |
| `process.resident_mb` | The resident set: `/proc/self/statm` on Linux, the working set on Windows, `proc_pidinfo` on macOS |
| `process.allocator_commit_mb`, `allocator_peak_commit_mb` | Bytes mimalloc has committed now and at its peak (`mi_process_info`) |
| `render_distance` | The window's render distance in chunks |
| `allocator` | `commit_mb`, `peak_commit_mb`, and where mimalloc's statistics are available `live_heap_mb` (the pages mimalloc has in use, each counted whole) and `overhead_mb` (committed less that) |
| `client_columns` | The window's loaded columns (absent on a dedicated server, which has no window and names its columns `columns`): `total_mb`, counts of `columns`, `sections`, `unique_sections` (each shared section once), `uniform_block_sections`, `light_arrays`, and the heap bytes by part (`blocks_mb`, `biomes_mb`, `light_mb`, `heightmaps_mb`, `section_records_mb`, `column_records_mb`) |
| `columns` | A dedicated server's loaded columns, the same fields as `client_columns` |
| `server_columns` | The same for the single-player server thread's own levels; `null` when the window has no server thread |
| `shared_with_server` | Sections (and their MiB) that both the window and its server thread hold: what one copy of them costs |
| `server_generation` | The server thread's generation: proto chunks waiting for their neighbours (`protos`, `protos_mb` and the parts), light views, `caches` as `[name, entries, mb]` rows (`mb` is `null` for a cache that cannot say), and `stages` as `[name, count]` rows |
| `entities` | The server's entities in every level: `count`, `total_mb` and `by_type` as `[id, count, mb]` rows for the 30 types holding the most bytes. A record's bytes are its struct and the buffers its fields own, from capacities |
| `players` | The server's players: `count`, `total_mb` (queues, chunk lists, tracked sets), and for the 50 biggest by name `send_queue` as `[name, packets, mb]` rows (packets and bytes waiting for the network thread), `chunks_waiting`, `chunks_sent` and `tracked_entities` as `[name, count]` rows |
| `chunk_send` | What the hub keeps to send chunks: `sections` (encoded sections), `heightmaps` and `frames` (compressed packets shared by every connection), each with `entries` and `size_mb`, and `total_mb` |
| `light_queue`, `generation_queue` | Chunks being lit, and jobs queued or generating, with `len` and the `size_mb` of their bookkeeping tables (the data they point at is in `server_generation`) |
| `far_tiles` | Far terrain tiles: `active` and `cached`, and the tiles `kept` for a return with their `kept_mb` |
| `renderer.section_arenas_mb`, `section_vertices_mb` | The GPU buffers of the section vertex arenas, and the vertices that resident sections and far tiles hold in them |
| `renderer.cpu` | The renderer's CPU-side bookkeeping: `resident_sections_mb`, `far_tiles_mb`, `block_entities_mb`, `light_sources_mb` |
| `renderer.gpu` | The GPU allocator's `allocated_mb` and `reserved_mb`, and the 40 largest resources as `["label", mb]` rows in `by_label_mb`; `null` without Vulkan or DirectX 12 |
| `named_mb`, `unattributed_mb` | `named_mb` adds up the columns (a section both the window and its server thread hold once), `server_generation`, `entities`, `players`, `chunk_send`, the two queues, `renderer.cpu` and the far tiles' `kept_mb`. `unattributed_mb` is `process.resident_mb` less that; it can be negative where a census overlaps another, and `null` where the resident set is not read |
| `heap_by_tag_mb` | Only in an `alloc-census` build, otherwise `null`: see [the heap census](#who-allocated-it-the-heap-census) |

Which number answers which question:

- `process.resident_mb` is what the operating system charges the game. It includes the GPU driver's host-side memory, mapped files and memory mimalloc holds but has not returned.
- `process.allocator_commit_mb` is the memory mimalloc has committed, touched or not, so it can be higher than the resident set (1869 against 1268 MiB in a fresh single-player world). It is not the live heap; for that use the census.
- The columns, `server_generation` and `renderer.*` are censuses: each walks its structures and adds up the bytes it finds. What no census reaches shows as the resident set less the parts. A census is a count of what is reachable, so it does not see leaked or forgotten blocks; the heap census does.
- `renderer.gpu` is video memory, not RAM.

F3 shows three vanilla memory lines on Linux and Windows, read every half second: `Mem:` and `Allocation rate:` come from the anonymous resident memory (on Windows the private working set), `Allocated:` from all resident memory. The allocation rate is the growth of that figure per second, so it is zero while memory shrinks and it does not count blocks that are allocated and freed within the interval.

## How much a frame allocates: `alloc-count`

`alloc-count` makes the game's allocator count blocks. Each thread owns a slot of four counters (allocations, bytes allocated, frees, bytes freed) that only it writes, so counting is two loads and stores per allocation and per free, with no atomic read-modify-write and no shared cache line. A `realloc` counts as a free of the old size and an allocation of the new one, so a growing `Vec` shows as churn. Slots are never reused: a finished thread's counts stay in the total. Beyond 1023 threads the rest share slot 0.

```sh
cargo build --profile fast -p horizon-client --features alloc-count
```

Without the feature `horizon_threads::GameAllocator` is plain mimalloc: nothing is counted and nothing is added to the allocation path. `alloc-census` includes `alloc-count`. With `system-allocator` nothing counts, and `alloc` is `null` as in a build without the feature.

In a counting build `horizon-ctl stats`, `bench` and `fly` reply with an `alloc` object next to the frame times (`null` in other builds), over the rendered frames since the last reset:

```json
"alloc": {
  "window_thread": { "allocs_per_frame": { "avg": 487.5, "p50": 340, "p95": 627, "max": 83317 },
                     "kib_per_frame": { "avg": 247.8, "p50": 210.9, "p95": 449.4, "max": 9548.9 },
                     "frees_per_frame_avg": 388.4 },
  "process": { "allocs_per_s": 336733.2, "mib_per_s": 187.6, "frees_per_s": 319727.8, "net_mib": 83.8 }
}
```

- `window_thread` is the thread that draws the frame, between two rendered frames. Work on other threads (chunk generation, meshing, the server thread) is not in it.
- `process` is all threads together, from the first to the last frame since the reset (so it is missing with one frame), and `net_mib` is bytes allocated less bytes freed in that time: growth of the live heap, in the allocator's own accounting. The first rendered frame after the game starts has no entry, because it has nothing to subtract.
- With `raw` (`stats raw`, `fly ... raw`) the reply also has `alloc_count` and `alloc_bytes`, one entry per frame beside `frame_ms`.
- Counts are requested sizes, not the allocator's size classes, and they exclude the census header.

Cost, measured with `cargo run --profile fast -p horizon-threads --example alloc_count_bench` (10 million allocate-and-free pairs of 64 bytes, per thread, on a machine shared with other builds, three runs):

| | mimalloc | counting | added |
| --- | --- | --- | --- |
| 1 thread | 3.0 ns | 3.9 to 4.1 ns | 0.9 to 1.1 ns |
| 8 threads | 5.4 ns | 7.4 to 7.7 ns | 2.0 to 2.3 ns |

On the game, three alternating runs of `--benchmark standard` with and without the feature gave median `cpu_p50_ms` per scene of 0.217/0.156/0.186/0.192 (counting) against 0.215/0.166/0.169/0.220 (plain, village/forest/ocean/mountain) and `memory.rss_steady_mb` 1406 against 1399: inside the spread between runs of one build, so the cost on the game is not distinguishable from noise.

## Who allocated it: the heap census

`alloc-census` replaces the global allocator with `horizon_threads::CensusAllocator`, which also counts like `alloc-count`: mimalloc with a 16-byte header in front of each block (more for alignments above 16). The last four bytes of the header hold the block's tag, and `LIVE[tag]` is a relaxed atomic that every allocation adds to and every free subtracts from. `horizon-ctl memory` then lists `heap_by_tag_mb`, the 60 largest tags, as `["tag", mb]` rows.

Build it and read it:

```sh
cargo build --profile fast -p horizon-client --features alloc-census
target/fast/horizon-client --resources .local/horizon-client/26.3 --world T &
target/fast/horizon-ctl memory | jq '.heap_by_tag_mb'
```

A block's tag is what ran on its thread when it was allocated, innermost first:

1. a startup step (`startup::step`, `startup::time`), a timed span (`spans::guard`, `spans::time`, which most of the game's work runs inside) or a chunk-streaming job (`stream.generate`, `stream.decorate`, `stream.finish`, `stream.light`, `stream.mesh`);
2. a job of the shared worker pool (`jobs.rs` tags it with the job source's label);
3. otherwise the thread's name (`pthread_getname_np` on Linux and macOS, the Rust thread's name on Windows), or `unnamed thread`. Linux keeps 15 bytes of a name, so `World data preload` shows as `World data prel`.

Two things to keep in mind when reading it:

- A block keeps its tag for life. Another thread freeing or growing it does not change the tag, so a column generated by a pool job and kept by the server thread counts for the job.
- The tag says which code allocated, not which structure holds the block. A Vec grown inside `stream.mesh` and handed to the renderer stays under `stream.mesh`.

The census is for measuring: it is off in players' builds, and `heap_by_tag_mb` is `null` there. An external profiler needed more than 7 GB to run the game under heaptrack (18 GB with line tables); the census needs nothing besides its headers.

### What the census costs

Measured on Linux (Vulkan, `--offscreen`, `--profile fast`, one commit built with and without `--features alloc-census`), five runs of `horizon-client --benchmark standard` without the census and four with it, alternating, on a machine shared with other builds. Medians of the runs:

| | without | with `alloc-census` | change |
| --- | --- | --- | --- |
| `memory.rss_steady_mb` | 1431.2 | 1457.0 | +25.8 MiB (+1.8%) |
| `memory.rss_peak_mb` | 1649.1 | 1677.6 | +28.5 MiB (+1.7%) |
| `frames[village].cpu_p50_ms` | 0.222 | 0.226 | +0.004 ms |
| `frames[forest].cpu_p50_ms` | 0.158 | 0.159 | +0.001 ms |
| `frames[ocean].cpu_p50_ms` | 0.170 | 0.174 | +0.004 ms |
| `frames[mountain].cpu_p50_ms` | 0.196 | 0.198 | +0.002 ms |

The memory cost is the headers (16 bytes a block, 32 or more for larger alignments). The CPU time of a frame moves by about 2%, which is inside the spread between the runs of one build (village 0.214 to 0.296 ms without the census). Chunk generation per second was in the same range in both. The runs were short and the machine was busy, so read these as "small", not as exact.


## Where one frame goes: spans, spikes and traces

The game times its work in named spans (`crates/horizon-client-render/src/spans.rs`). Three tools read them:

- `HORIZON_SPIKES=MS` times the spans on every thread and sets the slow ring's threshold: the last 256 frames (or server ticks) slower than MS milliseconds are kept with their largest spans and the render thread's CPU time in the frame. Nothing is logged; every capture carries the ring, and `horizon-perf query ticks --worst 10` or `query spans --slow` reads it.
- `horizon-ctl trace FILE [SECONDS]` writes every span of every thread to FILE, a `.hzperf` capture, for the given time (or until `horizon-ctl trace stop`; the spans stop at 60 s); `HORIZON_TRACE=FILE` writes one once the first hardware probe has finished. The startup steps stay in `HORIZON_STARTUP_TRACE`.
- `horizon-ctl stats [reset] [raw]`, `horizon-ctl bench SECONDS` and `horizon-ctl fly ... SECONDS [PITCH [raw]]` reply with the frame-time summary: frame, CPU and GPU time percentiles and the average of each phase of the frame. They carry no memory figures, except `alloc` in an `alloc-count` build.

See [Graphics](graphics.md) for how the spans are used to chase a slow frame.

## Resident memory in benchmark reports

`horizon-client --benchmark standard` (see [Benchmark report format](benchmark-report.md)) samples the process's resident set four times a second on Linux and macOS, from `/proc/PID/status` (`VmRSS`) or `ps`. Its report holds `memory.rss_peak_mb` (the maximum over the run) and `memory.rss_steady_mb` (the median while the four scenes are flown), and each scene's entry in `frames` has `cpu_p50_ms`, the median CPU time of a frame (the frame less its waits for the GPU, the display and the limiter). `tools/perf_bench.py` writes the same metrics for vanilla and for Horizon, for comparisons between builds.

## The renderer alone: `scene_replay --memory`

`scene_replay` draws a captured scene offscreen without the game (see [Graphics](graphics.md#render-capture-and-replay)). With `--memory on` its JSON adds a `memory` object after the timed frames: `gpu`, the renderer's GPU resources by name in MiB, and on macOS `footprint`, the process's physical footprint by category from `/usr/bin/footprint` (`null` elsewhere). The example declares the game's allocator, so the figures are the game's.

## Heap profilers: `system-allocator`

`--features system-allocator` sends Rust's allocations to the system heap instead of mimalloc, so profilers that replace `malloc` (heaptrack, valgrind's massif) see them. Use it on small probes only: the whole game is far too large for them (heaptrack needed more than 7 GB to follow it), and the system heap is slower than mimalloc. `system-allocator` wins over `alloc-census` when both are given.

```sh
cargo build --profile fast -p horizon-client --features system-allocator
heaptrack target/fast/horizon-client --resources .local/horizon-client/26.3 --world T
```
