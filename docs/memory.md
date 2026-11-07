# Memory

Where the game's memory goes, and the tools that say so. This page describes what exists today; each new tool adds its section when it lands.

Start with `horizon-ctl memory` for what holds memory now, add the heap census to see who allocated it, and fall back to an external heap profiler only on small probes.

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
| `client_columns` | The window's loaded columns: `total_mb`, counts of `columns`, `sections`, `unique_sections` (each shared section once), `uniform_block_sections`, `light_arrays`, and the heap bytes by part (`blocks_mb`, `biomes_mb`, `light_mb`, `heightmaps_mb`, `section_records_mb`, `column_records_mb`) |
| `server_columns` | The same for the single-player server thread's own levels; `null` when the window has no server thread |
| `shared_with_server` | Sections (and their MiB) that both the window and its server thread hold: what one copy of them costs |
| `server_generation` | The server thread's generation: proto chunks waiting for their neighbours (`protos`, `protos_mb` and the parts), light views, `caches` as `[name, entries, mb]` rows (`mb` is `null` for a cache that cannot say), and `stages` as `[name, count]` rows |
| `far_tiles` | Far terrain tiles: `active` and `cached` |
| `renderer.section_arenas_mb`, `section_vertices_mb` | The GPU buffers of the section vertex arenas, and the vertices that resident sections and far tiles hold in them |
| `renderer.cpu` | The renderer's CPU-side bookkeeping: `resident_sections_mb`, `far_tiles_mb`, `block_entities_mb`, `light_sources_mb` |
| `renderer.gpu` | The GPU allocator's `allocated_mb` and `reserved_mb`, and the 40 largest resources as `["label", mb]` rows in `by_label_mb`; `null` without Vulkan or DirectX 12 |
| `heap_by_tag_mb` | Only in an `alloc-census` build, otherwise `null`: see [the heap census](#who-allocated-it-the-heap-census) |

Which number answers which question:

- `process.resident_mb` is what the operating system charges the game. It includes the GPU driver's host-side memory, mapped files and memory mimalloc holds but has not returned.
- `process.allocator_commit_mb` is the memory mimalloc has committed, touched or not, so it can be higher than the resident set (1869 against 1268 MiB in a fresh single-player world). It is not the live heap; for that use the census.
- The columns, `server_generation` and `renderer.*` are censuses: each walks its structures and adds up the bytes it finds. What no census reaches shows as the resident set less the parts. A census is a count of what is reachable, so it does not see leaked or forgotten blocks; the heap census does.
- `renderer.gpu` is video memory, not RAM.

F3 shows three vanilla memory lines on Linux and Windows, read every half second: `Mem:` and `Allocation rate:` come from the anonymous resident memory (on Windows the private working set), `Allocated:` from all resident memory. The allocation rate is the growth of that figure per second, so it is zero while memory shrinks and it does not count blocks that are allocated and freed within the interval.

## Who allocated it: the heap census

`alloc-census` replaces the global allocator with `horizon_threads::CensusAllocator`: mimalloc with a 16-byte header in front of each block (more for alignments above 16). The last four bytes of the header hold the block's tag, and `LIVE[tag]` is a relaxed atomic that every allocation adds to and every free subtracts from. `horizon-ctl memory` then lists `heap_by_tag_mb`, the 60 largest tags, as `["tag", mb]` rows.

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

- `HORIZON_SPIKES=MS` logs the spans of every frame slower than MS milliseconds, with the render thread's CPU time in the frame.
- `horizon-ctl trace FILE [SECONDS]` writes every span of every thread to FILE as Chrome trace events for the given time (or until `horizon-ctl trace stop`); `HORIZON_TRACE=FILE` writes one from the start, so startup is traced too. Open the file in [ui.perfetto.dev](https://ui.perfetto.dev).
- `horizon-ctl stats [reset] [raw]`, `horizon-ctl bench SECONDS` and `horizon-ctl fly ... SECONDS [PITCH [raw]]` reply with the frame-time summary: frame, CPU and GPU time percentiles and the average of each phase of the frame. They carry no memory figures.

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
