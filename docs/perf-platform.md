# Spec P: one performance platform: the `.hzperf` capture, the `horizon-perf` dev tool, and the web UI

> Specs A-H and S1-S3 and the `perf/*` branches named here are the performance work tracked in #11751-#11755; they are being opened as PRs.

Design for the volunteers who build it. The design doc lives in the repo (`docs/perf-platform.md`); work
is tracked as one issue per piece (§11) under one tracker issue. The contract, meaning the `.proto` schema and the
fixtures, is reviewed by UMCEKO until it lands; after that, anyone can change it (no CODEOWNERS).

It supersedes the output side of spec H (HTML report, PR 4b viewer), the `memory-diff` command and `.hzap` file of
spec D, `tools/bench_stats.py` of spec A, and every ad-hoc perf script. Producers stay as their specs define them (H
sampler, C GPU detail, D counters, A scenarios, B scene corpus).

The rule it serves (user, 2026-11-07): few good tools, not many. There is one file format and one dev tool that answers
questions about it. The web UI draws what that tool computes. Nothing new reads perf data any other way.

Audiences, all from day one:

- **AI agents**: `horizon-perf summary` (1,500 tokens) and `horizon-perf query`, from the CLI or `horizon-perf mcp`.
- **Server owners**: `horizon-report` writes a file; they drag it into the web UI (self-hosted by the team; §7).
- **Developers**: the same CLI, `diff` with statistics, and the web UI's tree, flame graph and compare mode.

Line numbers are on `origin/main` = `1d63e0a5a3` unless a branch is named (`branch:path:line`). Vanilla citations are
in `.local/minecraft/26.3/sources/server/`.

## 1. Inventory: every perf tool today, and what happens to it

### On main

| Tool | What it is | Fate |
|---|---|---|
| `horizon-report [SECONDS\|stop\|status]` (dedicated console) and `horizon-server report` | `crates/horizon-client/src/game/dedicated/report.rs`; console parse `game/dedicated.rs:1651-1687`; recorder fed each tick `dedicated.rs:1343-1356`; max 600 s (`report.rs:23`). Writes `reports/perf-<time>.json` (`"format": 2`, keys `report.rs:370-393`) and a 9-line `.txt` (`report.rs:414-533`) in the server folder. `horizon-server report` POSTs the console line and polls `reports/` (`crates/horizon-server/src/cli.rs:289-347`). Nothing in `tools/` or CI reads `perf-*.json`. | **Folds.** The command names stay; the output becomes one `.hzperf` (§3). Ticks go to `health` and `span_blocks`; `spans`, `worst_ticks` and `chunk_workers` go to `spans` and `workers`; `mob_costs` and `levels` go to `world`; `network` goes to `net`; `machine`, `config` and `process` go to `Start`; `overhead` goes to `Stop.overhead`. The `.txt` is gone: the command prints the file's path and the web UI's address, and `horizon-perf summary` is the text form. |
| `tick_profile.rs` | Mob cost by type, 1 tick in `SAMPLE_EVERY = 4` (`game/tick_profile.rs:16`); span users bitmask `SPANS_SPIKES/REPORT/MOD` (`:38-51`). | **Stays as a producer** of `world.entity_costs`. Its span-user switch gains `SPANS_CAPTURE`. |
| `HORIZON_SPIKES=MS` slow-frame / slow-tick lines | `crates/horizon-client-render/src/spans.rs`; frame line `game.rs:20055-20058`, tick line `game/levels.rs:631-640`; dedicated default 250 ms (`dedicated.rs:673-676`). Parsed by `tools/pacing_bench.py:103` and by the ad-hoc scripts. | **Leaves the log entirely.** Slow ticks and frames go into the in-memory slow ring, which every capture carries (`slow`, §3). The env var stays as the ring's threshold. Vanilla's "Can't keep up!" warning is untouched (`dedicated.rs:316`, rate limit `:678`; vanilla `MinecraftServer.java:723`). |
| `HORIZON_TRACE=FILE`, `horizon-ctl trace`, console `horizon-trace` | Chrome-trace JSON writer `spans.rs:303-454`, streamed with a 256 KiB buffer, global mutex per span while on; ctl row `commands.rs:587` (`mcp: false`); console max 300 s (`report.rs:563`). | **Folds.** The span timeline becomes `timeline` chunks of a capture, and Perfetto JSON becomes `horizon-perf export --perfetto`. The three entry points stay as aliases that start a timeline-only capture. |
| `HORIZON_STARTUP_TRACE=FILE` | JSON-lines startup steps, marks and counters (`spans.rs:456-641`); read by `tools/startup_trace.py`. | **Stays for now.** It runs before the game home and reports folder exist, and it streams line by line so a startup crash keeps what ran. It could fold in once `HORIZON_PROFILE` covers startup (later, §11). |
| `gpu-passes [on\|off]`, F3 pass timings, `HORIZON_SHADER_PACK_TIMING` | `PassTiming` in `gfx/pack_timing.rs` (64 passes, 15-frame medians, `:16-20`, `:273-283`); ctl `game/control.rs:454-462`, row `commands.rs:586`; F3 `game/debug_lines.rs:160-176`; log line `pack_timing.rs:264-267`; bench metrics `benchmark.rs:119-150`. | **The producer folds into `gpu`.** `gpu-passes` stays as the live switch (`benchmark.rs` sends it in-process), and F3 stays as a live overlay. The `HORIZON_SHADER_PACK_TIMING` log line is retired: `profile client start --gpu` records the same data. |
| `crates/horizon-bench` (benchmark report v1) and `--benchmark SCENARIO` | `FORMAT = "horizon-benchmark-report"`, `VERSION = 1` (`crates/horizon-bench/src/lib.rs:18-19`); `Report` `:222-243`; telemetry upload via `for_upload` `:295-303`; written to `<game_dir>/benchmarks/`. Also written by the launcher (`launcher/src/perf.rs`) and re-implemented by `tools/perf_bench.py:137-138`. | **Stays as the share and upload format** (telemetry contract, 128 KiB cap, launcher). A capture's `bench` field embeds reports verbatim. `horizon-perf` also reads a bare `benchmarks/*.json`, so `diff` compares benchmark reports too. |
| `tools/server_bench.py` | Load driver: MSPT from `/tick query`, RSS/CPU, chunks/s, bot counts; report kind `horizon-server-benchmark` (`:595-609`). Compares Horizon with vanilla and other servers, which cannot write `.hzperf`. | **Stays as the driver.** It gains a `--profile[=ARGS]` flag, which runs `horizon-report` for the measure window and adds its run record with `horizon-perf attach`. This replaces `perf/streaming-caps`' `--horizon-report` and `perf/sampler`'s `--console-at`. |
| `tools/perf_bench.py`, `graphics_bench.py`, `pacing_bench.py`, `entity_occlusion_bench.py`, `bench.py` | Client and server drivers with their own JSON. | **Stay as drivers**, out of scope. `pacing_bench.py` stops parsing `slow frame` log lines and runs `horizon-perf query ticks --worst` on its capture (it needs each frame's span list). |
| `horizon-telemetry` (opt-in) | `Perf` payload ≤ 4 KiB, 12 a day (`payload.rs:9-50`, `:212-237`); consent switches off by default (`consent.rs:1-40`); scrubber `scrub.rs:1-10`. | **Stays separate.** Consent, size caps and the upload path are a different contract. Telemetry never queues or uploads a `.hzperf`. |
| `alloc-census` | Compile feature; tagged mimalloc wrapper (`crates/horizon-threads/src/heap_census.rs:182`); output only as `heap_by_tag_mb` in `horizon-ctl memory` (`game/memory.rs:168-170`). | **Producer.** Its snapshot goes into `memory` at capture start, every 10 s and at stop. |
| `horizon-ctl memory`, `status`, `stats`, `bench`, `fly … raw` | Live control replies (`control.rs:529-689`, `commands.rs:344-597`). | **Stay** as live state for agents and the benchmark driver. Captures reuse their producers; `FrameStats` feeds `health`. |
| Dedicated `server-status`, supervisor metrics, Prometheus | `game/dedicated/status.rs:132-173`; supervisor 1 Hz samples `crates/horizon-server/src/supervisor.rs:619-680`; `/api/v1/metrics/prometheus` (`api.rs:51-65`). | **Stay** for live external monitoring. `health` uses the same field names (`tps`, `mspt`, `rss`, `cpu`). Exporting counters there is a later PR (§3.6). |
| `/debug start\|stop`, `/jfr`, `/perf` | Vanilla commands (`game/commands/extra.rs:1006-1163`); `/perf` is listed but has no executor. | **Stay.** They are vanilla parity, not this platform; `/perf` is a separate parity item. Remote profiling borrows its permission level (§4.2). |
| F3+2/F3+3 charts, performance overlay | `debug_chart.rs`, `perf_overlay.rs`. | **Stay** (live UI). |
| `render-capture` / `scene_replay` | Input fixture: mesh state + camera, hundreds of MB to 1.45 GB, built from Mojang assets (spec B). | **Stays separate.** A scene capture is a renderer input, not a measurement. `scene_replay --json` results can be stored as a `bench` entry referencing the scene by `corpus_id`. |
| `HORIZON_SPLIT_STATS`, `HORIZON_OCCLUSION_PROFILE` | Subsystem log lines every few seconds (`game/split/frames.rs:1-10`, `game/occlusion_culling.rs:856`). | **Fold when next touched**, as counters (§3.6). No piece of §11 does it; nothing reads them. |
| `examples/*_bench.rs`, `*_profile.rs` | Microbenchmarks. | **Stay**, out of scope. |

### On local branches (not on main)

| Branch | What it adds | Fate |
|---|---|---|
| `perf/sampler-core` ⊂ `-stackcopy` ⊂ `-report` ⊂ `perf/sampler` ⊂ `perf/sampler-viewer` (= `perf/sampler-tree`, same tree `d5dc833c75`); `perf/sampler-context` is an outdated sibling | SIGPROF/SIGRTMIN per-thread timers, rings, `.eh_frame` stack copies (`perf/sampler-viewer:crates/horizon-threads/src/sampler/linux.rs:39-77`, `sampler.rs:421-485`); context stores `horizon_threads::work` (`work.rs:117-180`); bounded trie aggregation (`profile/agg.rs:8-37`); in-process symbolization; writer emitting `profile-*.html` + `.pb.gz` + `.folded` (`profile/write.rs:29-105`), with `viewer.js` inlined into every HTML (`profile/viewer.rs:10`, `:209-212`); `horizon-report --profile`, `horizon-ctl profile`, F3+O, `HORIZON_PROFILE=DIR` (`crates/horizon-client/src/profiling.rs:104-308`). | **The producer lands; the outputs fold.** Sampler, stack copies, context stores, aggregation, symbolization and the commands land as spec H plans them. `html.rs`, `viewer.rs`, `viewer.js` and the folded writer do not land: the `samples` field replaces them, the web UI (§7) replaces the HTML, and `horizon-perf export` (§6.4) replaces the pprof and folded writers. **`perf/sampler-viewer` is not opened as is.** Its tree logic informs the web UI. |
| `perf/alloc-count` | Per-thread alloc counters (`crates/horizon-threads/src/alloc_count.rs:35-171`) in `stats`/`bench` replies; `docs/memory.md`. | **The producer lands.** The counters become `health` fields (`allocs_per_s`, `alloc_mib_per_s`). `docs/memory.md` gains a capture section. Its conflict with `perf/sampler`'s 5-line `docs/memory.md` is resolved by keeping alloc-count's version. |
| `perf/bench-ab-statistics-impl` (A PR 1) | Hodges–Lehmann + exact Wilcoxon signed-rank CI (`crates/horizon-bench/src/compare.rs:54-109`), verdicts, `Report.ab`; Python twin `tools/bench_stats.py`. | **Lands without `tools/bench_stats.py`.** The paired statistic stays in `horizon-bench`: the game computes `Report.ab` and the launcher shows it, so it must ship, and `horizon-perf` is not shipped. `horizon-perf` reads `Report.ab`; it does not recompute it (§6.6). The Python vectors become Rust tests in `horizon-bench`. |
| `perf/bench-travel` (= `perf/bench-ab`) | `--benchmark travel`, `--benchmark-set` (`perf/bench-travel:crates/horizon-client/src/benchmark.rs:58-140`, `:476-518`). | **Lands as is.** It is a scenario, so a producer. Piece B1 adds `--profile` to `--benchmark`. |
| `perf/capture-format-v1`, `-packed`, `-journal` (= `perf/scene-corpus`) | Scene capture v1 (directory, `format`/`version` with `MAX_MAJOR`, `perf/capture-journal:crates/horizon-client-render/src/gfx/capture.rs:765-779`), journal `HZJ1`. | **Stays separate** (input fixture). |
| `perf/gpu-pass-detail`, `-counters`, `-detail-output` (C PRs 1-3) | Dispatch parts, pipeline statistics, `gpu_pass_parts`/`gpu_counters` in stats JSON (`perf/gpu-pass-detail-output:crates/horizon-client-render/src/gfx/pack_timing.rs:852`), F3 lines, `HORIZON_GPU_PASS_DETAIL`. | **The producer lands.** Piece G1 records the same data into `gpu`. `HORIZON_GPU_PASS_DETAIL` becomes an alias of `profile client start --gpu=detail`. |
| `perf/streaming-caps` `212d478830` (S3-0) | Streaming `updates` and caps in `perf-*.json` (`perf/streaming-caps:crates/horizon-client/src/game/dedicated/report.rs:216-225`), `server_bench.py --horizon-report` (`:412-422`, `:477-478`). | **Absorbed.** `job_stats::take_updates()` (`streaming.rs:39-66`) feeds counters (`streaming.updates`, `streaming.cap{kind=decoration\|generation\|light}`). The bots-stderr fix lands alone; `--horizon-report` becomes `--profile`. |
| `perf/hitch-fixes` | `culprit_with_wait`, `off-CPU` in the slow-frame line (`perf/hitch-fixes:crates/horizon-client-render/src/spans.rs:228-269`). | **Lands into the slow ring, not the log.** `slow` keeps `cpu_ms` per slow frame, so "waited" is a query (`spans --slow` reports `waited_ms`). |
| `perf/tick-proof-tools` | Tick digest (`HORIZON_TICK_DIGEST`). | **Stays separate:** it is a determinism proof, not a timing tool. |

### Ad-hoc scripts from the perf work → `horizon-perf query`

These scripts are the requirements for §6. A query answers each of them, so none is needed again.

| Script | What it computed | Replacement |
|---|---|---|
| `slowticks.py` | Per span over slow-tick log lines: wall, cpu, waited ms per tick | `query spans --slow` |
| `spikes.py` | Per span over slow frames: total, frame count, worst | `query spans --slow --side client --thread render` |
| `inclusive.py` | Inclusive % per function from folded stacks, filtered by substring | `query hot --by total --match horizon` |
| `s1_breakdown.py` | Inclusive share and ms/tick for ~40 regex-named frames | `query frames '/tick_lan$/' '/Hub>::tick_level$/' …` |
| `s1_children.py` | Children of the innermost `FUNC` frame, merged, as % and ms/tick | `query callees f:… --depth 1` |
| `s1_table.py`, `s3_table.py` | Same key frames across several runs, one column each | `query frames NAMES A.hzperf B.hzperf C.hzperf` (one column per capture) |
| `s1a_spans.py` | Mean span ms/tick over the last N seconds, plus MSPT | `query spans --from -60` |
| `s1b_table.py`, `s3a_table.py` | Runs table: MSPT, bots, chunks/s, worker cores, streaming caps, span ms | `query spans`/`series`/`world`/`bench` over several captures |
| `s3_counters.py` | Rates from ad-hoc `s3:` counter log lines | counters (§3.6) + `query series streaming.updates` |
| `s3_windows.py` | Slow-tick spans per 30 s window, with bot count | `query series span:tick_lan span:streaming h:players --every 30` |
| `h5_top.py` | Scraped tables out of the generated HTML | Obsolete: there is no HTML; `query hot` |
| `h5_perf_top.py` | Top self symbols from `perf script` | `query hot --by self` (the sampler matched `perf` on 8 of the top 10) |
| `*_profile.sh`, `*_fold.sh` | `perf record` + `inferno-collapse-perf` | `horizon-report 60` / `horizon-ctl profile start`; `horizon-perf export --folded` if a folded file is still wanted |

## 2. Architecture

Two crates. The game ships the small one; everything that reads a capture lives in the dev-only one.

```
 producers                                                    shipped: horizon-perf-format
 horizon-threads: sampler, work contexts, aggregation, symbols ─┐  ┌─ generated types (Capture, …)
 horizon-client-render: spans, FrameStats, PassTiming, streaming─┼─►│  gzip writer + writer thread
 game: tick_profile, lan hub bytes, levels, memory              ─┘  └─ health recorder, counter registry
                                                                         │
                                                       reports/profile-*.hzperf
                                                                         │
                                         dev-only, never shipped: horizon-perf (lib + bin)
                                         reader · follow reader · query · stats · summary · ids · diff · export
                                              │                    │                     │
                                       CLI subcommands      horizon-perf mcp     wasm32 build ──► web UI
                                                                                                 (horizon-site)
```

### 2.1 `crates/horizon-perf-format` (shipped)

It holds exactly three things:

- **The generated types.** It holds the `.proto` files under `proto/horizon/perf/v1/` and a `build.rs` that compiles
  them with `protox` (a pure-Rust `.proto` compiler, so neither volunteers nor CI install `protoc`). `prost-build`'s
  `compile_fds` turns the descriptor set into Rust types. The runtime dependency is `prost`.
- **The gzip writer.** It sits on `flate2` (pure-Rust backends only: `miniz_oxide` in standalone and wasm builds; in
  the game, Cargo's feature unification turns on the workspace's `zlib-rs`, also pure Rust). It runs on its own writer
  thread, which drains a bounded channel; a full channel makes the producer drop the message and count it, never
  block, except `Stop`, which waits (§3.4).
- **The in-memory health recorder and the counter registry** (§3.6), as pure data structures fed by the game.

One recorded exception: `files` (capture file names, retention, the saved line, `is_complete`) lives here because
`horizon-server` and the game both need it and the server cannot depend on `horizon-client` (§13, J).

Nothing else goes in: no reader, no query, no statistics, no JSON, no serde. It builds for `wasm32-unknown-unknown`, so
`horizon-perf` and the web UI share its types. No size budget is set for now. If the crate grows, compile features
can gate parts of it later.

New dependencies: `prost` (runtime), `protox` and `prost-build` (build only). None of them is in `Cargo.lock` on
`1d63e0a5a3`.

### 2.2 `crates/horizon-perf` (dev-only)

It is a workspace member with `publish = false`, and no release workflow packages it. It is one library and one
binary, `horizon-perf`. Modules: `read` (whole-file reader), `follow` (incremental reader), `ids` (§5.2), `query` (§6),
`stats` (§6.6), `summary` (§5), `export` (pprof, folded, Perfetto), `attach`, `cli`, `mcp` (§8), `wasm` (C-ABI exports
for the web UI, behind feature `wasm`).

Dependencies: `horizon-perf-format`, `prost`, `flate2`, `serde_json` (query replies are JSON for agents; this is
not an export of the capture). It depends on no other internal crate. The library builds for `wasm32-unknown-unknown`,
so it uses no `libc`, no threads, no clock and no file system outside `cli` and `mcp`.

Benchmark reports are read through a small mirror struct of the fields `horizon-perf` queries, rather than by
depending on `horizon-bench`. That crate pulls in `horizon-telemetry` (hardware probes, `getrandom`), which is not
known to build for wasm32. A dev-dependency test checks that a report written by `horizon-bench` parses.

### 2.3 Dependency direction

| Crate | Depends on | Notes |
|---|---|---|
| `horizon-perf-format` | `prost`, `flate2` (pure-Rust backends); build: `protox`, `prost-build` | no internal crates |
| `horizon-client-render` | `horizon-perf-format` | converts `horizon_threads` aggregates, spans and `PassTiming` into messages (`perf_capture.rs`) |
| `horizon-client-lib`, `horizon-client` (game, dedicated server) | `horizon-perf-format` | triggers, the writer, the remote channel (§4.2) |
| `horizon-threads` | nothing new | stays free of internal dependencies; it keeps producing its in-memory `Profile` |
| `horizon-perf` | `horizon-perf-format` | never a dependency of anything shipped |
| web UI (`sunrise-org/horizon-site`) | `horizon_perf.wasm`, a build artifact of this repo | draws only |

The conversion from `horizon_threads::Profile` to the `samples` message lives in horizon-client-render. It replaces
`profile/viewer.rs`, which today builds the same columnar data inside horizon-threads
(`perf/sampler-viewer:crates/horizon-threads/src/profile/viewer.rs:77-206`).

`horizon-ctl` and `horizon-server` gain no reading code. They trigger captures, and the game prints the written path
and the web UI's address.

## 3. The capture: `.hzperf`

### 3.1 Encoding

A `.hzperf` file is one gzip stream (RFC 1952, a single member) whose inflated bytes are one protobuf `Capture`
message. Sections are fields of `Capture`. There are no frames, no index and no per-section codecs. The format offers no
partial reads and no crash recovery: a reader inflates the whole stream. Readers are our own tools (`horizon-perf` and
the web UI's wasm), never raw `protoc --decode`.

Why protobuf (decision Q2): the schema is the contract, decoding is fast and uses little memory, and the timeline
scales. As a measured point of comparison, the 2-minute server profile's columnar JSON is 547 KB raw and 120 KB
deflated. pprof stays an export.

### 3.2 Live-follow rules (part of the contract)

These rules let a reader watch a capture grow while it is being written.

1. **Whole fields only.** The writer appends whole top-level fields of `Capture` (tag, length, message) and nothing
   else. After each one it does a deflate sync flush (`FlushCompress::Sync` through `flate2`), so the bytes on disk always
   inflate to a whole number of top-level fields.
2. **Live data is repeated and appended.** Anything meant to be visible live (health samples, counters, the slow
   ring, span blocks, timeline chunks, marks, warnings, memory snapshots) is a `repeated` field. Each new value is one
   more element, appended. This relies on protobuf's rules that concatenated messages merge and that repeated fields
   append.
3. **End-only data is written last.** This covers the call-tree aggregate (`samples`), span totals and worst ticks,
   `gpu`, `net`, `workers` and `bench`. `Stop` is each side's last field.
4. **Every top-level field except `format` is repeated, and every message except the capture-wide `Attachment` carries its `side`.** Two sides' streams
   can then be concatenated without a singular field of one overwriting the other's (§4.2).
5. **The follow reader** (`horizon_perf::follow`) inflates incrementally, buffers, and yields each top-level field once
   its last byte has arrived. A side is complete when its `Stop` has arrived. A file whose sides lack a `Stop` is a
   capture still being written, or one cut short by a crash. The whole-file reader refuses such a file with
   `this capture has no end for its server side: it is still being written or the game stopped` and never tries to
   salvage it.

The file is written under its final name, so `horizon-perf follow` can open it while it grows. Retention (§3.7)
counts only finished files.

### 3.3 Schema and versioning

- **The contract** is `crates/horizon-perf-format/proto/horizon/perf/v1/*.proto`. A change to it, or to the fixtures,
  needs the contract owner's review.
- **Field rules are protobuf's.** Never reuse a field number. Removed fields are listed under `reserved`, both
  numbers and names. A new field is a new number; old readers skip it. A field never changes type or meaning. A change
  that would need that is a new field.
- **`Capture.format`** is `1`. It changes only if the live-follow rules themselves change. A reader refuses a larger
  value with `this capture is format N; this horizon-perf reads up to M: update it`.
- **Frozen fixture:** `crates/horizon-perf-format/tests/fixtures/v1.hzperf`, a few KB, written once by hand-built
  messages and never regenerated. It must always decode, with the values the test lists. Every later schema version
  adds its own frozen fixture beside it.
- **Unknown fields:** `prost` drops unknown fields when it decodes. So the tools that rewrite a capture (`attach`, the
  remote merge in §4.2) work on the wire bytes, never decode and re-encode, and keep fields they do not know.
- **Canonical JSON compatibility.** There is no JSON export for now, but no construct may break protobuf's canonical
  JSON mapping, so one can be added later. Concretely:
  - proto3 only, and no groups;
  - field names in `lower_snake_case`, with no two fields mapping to the same JSON name;
  - every enum has a `*_UNSPECIFIED = 0` value;
  - map keys are strings or integers only;
  - no `google.protobuf.Any`;
  - times are integer milliseconds or seconds as `double`, never a home-made string format.

  A test over the compiled descriptor set enforces this list.

### 3.4 Writing

- One writer thread per process, named `horizon-perf-writer`, owns the file and the deflate state. Producers send
  finished messages through a bounded channel (4,096 messages). A full channel drops the message and increments
  `Stop.overhead.dropped_messages`. A tick or frame never waits on the writer.
- Strings in live data are interned: the first use of a string by a side appends a `StringEntry` (`side`, `id`, `text`),
  and messages refer to `id`. The end-only `samples` uses the same table.
- `t` is seconds since that side's `Start.started_unix_ms`. Ticks are the server's tick counter; frames are the client's
  frame counter. Units are in field names (`_ms`, `_mb`, `_bytes`, `_per_s`).

### 3.5 The `Capture` message

Sketch: the top-level field numbers are meant to be final; nested messages that a comment only describes (`Build`,
`Machine`, `Options`, `Functions`, `Overhead`, the enums `Kind`, `Trigger`, `WarningCode`, `StopReason`…) are spelled
out in F1 and the piece that adds them, for the contract owner's review. Comments mark each
top-level field: **start** means written once per side when the capture starts, **live** means appended during the
capture, **end** means written once per side at stop, before `Stop`.

```proto
syntax = "proto3";
package horizon.perf.v1;

message Capture {
  uint32 format = 1;                       // start: 1; the only singular field
  repeated Start start = 2;                // start: one per side, the side's first field
  repeated StringEntry strings = 3;        // live: interned strings, appended on first use
  repeated HealthWindow history = 4;       // start: the in-memory tiers from before the capture
  repeated SlowEvent slow_history = 5;     // start: the slow ring from before the capture
  repeated WorldSnapshot world = 6;        // start and end: label "start" / "stop"
  repeated Mark marks = 7;                 // live
  repeated Warning warnings = 8;           // live
  repeated HealthSample health = 9;        // live: one per second per side
  repeated CounterDef counter_defs = 10;   // live: appended when a counter first reports
  repeated CounterSample counters = 11;    // live: one per second per side
  repeated SlowEvent slow = 12;            // live
  repeated SpanBlock span_blocks = 13;     // live: one per block per thread
  repeated MemorySnapshot memory = 14;     // live: start, every 10 s, stop
  repeated TimelineChunk timeline = 15;    // live: one per second per thread while the timeline is on
  repeated SpanSummary spans = 16;         // end
  repeated Workers workers = 17;           // end
  repeated SampleProfile samples = 18;     // end: the call-tree aggregate
  repeated GpuPasses gpu = 19;             // end: clients only
  repeated NetTraffic net = 20;            // end: servers, and a client's own bytes
  repeated BenchResult bench = 21;         // end
  repeated Attachment attachments = 22;    // end: added by drivers through horizon-perf attach
  repeated Stop stop = 23;                 // end: the side's last field
  repeated GpuFrames gpu_frames = 24;      // live: clients only, the timed frames, a block about every second
}

enum Side { SIDE_UNSPECIFIED = 0; SIDE_CLIENT = 1; SIDE_SERVER = 2; }
enum Unit { UNIT_UNSPECIFIED = 0; UNIT_TICK = 1; UNIT_FRAME = 2; }

message StringEntry { Side side = 1; uint32 id = 2; string text = 3; }

message Start {
  Side side = 1;
  Kind kind = 2;                           // DEDICATED, INTEGRATED (single-player server thread), CLIENT
  Trigger trigger = 3;                     // CONSOLE, CTL, KEY (F3+O, old captures), ENV, BENCHMARK, REMOTE, COMMAND
  int64 started_unix_ms = 4;
  double uptime_s = 5;
  Build build = 6;                         // version, commit, profile, target, features, symbols (DWARF/SYMTAB/NONE)
  Machine machine = 7;                     // os, os_version, arch, cpu, cpu_threads, cpu_cores, ram_mb, cgroup limits, gpu
  Options options = 8;                     // hz, threads, ticks_over_ms, wall, timeline, gpu mode, samples on/off
  Settings settings = 9;                   // typed allowlist: view/simulation distance, max players, compression threshold
  GameRules game_rules = 10;               // typed allowlist
  HorizonOptions horizon_options = 11;     // typed allowlist of Horizon switches, boolean or numeric
  repeated Mod mods = 12;                  // id, version, kind, crate
  bool same_process = 13;                  // the integrated server shares the client's OS process
  uint32 players = 14;
  uint64 world_fs_used_mb = 15;
  uint64 world_fs_total_mb = 16;
}

message HealthSample {
  Side side = 1;
  double t = 2;
  optional float tps = 3;                  // servers
  optional float mspt_avg = 4;
  optional float mspt_p50 = 5;
  optional float mspt_p95 = 6;
  optional float mspt_max = 7;
  optional float fps = 8;                  // clients
  optional float frame_ms_p50 = 9;
  optional float frame_ms_p95 = 10;
  optional float frame_ms_p99 = 11;
  optional float frame_ms_max = 12;
  optional float frame_cpu_ms_p50 = 13;
  optional float frame_gpu_ms_p50 = 14;
  float cpu_process_cores = 15;
  float cpu_system_cores = 16;
  float rss_mb = 17;
  float physical_used_mb = 18;
  float swap_used_mb = 19;
  optional float allocator_commit_mb = 20;
  optional float allocs_per_s = 21;        // alloc-count builds
  optional float alloc_mib_per_s = 22;
  uint32 players = 23;
  uint32 chunks = 24;
  uint32 entities = 25;
}

message HealthWindow { Side side = 1; float step_s = 2; double t0 = 3; repeated HealthSample samples = 4; }

message CounterDef { Side side = 1; uint32 id = 2; uint32 name = 3; repeated Label labels = 4; uint32 unit = 5; }
message Label { uint32 key = 1; uint32 value = 2; }        // string ids
message CounterSample { Side side = 1; double t = 2; repeated uint32 ids = 3; repeated double values = 4; }

message SlowEvent {
  Side side = 1; double t = 2; Unit unit = 3; uint64 counter = 4; float wall_ms = 5; float cpu_ms = 6;
  repeated SpanCost top = 7;               // the 8 largest spans
}
message SpanCost { uint32 name = 1; float wall_ms = 2; float cpu_ms = 3; uint32 depth = 4; }

message SpanBlock {                        // per thread, per block of block_s (default 5 s)
  Side side = 1; uint32 thread = 2; uint32 block = 3; double t0 = 4; float block_s = 5; uint32 units = 6;
  repeated uint32 span = 7; repeated uint32 parent = 8;
  repeated float wall_ms = 9; repeated float cpu_ms = 10; repeated uint32 seen = 11;
}

message TimelineChunk {                    // columnar events of one thread over about one second
  Side side = 1; uint32 thread = 2;
  repeated uint32 name = 3; repeated uint64 start_us = 4; repeated uint32 dur_us = 5; repeated uint32 depth = 6;
}

message MemorySnapshot { Side side = 1; double t = 2; uint32 label = 3; repeated uint32 path = 4; repeated double value_mb = 5; }
message Mark { Side side = 1; double t = 2; string name = 3; }
message Warning { Side side = 1; double t = 2; WarningCode code = 3; repeated double args = 4; }

message SpanSummary {
  Side side = 1; uint32 thread = 2; Unit unit = 3; uint64 count = 4;
  repeated SpanTotal spans = 5;            // name, parent, depth, wall_ms, cpu_ms, seen
  Histogram histogram = 6;                 // edges_ms, counts
  repeated WorstUnit worst = 7;            // the 16 worst ticks or frames with their full span trees
}

message Workers { Side side = 1; repeated WorkerKind kinds = 2; }  // kind, jobs, cpu_cores, ms_per_job

message SampleProfile {
  Side side = 1;
  repeated SampleKind kinds = 2;           // cpu (ms), wall (ms), alloc (bytes), with period
  repeated SampledThread threads = 3;      // name, units, slow_units, unit
  float block_s = 4;
  repeated BlockUnits block_units = 5;     // ticks or frames per block, per thread
  Functions functions = 6;                 // columnar, deduplicated: name, file, line, crate, addr
  Nodes nodes = 7;                         // columnar: parent, function, thread_root
  Contexts contexts = 8;                   // columnar: span, entity, block, dim (string ids), chunk_x, chunk_z (sint32)
  Weights weights = 9;                     // columnar: node, thread, kind, state, slow, context, block, count
}

message GpuPasses { Side side = 1; GpuDevice device = 2; repeated GpuPass passes = 3; repeated GpuCounter counters = 4; GpuSource source = 5; }
message GpuFrames { Side side = 1; repeated GpuFrame frames = 2; repeated float cpu_frame_ms = 3; }
message GpuFrame { GpuSource source = 1; double t = 2; optional float total_ms = 3; repeated GpuPass passes = 4; }
message NetTraffic { Side side = 1; float block_s = 2; repeated PlayerTraffic players = 3; }
message WorldSnapshot { Side side = 1; double t = 2; uint32 label = 3; repeated DimensionCounts dims = 4; repeated EntityCost entity_costs = 5; }
message BenchResult { Side side = 1; string format = 2; string report_json = 3; string corpus_id = 4; }
message Attachment { string name = 1; string media_type = 2; string body = 3; }

message Stop {
  Side side = 1; double t = 2; StopReason reason = 3;  // NORMAL, WINDOW_CAP, DISCONNECTED, SHUTDOWN
  Overhead overhead = 4;                   // fields listed below the sketch
}
```

Notes on fields:

- `Stop.overhead`: `spans_ms_per_tick`, `spans_share`, `sampler_share_of_thread`, `writer_core_share`,
  `dropped_samples`, `dropped_messages`, `timeline_stopped_t`.
- `Start.settings`, `game_rules` and `horizon_options` are typed allowlists, never a dump of a file or of the
  environment (§4).
- `Start.build.symbols` tells the summary why inlined frames merged.
- Clients fill the `frame_*` fields and `fps`; servers fill `tps` and `mspt_*`. `cpu_*_cores` are in cores, not
  percent. There are no GC counts, because there is no GC: `allocs_per_s` and `allocator_commit_mb` take their place in
  the health bar.
- `SampleProfile.weights.state` is 0 for a CPU sample, 1 for wall running, 2 for wall waiting (as
  `perf/sampler-viewer:…/profile/agg.rs:17-37`).
- `SampleProfile` as built (S2): `nodes` has no row for the thread or the wall-state frames, because `weights.thread` and
  `weights.state` carry them; a root node has `thread_root = 1 + thread index`. An inlined call is a chain of nodes, and
  `weights.node` is the innermost. `slow_over_ms` is the duration from which a tick counts as slow; read it there, because
  `Start.options.ticks_over_ms` is left unset. Nodes are unique per (parent, function): addresses of one function under one
  caller share a node. Time blocks are `block_s` long (1 s, at most 1800 of them: 30 minutes); the last block takes whatever follows it. A time range in a query selects every block it overlaps and the reply's `selection.from`/`to` say the blocks it used.
- Q2 as built (sample queries read `samples`; `meta`'s `Sections.samples` counts it):
  - Functions are merged by normalized name (`horizon_perf::names`), so every monomorphization and address of one function is one row; ids are `f:` over that name, `p:` over the thread and the normalized frames.
  - Weights are `count × kind.period`; "per tick" divides by the thread's units in the selection (`block_units` over the window's blocks; slow units are scaled by the window's share, with a warning).
  - A function's inclusive time counts a recursion once (the outermost call). Percentages are shares of the selection's weight.
  - `callers` and `callees` rows are merged over occurrences and carry `f:` ids; `tree` rows carry `p:` ids where one call path explains them, else `f:`. `--depth` counts levels of children (`tree`) or of rows (`callers`, `callees`).
  - A chunk of (0, 0) in `contexts` means none was set. `split --by state` divides the selected kind, so use `--select wall` to see running against waiting. `group --by thread` ignores `--thread`.
  - `--where chunk=` takes `dimension/x,z` (a comma inside a chunk value continues it).
- `functions` is deduplicated, unlike the viewer blob's per-node names. This is what makes function ids (§5.2) and
  bottom-up views cheap.
- `addr` is the module-relative address, kept so that a later split-debug-file design can symbolize offline (the
  H 4b note). With `symbols = NONE`, names are `crate+0xOFF`.
- `block` is the time block. It multiplies keys by at most the blocks the session spans (one a second, up to 1800), and aggregation stays bounded by `MAX_KEYS = 1<<18`
  (`agg.rs:8-10`). When the table is full, further samples are dropped and counted in `dropped_samples`.
- `SpanBlock` replaces `perf-*.json`'s per-tick spans for statistics (§6.6). `SpanSummary.worst` keeps 16 ticks with
  full trees; today there are 10 ticks with 8 spans each (`report.rs:25`, `:159-171`).
- `NetTraffic.players` identifies players by join-order index (`p1`, `p2`, …): bytes and packets in and out per block,
  plus `joined_t` and `left_t`. Today's totals are at `report.rs:217-252`. A `by_packet` field comes later, because the
  hub does not count packets by type yet.
- `WorldSnapshot` holds chunks, block entities and entities by type per dimension (today `report.rs:297-341`).
  `entity_costs` holds timed ms/tick per type and dimension, from `tick_profile.rs` (today `mob_costs`,
  `report.rs:177-194`).
- `GpuPasses` holds `PassTiming` medians, C's dispatch parts and counters, `parts_mode` and `fragment_unit` (as
  `perf/gpu-pass-detail-output:…/pack_timing.rs:852`), plus adapter, backend and driver. The per-frame GPU time is the
  `frame_gpu_ms_p50` health field.
  - **As built (G1):** one `GpuPasses` per timer (`source`: Horizon's own passes or the shader pack's), written at stop
    from the medians over each timer's last 15 timed frames. A pass has `ms` (unset when only its parts or counters
    were seen), its `parts`, and `GpuCounter`s with the totals, the target's `pixels` and the segments. The device
    carries `fragment_unit` and `fragments_per_pixel` is derived when reading (`query gpu`), so the unit travels
    with the counts. `--gpu` without a device that times passes writes `WARNING_CODE_GPU_UNAVAILABLE` and no field.
    `Options.gpu` records `--gpu` as asked. `HORIZON_SHADER_PACK_TIMING` and `HORIZON_GPU_PASS_DETAIL` start a capture
    (`--gpu`, `--gpu=detail`) at launch; the renderer no longer reads them.
  - **Per frame:** every frame a timer times during a capture goes into `gpu_frames` (`GpuFrame`: its start on the
    capture clock, `total_ms` from its first timestamp to its last, each pass and part with a name's occurrences
    summed), about one block a second, with the CPU ms of every frame the window drew (`cpu_frame_ms`). `GpuPasses`
    `ms` is then the median over the whole capture. Where the device has timestamps inside passes
    (`GpuDevice.pass_split` = `DRAW_GROUPS`), the world passes are split into their draw groups (sky, solid terrain,
    entities, translucent terrain, clouds, weather, hand...) and compute passes into their dispatches in every timed
    frame; Metal keeps pass boundaries (`PASSES_ONLY`). `query gpu` adds each pass's and part's samples, p50, p95,
    p99, max and share of the GPU frame p50, and a summary row with the GPU and CPU frame percentiles and a `bound`
    verdict; `query gpu_frames` lists the frames, flagging those inside a slow frame.
- `Mark.name` is the only free text in a capture. It is typed by the person profiling (`profile mark NAME`).
  `Warning`s are codes with numbers, so tools render them in their own language.
- `BenchResult.report_json` embeds a `horizon-benchmark-report` v1 verbatim (or `scene_replay --json`, with
  `corpus_id`).
- The span, world, net, memory and workers messages are spelled out in `capture.proto` (piece Q1). Their conventions:
  - String id 0 means none: a root span's `parent`, or an unset counter unit.
  - `SpanBlock.span[i]` ran under `parent[i]`; a span that runs under several parents is one row per parent, and tools
    merge them by name.
  - `SpanSummary` adds `wall_ms_total` and `cpu_ms_total`, the units' own time, so `%MSPT` has a denominator.
  - `Histogram.counts[i]` holds the units with a wall time in `[edges_ms[i-1], edges_ms[i])`, the first from 0, and the
    last bucket is open, so `counts` has one more entry than `edges_ms`.
  - `WorstUnit.spans` is the whole span tree, parents before children, with `depth`.
  - `PlayerTraffic` arrays start at the capture's start; a block before the player joined is 0.
  - `MemorySnapshot.value_mb` holds MB when the path's last key (before any `[name]`) ends in `_mb`, otherwise a count.
- What the dedicated server fills (S1b):
  - `span_blocks` and `spans` are for the thread `Server thread`, in blocks of 5 s from the capture's start. A tick's
    spans are merged by (name, parent name); `Histogram.edges_ms` is fixed in the server (`server_perf/spans.rs`).
  - `net` counts the wire bytes of each connection once a second and leaves `packets_in` and `packets_out` empty,
    because the hub does not count packets yet.
  - `world` has the labels `start` and `stop`; `entity_costs` and `workers` come at the stop. Loose entities are
    counted as `minecraft:item`, `minecraft:experience_orb`, `minecraft:minecart` and `minecraft:boat`, and all
    projectiles together as `horizon:projectile`.
  - `Start.game_rules` and `Start.horizon_options` are the allowlists in `capture.proto`; a field is added there
    when a switch starts to matter for what a tick costs.
  - `TickDetail.spans` holds the tick's own spans only; the spans recorded between ticks (the chunk pump) are left out
    of `span_blocks`, `spans` and a slow tick's `top`, so span sums stay consistent with the tick's wall time.
  - `HorizonOptions.max_view_distance` is 0 when the far ring was not created, even if `server.toml` sets a distance.
  - `Overhead.mob_timing_ms_per_tick` and `mob_timing_share` price the mob tick sampling next to `spans_*`.
- `horizon-perf query` rounds numbers to 3 decimals. `spans --slow` averages over the slow ticks; `ticks` and `tick`
  prefer a unit's whole tree from `spans.worst` over the 8 largest spans of `slow` and `slow_history`.

### 3.6 Health recorder and counter registry

- **Always on, in memory, on servers and clients.** The tiers are fixed: 1 s × 600 (10 min), 10 s × 360 (1 h),
  60 s × 1440 (24 h) and 15 min × 672 (7 days). With ~25 fields of f32 this is about 300 KB, constant however long the
  game runs. A capture copies the tiers into `history` at start, so even a 60 s capture shows TPS over 1, 5 and
  15 minutes and last night. After that, `health` gets one sample per second.
- **No health file.** Nothing is written to disk unless a capture runs. If a rolling health file is ever added, it is
  off by default.
- **The slow ring** keeps the last 256 ticks or frames over the `HORIZON_SPIKES` threshold (dedicated default 250 ms,
  `dedicated.rs:673-676`), each with its top 8 spans. Captures copy it into `slow_history` at start and append new ones
  to `slow`.
- **Counters:** `horizon_perf_format::counter("streaming.cap", &[("kind", "light")])` returns a handle; `add` and
  `set` are relaxed atomics. Names are dotted. Labels are part of a counter's identity from day one (`CounterDef.labels`),
  so a Prometheus exporter is one small PR later: it would map `CounterDef` straight onto metric names and labels.
  Counters are the one place where ad-hoc instrumentation goes, instead of new log lines like the `s3:` lines.
- **Memory:** `horizon-ctl memory`'s reply (`game/memory.rs:153-170`), flattened to paths, at start, every 10 s and
  at stop. Spec D's `flatten`/`diff` logic becomes `horizon-perf query memory --diff`.

### 3.7 Files and lifecycle

- **Where:** a dedicated server writes to `reports/` in its folder, as documented today (`docs/hosting.md:358-368`).
  A client writes to `<game home>/reports/`, beside `logs/` and `crash-reports/`. The game home is `--home`, else
  `HORIZON_HOME`, else the platform data folder (`crates/horizon-client-render/src/logging.rs:111-121`). Today the
  sampler branch writes into the working directory
  (`perf/sampler-viewer:crates/horizon-client/src/game/profile_control.rs:10-12`). Moving to the game home also keeps
  agent runs isolated by their scratch `HORIZON_HOME`. `HORIZON_PROFILE=DIR` overrides the folder.
- **Names:** `profile-<YYYYMMDD-HHMMSS>Z.hzperf` (UTC).
- **On finish:** an in-game `/profiler` capture shows its local path, uploads in the background, and displays a clickable capture URL through `horizon.perf.shared`. Upload failure keeps the file and offers `/profiler share` to retry. Control-socket, environment and dedicated-console captures print the local path and viewer homepage; `horizon-perf share FILE` uploads those explicitly.
- **Retention:** after writing, the game deletes the oldest `profile-*.hzperf` beyond 30 files or 1 GiB in that folder,
  and logs what it deleted. Files that don't match the pattern, for example files the user renamed, are never touched.
  Old `perf-*.json` and `.txt` files are left alone.
- **Caps:** the window is at most 600 s, as today (`report.rs:23`). The timeline stops after 60 s or 256 MiB of
  chunks, whichever comes first, and records `Stop.overhead.timeline_stopped_t` and a warning.

### 3.8 Who writes what

| Field | Dedicated `horizon-report` | Client `profile client` / `/profiler` | Single player, both sides | Remote server via `profile server` | `--benchmark --profile` |
|---|---|---|---|---|---|
| start, history, slow, health, counters, marks, warnings, stop | server | client | both | server (on the server and in the client's file) | client |
| span_blocks, spans | server | client | both | server | client |
| samples | by default where the sampler exists (Linux today); `--no-samples` | by default | both | by default | with `--profile` |
| gpu | — | with `--gpu` (on when F3 pass timing is on) | client | — | yes |
| net | yes | its own bytes | server | server | — |
| world, workers | yes | integrated server only | server | server | — |
| timeline | `horizon-trace` | `--timeline`, `trace` | both | `--timeline` | — |
| bench | — | — | — | — | yes |

`horizon-report` samples by default. Its measured cost is about 0.05% of the tick thread plus 0.13% of a core for the
writer (measured during the sampler work). `--no-samples` turns it off.

## 4. Capturing, permissions and privacy

### 4.1 Triggers (in the game; they only start and stop the writer)

```
horizon-ctl profile [server|client] start [--seconds S] [--hz N] [--over MS] [--threads main|all]
                                          [--wall] [--gpu[=detail]] [--timeline] [--no-samples]
horizon-ctl profile [server|client] stop|status
horizon-ctl profile mark NAME
```

- **No side given:** both sides, as separate sections (`side`) of one capture. In single player the server thread is
  the window's own `serving` Game, so it needs no permission (`game/integrated.rs`). On a remote server, §4.2 applies;
  if that is refused, the capture holds the client side and a warning says why.
- **`client`:** the window only. **`server`:** the server only. In single player that is the integrated server
  thread; when connected to a server, it is that server, through §4.2.
- **`stop`** replies with the written path and the web UI's address. **`status`** gives the running sides, elapsed
  time and bytes written.
- **`/profiler start|stop [client|server]`** in chat runs `profile start` / `profile stop`. **`HORIZON_PROFILE=DIR`** captures the whole run and
  writes it into DIR at exit.
- **As built (C2):** in single player a bare `profile` writes both sides into the window's file. The server thread
  (`game/integrated/capture.rs`) is handed the file's writer, a clone of its sender, and the capture's start as an
  instant, so both sides' `t` count from the same zero; it writes `Start` (kind `INTEGRATED`), its ticks, spans,
  mob costs and `tick` thread samples, and its own `Stop`, and the window finishes the file once both are in. A bare
  `profile start` before a world opens, `HORIZON_PROFILE` and `/profiler` are joined by the server when it comes up (once per
  capture). `profile server start` makes a file of the server thread alone (on a server joined over the network, see R1a).
  `stop` ends the capture with every side it holds (a `--side` it does not hold is an error), `status` lists
  the sides that run, and `mark` goes to every running side or the one `--side` names. Replies list `sides`, server first; on `start` they are the sides asked for, and `status` and `stop` confirm which wrote. A `mark` reply's `t` is the window's game clock for every side, while the file stores it from the capture's start. With `--threads all`
  the window's sampler claims every opted-in thread, the server thread included, so the server side has no samples of its own.
- **As built (R1a):** `crates/horizon-client/src/perf_net.rs` is the wire (a `Request` of `Start` or `Stop`, a `Reply` of
  `Started`, `Finished` or `Refused`, and the client's `Link` state) and `game/perf_remote.rs` the server's half. A request
  is checked where it arrives: the guest must be a Horizon client (brand `horizon`) and at operator level 4, else
  `Denied`; the guest's switches are held to `profile start`'s limits on decode and never carry a file or a GPU switch. A
  `Start` is carried out between two ticks by the server thread (`Target::Own`, trigger `REMOTE`) or by a dedicated
  server's tick loop (`serve_perf_requests`, which maps it to the `horizon-report` capture); a second one while any
  capture runs, the console's or `profile server` included, gets `AlreadyRunning`. Only the guest that started a
  capture may stop it (`NotRunning` otherwise). The server answers `Started` with its `reports/` file and `Finished` once
  the file is whole, whichever way the capture ended (stop, window or shutdown), and logs `Horizon: NAME started a server
  profile (...)` and `Horizon: NAME's server profile saved to PATH`. The client treats a server as Horizon when its brand
  is `horizon`: `horizon:block_registry` is only sent when content mods are loaded, so it would miss ordinary Horizon
  servers. `profile server start|stop` on such a server waits for those answers (10 s for the first); a bare `profile start`
  also sends `Start` and a refusal becomes the warning `SERVER_NOT_HORIZON`, `SERVER_DENIED` or `SERVER_ALREADY_RUNNING` in
  the client's file, and `Started` leaves the mark `server_started`. The server's file follows (R1b).
  For tests only, `horizon-ctl lan op NAME LEVEL` sets a connected guest's operator level on a LAN host.
- **As built (R1b):** `Finished` carries the size of the server's file, and that many bytes follow as `Chunk` replies of at most
  32 KiB. The server sends a chunk only when one is due, at least 125 ms after the last (never above 256 KiB in any second), and
  only while the guest has at most 64 KiB queued, so gameplay packets go first. The client feeds the chunks to
  `horizon_perf_format::PartMerger`, which inflates the stream and yields whole top-level fields without `format`, unknown fields
  included. A window capture that asked for the server (a bare `profile start`) keeps its writer open from `Started` on and appends
  those fields to its own stream, so its file finishes once the part has arrived. `profile server stop` alone saves the server's gzip
  bytes unchanged as a file of its own in the client's `reports/` (with retention) and replies with that path. A lost connection, a
  bad stream, an error from the server or 30 s without progress ends the transfer: the window's file keeps the whole fields received
  and gets `SERVER_PART_INCOMPLETE` after the client's `Stop` (the cut is only known then), plus a server `Stop` with reason
  `DISCONNECTED` when the part had begun without one, so the whole-file reader still opens it; a file of its own is removed.
- **As built (C1):** `profile client` captures the window only. A client capture without `--seconds` stops at the 600 s window cap with
  `STOP_REASON_WINDOW_CAP`, so `HORIZON_PROFILE` records the first 10 minutes of a run. The client's slow ring takes
  frames over `HORIZON_SPIKES`, else 50 ms. The health gauges (process and machine probes) are read on a thread of their
  own so a frame never waits on `ps`.
- **As built (T1):** `--timeline` on `profile start` and `horizon-report`, `horizon-ctl trace FILE [SECONDS]`,
  `HORIZON_TRACE=FILE` (client once the first hardware probe has finished, server from its first tick) and `horizon-trace [SECONDS]` start a
  timeline-only capture; with FILE it is written exactly there and retention leaves the folder alone. One `timeline`
  chunk per thread per second, `start_us` since the capture began, `thread` a string id of the thread name (threads
  alive together under one name get `#2`, `#3`). The process has one timeline: a second capture asking for it
  records without. A thread keeps at most 262144 spans between two drains (one second); a thread busier than that drops
  the rest silently, so a very dense second can be incomplete. A cap warning carries `[seconds recorded, bytes written]`.
- **Dedicated console:** `horizon-report [SECONDS|stop|status] [--no-samples]` and `horizon-server report` capture the
  server side.
- The `profile` row in `horizon-ctl` stays a trigger; it has no reading subcommands.

### 4.2 Profiling a dedicated or remote server from a client

1. **Is it Horizon?** A Horizon server sends `horizon:block_registry` during configuration
   (`crates/horizon-client/src/lan/conn.rs:1331`), and the client records it (`net.rs:1121`). Without it,
   `profile server` fails with `this server isn't running Horizon` (translation key `horizon.perf.not_horizon`), and
   `profile` with no side captures the client only, with that warning.
2. **Request.** The client sends `Start` options on a Horizon-only custom payload channel, `horizon:perf`.
3. **Permission.** The server requires operator level 4, as vanilla's `/perf` does
   (`net/minecraft/server/commands/PerfCommand.java:38`, `Commands.LEVEL_OWNERS`). A LAN host applies the same rule to
   its guests. A refusal replies `Denied`. As vanilla's `commands.perf.alreadyRunning` does, only one server capture
   runs at a time; a second request gets `AlreadyRunning`.
4. **Log.** The server logs who profiled and when, at start and stop:
   `Horizon: Steve started a server profile (60 s, samples on)` and `Horizon: Steve's server profile saved to
   reports/profile-20261107-143204Z.hzperf`. The capture names each connection's player too (§4.3).
5. **Server copy.** The server writes its side as any server capture, into its own `reports/` (with retention).
6. **Transfer.** At stop, the server sends the finished file's gzip bytes to the admin's client on `horizon:perf` in
   chunks of 32 KiB, rate-limited to 256 KiB/s and lower under bandwidth pressure. Gameplay packets go first, so the
   transfer never delays them.
7. **Merge.** Every top-level field of the server's stream carries `side = SERVER`, and only `format` is singular. So
   the client merges without decoding: it inflates the server's stream and appends those bytes, minus the leading
   `format` field, to its own deflate stream before closing. The client's file stays one gzip stream. For
   `profile server` alone, the client saves the server's bytes unchanged.
8. **Alignment.** The client records a `server_started` mark when the server acknowledges the start, which aligns the
   two sides' `t` to within one round trip.
9. **Failure.** If the client disconnects mid-transfer, its own side is closed with warning `SERVER_PART_INCOMPLETE`.
   The server's copy remains on the server.

### 4.3 Privacy: what never goes into a capture

This is the same promise as `perf-*.json` (`report.rs:8-9`) and the telemetry rules (`docs/telemetry.md:57`, `:63`),
enforced by construction. Every message is typed, and the only strings from outside our code are registry ids, mod ids
and versions, function and file base names, and `Mark.name`.

| Never | How |
|---|---|
| UUIDs, account ids, skins | `net` names each connection by player name, client brand and view distance, nothing else; `world` counts types only |
| IP addresses, server address, LAN/relay codes, MOTD | not in any message |
| World seed, world name, level folder name | not in any message; `settings` is an allowlist without `level-seed`/`level-name` |
| Entity custom names, sign/book/chat/command text | contexts hold registry ids (entity type, block type), never component data |
| User paths (home, game dir, working dir) | symbol file names are base names only (as the sampler already does, commit `bd412b6d62`); no paths in `Start` |
| Environment variables, command line, host name, MAC, serials | `horizon_options` is an allowlist of Horizon switches with boolean or numeric values |
| Block coordinates | only chunk coordinates |

Included on purpose: chunk coordinates per dimension, entity and block type names, mod ids and versions. This is spec
H's decision ("they are what a server owner needs to find the lag"). Player names are included too: a server owner has to know which connection floods or lags. The web UI's first screen and the summary's footer
say so in one line.

Test (pieces S1b and R1): a fixture dedicated server has named players, a seed, a world name, a custom-named mob, a
sign and a connected player's IP. It writes a capture; the test inflates it and asserts that none of those strings
occurs in the bytes, except the players' names, which `net` must hold.

For S1b that is two checks. `a_capture_of_a_server_with_names_text_and_a_seed_holds_none_of_them` (in `report.rs`) has no
connected player and is `#[ignore]`d: it needs `HORIZON_TEST_RESOURCES`, so run it with `--ignored`. IP addresses are
covered by `tools/server_perf_e2e.py`, which joins bots named `SteveSecret` from `127.0.0.1` to a fixture server and checks
the capture names them in `net` (build with `cargo build --profile fast -p horizon-client -p horizon-bots -p horizon-perf`, then run
`python3 tools/server_perf_e2e.py --bin-dir target/fast`). It is not in CI yet: run it by hand before merging a change to what a capture holds.

Uploads (§7.3) change nothing in the file: what a shared capture holds is exactly what the local one holds. Sharing is
an explicit action (the UI's Share button, `horizon-perf share`, or starting an in-game `/profiler` capture, whose message announces uploading);
the upload server stores the bytes as sent, keeps no account data, logs no capture contents, and drops IPs from its
logs after the rate-limit window.

## 5. The agent report: `horizon-perf summary`

### 5.1 Rules

- **Budget:** 1,500 tokens by default (≈ 6 KB, measured as bytes / 4); `--budget N` raises or lowers it. Sections are
  cut from the bottom of their lists, never mid-row. The last line always survives.
- **Order:** what an agent needs first.
  1. One header: what, where, how long, build.
  2. Health verdict.
  3. Where the tick goes (spans).
  4. Hot functions.
  5. Splits by context.
  6. Worst ticks.
  7. One line each for net, memory, GPU and world.
  8. Warnings and measured cost.
  9. `next:`, the exact drill-down commands.

  A capture with both sides gives each side its own header, health and spans, server first.
- **Units:** every column header names its unit: ms/tick on servers, ms/frame on clients. "ms/tick" from samples is
  samples × period ÷ ticks in the selection, so on a fully busy tick thread the CPU column sums to about the MSPT.
- **Ids:** every row ends in a stable id in brackets, which every `query` accepts.
- **Baseline:** with `--baseline B.hzperf`, numeric rows gain `Δ` and a verdict word from §6.6: `slower`, `faster`,
  `~` (within noise, or negligible), or `?` (too few blocks). Rows absent from one side say `new` or `gone`.

### 5.2 Stable ids

An id is `<kind>:<key>`. The same thing gets the same id in every capture whose build has the same function names. Ids
carry no side: `--side` selects it, so an id from a server-only capture works on a both-sides capture.

| Kind | Key | Example |
|---|---|---|
| `s` span | its name | `s:tick_lan` |
| `f` function | 8 lowercase base32 characters of FNV-1a 64 over the normalized name | `f:2n7xq4ka` |
| `p` call path | the same hash over the thread name and normalized frames, root to node | `p:k3m0d9tr` |
| `e` entity type, `b` block type, `d` dimension, `t` thread | registry or thread name | `e:zombie`, `b:hopper`, `d:the_nether`, `t:tick` |
| `c` chunk | `dimension/x,z` | `c:overworld/12,-4` |
| `k` tick or frame | its counter | `k:48812` |
| `g` GPU pass, `n` player, `m` memory row, `h` health field, `x` counter | name / join index / flattened path / field / name and labels | `g:terrain`, `n:p17`, `m:heap_by_tag_mb[chunk columns]`, `h:mspt_p95`, `x:streaming.cap{kind=light}` |

Normalization rules are the ones `inclusive.py` and `s1_children.py` needed:

- closures `{closure#N}` become `{closure}`;
- generic arguments are removed;
- `<X as Trait>::f` becomes `X::f`.

The short display name is the last two path segments (`Game::tick_blocks`); the full name is in the JSON. With 8
characters (40 bits), the probability of a collision among 20,000 functions is about 2×10⁻⁴. If two functions in one
capture collide anyway, the later one in table order gets `~2` appended.

### 5.3 Example: a 200-bot server

`horizon-perf summary reports/profile-20261107-143204Z.hzperf` (≈ 1,250 tokens):

```
profile-20261107-143204Z.hzperf: dedicated server, 120.0 s, 1152 ticks, Horizon 0.42.1 (1d63e0a5a3) release, Linux x86_64, 24 threads
health  TPS 9.6/20 (1m 9.5, 5m 9.8, 15m 11.2) · MSPT p50 98.7 p95 141.3 p99 188.0 max 312.4 ms · 1152 of 1152 ticks over 50 ms
        CPU 3.1 cores (system 7.9 of 24) · RSS 5.8 GB (+212 MB over the window) · 200 players · 61,842 chunks · 9,310 entities
tick phases            ms/tick  %MSPT  cpu ms  waited
  tick_lan               41.3   39.6   40.0     1.3   [s:tick_lan]
  level_overworld        38.9   37.3   38.6     0.3   [s:level_overworld]
    tick_blocks          14.1   13.5   14.0     0.1   [s:tick_blocks]
    tick_mobs             9.8    9.4    9.8     0.0   [s:tick_mobs]
    streaming             6.2    6.0    5.1     1.1   [s:streaming]
    spawn_natural         3.9    3.7    3.9     0.0   [s:spawn_natural]
  lan_waypoints          12.4   11.9   12.4     0.0   [s:lan_waypoints]
  level_the_nether        2.1    2.0    2.1     0.0   [s:level_the_nether]
hot functions, tick thread, CPU, all ticks    self ms/tick  total ms/tick
  Waypoints::tick_level                          6.10         12.2   [f:2n7xq4ka]
  Hub::track_entities                            3.42         14.8   [f:x0c41mvh]
  PalettedContainer::get                         2.95          2.95  [f:9qe2b7ws]
  ChunkEncoder::bodies_with                      2.61          5.30  [f:m4t8yh2c]
  mi_free                                        1.31          1.31  [f:t5ga8w0n]
  Sim::set_block_limit                           0.95          3.15  [f:vd3k1z8p]
  HashMap<TickTypeId,…>::get                     0.85          0.85  [f:j7r2n5qa]
  Clientbound::encode                            0.81          4.02  [f:c6w9e3fu]
by entity type (sampled ms/tick): zombie 3.1 [e:zombie] · item 2.2 [e:item] · villager 1.4 [e:villager] · skeleton 0.9 [e:skeleton]
by block type: hopper 1.2 [b:hopper] · water 0.8 [b:water] · redstone_wire 0.4 [b:redstone_wire]
by chunk: overworld/12,-4 1.9 [c:overworld/12,-4] · overworld/13,-4 1.1 [c:overworld/13,-4] · the_nether/-3,7 0.6 [c:the_nether/-3,7]
worst ticks: 312.4 ms at 71.4 s (tick_lan 180.2, streaming 61.0) [k:48812] · 288.0 ms at 15.0 s (spawn_natural 120.5) [k:47681]
net     out 38.1 MiB/s, in 1.2 MiB/s · per player out p50 180 p95 240 max 410 KiB/s [n:p17 max]
memory  heap: chunk columns 2104 MB (+96) [m:heap_by_tag_mb[chunk columns]] · lod 410 MB (+0) · entities 220 MB (+31)
workers generate 2.31 cores · light 0.88 · decorate 0.52 · mesh 0.00 (server)
cost    spans 0.61 ms/tick (0.6% of MSPT) · sampler 0.05% of tick thread · writer 0.13% of a core · 0 samples dropped
note    chunk coordinates and entity/block type names are included; no names, UUIDs, addresses, seed or world name
next    horizon-perf query callers f:2n7xq4ka · horizon-perf query split s:tick_lan --by thread · horizon-perf query ticks --worst 5
```

With `--baseline reports/profile-20261107-101500Z.hzperf`, the phase table becomes:

```
tick phases            ms/tick    Δ ms   verdict       [id]
  tick_lan               41.3    -12.8   faster        [s:tick_lan]
  level_overworld        38.9     +0.4   ~             [s:level_overworld]
  lan_waypoints          12.4    -14.9   faster        [s:lan_waypoints]
```

The header gains `baseline: profile-20261107-101500Z.hzperf, same build kind, 24 vs 24 blocks of 5 s`.

## 6. Queries: `horizon-perf` subcommands

### 6.1 Command grammar

```
horizon-perf summary [FILE|latest] [--baseline FILE] [--budget TOKENS] [--side S] [--thread T] [--slow|--all] [--json]
horizon-perf query   OP [ARGS] [FILE...] [FILTERS]                     JSON always
horizon-perf diff    A B [--min-ms X] [--json]
horizon-perf list    [DIR]                                             one line per capture
horizon-perf follow  [FILE|latest] [--every S] [--json]                health and slow events as they are written
horizon-perf export  [FILE|latest] --pprof|--folded|--perfetto [--out PATH] [--side S] [--thread T] [--select K] [--slow]
horizon-perf attach  FILE --name NAME --body JSONFILE                  adds an Attachment at the wire level
horizon-perf mcp                                                       §8
```

`FILE` defaults to `latest`: the newest finished `.hzperf` in `--dir DIR`, else in `$HORIZON_HOME/reports`, else in
`./reports`. A bare `benchmarks/*.json` is accepted wherever a capture is.

### 6.2 Operations

| OP | Arguments | Answers | Replaces |
|---|---|---|---|
| `hot` | `--by self\|total`, `--match TEXT` | functions ranked | `inclusive.py`, `h5_perf_top.py` |
| `tree` | `ID` (`p:` or `f:`; merged over its occurrences), `--depth N`, `--min-pct X` | top-down subtree | the viewer's tree |
| `callers` | `ID`, `--depth N` | bottom-up callers of a function, merged across the tree | the viewer's bottom-up view |
| `callees` | `ID`, `--depth N` | children of a function, merged | `s1_children.py` |
| `split` | `ID`, `--by span\|entity\|block\|chunk\|dim\|thread\|state\|slow` | that node's or span's weight split by one context | the viewer's detail panel |
| `group` | `--by span\|entity\|block\|chunk\|dim\|thread` | total sampled weight per context value | group tables, mob costs |
| `frames` | `NAME…` (id, exact short name or `/regex/`) | inclusive and self per name; one column per capture | `s1_breakdown.py`, `s1_table.py`, `s3_table.py` |
| `spans` | `--names a,b`, `--slow` | per span: ms/tick, cpu, waited, seen; one column per capture | `slowticks.py`, `spikes.py`, `s1a_spans.py` |
| `ticks` | `--worst N`, `--over MS` | worst ticks/frames with span trees, from `spans.worst`, `slow_history` and `slow` | worst-ticks lines |
| `tick` | `k:ID` | one tick's span tree, and its samples if the tick was slow | |
| `series` | `FIELD…` (`h:` fields, `span:NAME`, `x:` counters), `--every S` | time series, resampled; `--from` before 0 reaches into `history`; the web UI and MCP can add `stats` (min, mean over the window, median, p95, max, last) to each series | `s3_windows.py`, `s3_counters.py` |
| `world`, `net`, `gpu`, `gpu_frames`, `meta`, `bench`, `marks`, `warnings` | section filters | the section, filtered and ranked | `s3a_table.py`, `s1b_table.py` |
| `memory` | `--diff [LABEL_A LABEL_B]`, `--min-mb X` | a snapshot, or spec D's diff of two (`grew_mb`, `shrank_mb`, `counts`, `only_in_a/b`) | spec D's `memory-diff` |

Filters, common to the sample-based ops:

| Filter | Meaning |
|---|---|
| `--side client\|server` | default `server` when the capture has one, else `client` |
| `--thread T` | thread name; default the side's main thread (`tick` on servers, `render` on clients) |
| `--select cpu\|running\|waiting\|wall\|alloc\|live` | sample kind and state; default `cpu` |
| `--slow` / `--all` | only samples of ticks over the threshold, or every tick (default: all) |
| `--where k=v[,k=v]` | AND of `span`, `entity`, `block`, `chunk`, `dim`; a repeated key is OR (`entity=zombie,entity=husk`) |
| `--from T --to T` | seconds; negative counts from the end (`-60`); `mark:NAME` |
| `--limit N` | rows (default 25) |

An ambiguous name returns an error that lists the candidates with their ids, so an agent can pick one without a second
search.

### 6.3 JSON shapes

These are query replies, not a capture export. Every reply has one envelope:

```json
{"ok": true, "op": "hot",
 "captures": [{"file": "profile-20261107-143204Z.hzperf", "sides": ["server"], "kind": "dedicated", "seconds": 120.0}],
 "selection": {"side": "server", "thread": "tick", "select": "cpu", "slow": false, "where": {}, "from": 0.0, "to": 120.0,
               "ticks": 1152, "samples": 27007},
 "unit": "ms_per_tick", "rows": [], "warnings": []}
```

Row shapes:

| OP | Row |
|---|---|
| `hot` | `{"id": "f:2n7xq4ka", "name": "Waypoints::tick_level", "full": "horizon_client_lib::lan::waypoints::Waypoints::tick_level", "file": "waypoints.rs", "line": 211, "self": 6.10, "total": 12.2, "self_pct": 5.9, "total_pct": 11.7, "samples": 3513}` |
| `tree`, `callers`, `callees` | `{"id": "p:…", "function": "f:…", "name": "…", "total": 1.2, "self": 0.1, "pct": 1.1, "samples": 690, "children": [ … ], "folded": {"rows": 17, "total": 0.08}}` |
| `split`, `group` | `{"by": "entity", "key": "e:zombie", "label": "zombie", "value": 3.1, "pct": 21.0, "samples": 1786}` |
| `frames`, `spans` | `{"id": "s:tick_lan", "name": "tick_lan", "values": [41.3, 54.1], "cpu": [40.0, 52.2], "waited": [1.3, 1.9]}`, one value per capture |
| `ticks` | `{"id": "k:48812", "t": 71.35, "wall_ms": 312.4, "cpu_ms": 290.1, "spans": [["tick", 0, 312.4, 290.1]]}` |
| `series` | `{"field": "h:mspt_p95", "step_s": 30, "t": [0, 30, 60, 90], "values": [130.2, 141.0, 152.3, 140.8]}` |
| `diff` rows | `{"id": "s:tick_lan", "a": 54.1, "b": 41.3, "diff": -12.8, "diff_pct": -23.7, "ci95": [-14.0, -11.6], "confidence": 0.95, "blocks": [24, 24], "verdict": "faster"}` |

Numbers are computed unrounded and printed rounded to 3 significant decimals, as spec A rounds.

### 6.4 Exports

- `--pprof`: the hand-encoded writer from `perf/sampler-viewer:crates/horizon-threads/src/profile/pprof.rs`, moved to
  `horizon_perf::export`. It carries labels `span`, `entity`, `block`, `chunk`, `dim`, `slow` and `state`, and one
  sample type per kind.
  Samples also carry `side` and `thread` labels, and both sides share one function table.
- All three: `horizon-perf export [FILE|latest] --pprof|--folded|--perfetto [--out PATH] [--side] [--thread] [--select] [--slow]`;
  without `--out` the bytes go to stdout.
- `--folded`: one line per stack (`side;thread;frame;...;frame count`), for `inferno` and old scripts; it counts `--select` (cpu by default), and waiting wall samples end in `[waiting]`.
- `--perfetto`: the Chrome trace JSON of `spans.rs:303-454` (the same `{"ph":"X"}` events and thread-name metadata),
  built from the `timeline` chunks. Both sides become separate processes in the trace.
- The web UI offers the same three, through the same wasm code.

### 6.5 Agent session, worked

Goal: the 200-bot server runs at TPS 9.6. Find what to fix, then check the fix.

1. `horizon-perf summary` gives the report in §5.3. `tick_lan` is 40% of the tick and `lan_waypoints` takes
   12.4 ms/tick; `Waypoints::tick_level` has the most self time.
2. `horizon-perf query callers f:2n7xq4ka --depth 3`:
   ```json
   {"ok": true, "op": "callers", "unit": "ms_per_tick", "rows": [
     {"id": "p:r8d2kq0m", "name": "Game::lan_update_waypoints", "total": 12.2, "pct": 11.7, "samples": 3513,
      "children": [{"id": "p:b1n6zx4e", "name": "Game::tick_lan", "total": 12.2, "pct": 11.7, "samples": 3513,
                    "children": [{"id": "p:h0w3vy7s", "name": "Game::tick_server", "total": 12.2, "samples": 3513}]}]}]}
   ```
3. `horizon-perf query split f:2n7xq4ka --by dim` gives `overworld 11.9`, `the_nether 0.3`. Then
   `horizon-perf query callees f:2n7xq4ka --depth 1` gives `Waypoints::update_or_create 7.4`, `waypoint_team 2.9`,
   `(self) 6.1`. The cost is per player pair, not per dimension.
4. `horizon-perf query series h:players span:lan_waypoints --every 30` shows `lan_waypoints` growing faster than the
   player count: `[3.1, 6.0, 9.8, 12.4]` ms/tick for `[50, 100, 150, 200]` players. That is quadratic, which matches
   spec S1.
5. After the fix, take a new capture under the same load, then run
   `horizon-perf diff reports/profile-20261107-101500Z.hzperf latest --min-ms 0.5`:
   ```
   A profile-20261107-101500Z.hzperf  B profile-20261107-143204Z.hzperf  (24 vs 24 blocks of 5 s, server tick thread, CPU)
   MSPT p50         124.0 → 98.7    -25.3 ms   CI [-28.1, -22.0]  faster
   s:lan_waypoints   27.3 → 12.4    -14.9 ms   CI [-15.6, -14.1]  faster
   s:tick_lan        54.1 → 41.3    -12.8 ms   CI [-14.0, -11.6]  faster
   s:level_overworld 38.5 → 38.9     +0.4 ms   CI [-0.6, +1.3]    ~ within noise
   f:2n7xq4ka        19.8 → 6.1     -13.7 ms   CI [-14.4, -13.0]  faster
   warning: A's MSPT drifts upward over its window (Theil–Sen +0.21 ms/s): bots were still joining
   ```

While the capture in step 5 runs, `horizon-perf follow latest` prints one health line per second, so an agent sees
the load settle before stopping it.

### 6.6 Statistics (`horizon_perf::stats`)

- **Two captures** (two runs, not paired): each capture's window is cut into time blocks of `block_s` (default 5 s;
  at most 32 blocks).
  - **Block values.** Per block, a metric's value is its ms/tick in that block. Samples use the `block` column of
    `weights`; spans use `span_blocks`; health uses the mean of the block's `health` samples.
  - **Effect.** The effect is the two-sample Hodges–Lehmann shift: the median of all `b_j − a_i`.
  - **Confidence interval.** The 95% CI is the exact Mann–Whitney interval for up to 32 + 32 blocks, computed by
    dynamic programming over the U distribution.
  - **Why blocks and medians.** Blocks, not ticks, because neighbouring ticks are correlated. Medians, because tick
    times are heavy-tailed.
- **Verdicts** (spec A's words):
  - `too_few_blocks`: fewer than 6 blocks on a side;
  - `within_noise`: the CI contains 0;
  - `negligible`: |effect| < max(0.05 ms, 2% of A);
  - `slower` or `faster` otherwise.
- **Drift warning:** the Theil–Sen slope of each side's per-block MSPT. If its CI excludes 0 and the slope × window
  exceeds 5% of the median, the result says the window was not steady.
- **Paired designs** (spec A's B A A B blocks in one run): the game computes `Report.ab` with `horizon-bench`'s
  Hodges–Lehmann and exact Wilcoxon code, which must ship for the launcher. `horizon-perf diff` on two benchmark
  reports prints `ab` when present and never recomputes it. Without `ab`, each `Frames` repeat is a block for the
  two-capture statistic.
- Spec B's nightly regression rule (permutation test, MAD history) stays in its own tool. It is a CI policy over a
  history, not a question about two captures.

## 7. The web UI (`sunrise-org/horizon-site`)

### 7.1 What it is and where it lives

- **Location and stack.** The browser UI and upload API live in [sunrise-org/horizon-site](https://github.com/sunrise-org/horizon-site), a fork of the Next.js App Router website. The UI has its own root layout under `app/perf/`; helpers live under `lib/perf/`, translations under `messages/perf/`, and static assets under `public/perf/`. The routes are `/perf`, `/perf/c/<id>` and `/api/perf/captures`. It runs on the umceko cluster in namespace `sunrise`; configuration stays in environment variables.
- **The wasm does the computing.** The page loads `horizon_perf.wasm`, the `horizon-perf` library built for
  `wasm32-unknown-unknown` with feature `wasm`, for reading, queries and statistics. The UI only draws, and every
  number it shows comes from a wasm call. So the web UI and the CLI cannot disagree, and no second implementation
  needs shared test vectors (as `bench_stats.py` did for `compare.rs`).
- **The wasm interface** is a C ABI with JSON in and out. It has no `wasm-bindgen` dependency.

  | Export | Does |
  |---|---|
  | `hp_alloc(len) -> ptr`, `hp_free(ptr, len)` | memory for the UI to pass bytes in |
  | `hp_open(ptr, len) -> handle` | decodes a file; returns an error JSON on failure |
  | `hp_query(handle, req_ptr, req_len) -> reply` | the request is the same op and filters as `horizon-perf query`, as JSON; the reply is §6.3's envelope |
  | `hp_summary(handle, req) -> text` | the §5 summary |
  | `hp_diff(a, b, req) -> reply` | the §6.6 statistics |
  | `hp_export(handle, kind) -> bytes` | pprof, folded or Perfetto |
  | `hp_close(handle)` | frees a decoded capture |

  A ~60-line `horizon_perf.js` shipped beside the wasm wraps these, so the UI does not manage wasm memory itself.
  The wasm returns data, ids and warning codes, never English sentences except `summary`. UI strings live in the
  UI's own translation files (`messages/perf/` in the site repository, translation-ready from the start).
- **The build.** `cargo xtask perf-wasm` writes `target/perf-wasm/{horizon_perf.wasm,horizon_perf.js}`. The `perf-wasm` workflow tests the ABI and uploads that bundle. The site imports the matching reader and wrapper with `npm run perf:import -- HORIZON_SOURCE_DIR PERF_WASM_DIR`, records their source revision and checksums, and checks them before every build. Normal site builds need Node only.
- **Local first, upload by choice.** The page opens a `.hzperf` from a file picker or drag and drop and reads it in
  the browser; nothing leaves the machine unless the user presses **Share**, which uploads and pins it through §7.3
  after a one-line notice of what the file holds (§4.3) and returns a link `…/perf/c/<id>`. Opening a link fetches the capture
  from the same API. Compare mode opens two files or two links. The page's CSP allows `connect-src 'self'` only.
- **No HTML from the game.** There is no `profile-*.html`, and no per-report JS or CSS.
### 7.2 Features

These are spark-style features, plus what `perf/sampler-viewer` already had.

| Feature | Data |
|---|---|
| Health bar: TPS 1/5/15 min, MSPT min/median/p95/max, CPU process and system, memory process/physical/swap, disk, allocator counts (in place of GC) | `history`, `health`, `Start` |
| Metadata panel: platform, OS, CPU, uptime, capture duration; tabs for memory, network, flags (build features and Horizon switches), config (`settings`), world, game rules, mods; one tab per side | `Start`, `world`, `net`, `memory` |
| Call tree "all": ms per tick, %, self %, samples; single-child chains collapsed (`a > b > c`, expandable); siblings under 0.05% folded into "N smaller"; lazy rendering of expanded rows only | `tree` (as `perf/sampler-viewer:…/viewer.js:62-117`) |
| Colour by crate: `horizon_world`, `horizon_client*`, `horizon_*`, std/core/alloc, third-party, native mods | `functions.crate` |
| Group by none/thread/span/entity/block/chunk/dimension; selection: all ticks, slow ticks only, wall running, wall waiting, allocations, live bytes; side and thread pickers | `group`, filters |
| Detail panel: the selected node's split by every context (top 10 each) and its merged callers | `split`, `callers` |
| Search (highlight, auto-open), focus a node, collapse all, auto-open the hottest path | `tree`, `hot` |
| Flat view: top 250 functions, top-down or bottom-up, sorted by total or self | `hot` |
| Mods view: the profile split by mod, from `functions.crate` mapped through `Start.mods[].crate` | `group --by mod` |
| Merge mode: when on, identical functions merge regardless of caller path; wrapper frames (`spans::time`, `FnOnce::call_once` and others in a `known.rs` table) are hidden | `tree --merge` |
| Info points: tooltips on well-known frames and spans (`tick_blocks`, `tick_lan`, `streaming`…), keyed by stable id | the UI's translations, keyed `s:`/`f:` ids |
| Flame graph (canvas) for the current selection | `tree` |
| Ticks tab: tick-time histogram, worst ticks; clicking one shows its span tree | `ticks`, `tick` |
| GPU, Network, World, Bench and Marks tabs | their queries |
| Compare: two files; every view shows Δ and the §6.6 verdict colour | `hp_diff` |
| Copy id: every row copies its stable id, so a human can hand an agent `f:2n7xq4ka` | §5.2 |
| Export: pprof, folded, Perfetto | `hp_export` |
| Short names, with the full name on hover | `hot.full` |

### 7.3 Upload API (Next.js route handlers, compatible with playhorizon.dev)

Same conventions as the site's API: JSON replies, `cache-control: no-store` on every JSON reply, errors as
`{"error": {"code": "...", "message": "..."}}` with a matching status. Paths are relative to the app, so they keep
working when mounted on playhorizon.dev.

| Method and path | Request | Reply |
|---|---|---|
| `POST /api/perf/captures` | body: the raw `.hzperf` bytes, `content-type: application/vnd.horizon.perf` (or `application/octet-stream`);  | `201 {"id": "k3m0d9tr2q", "url": "https://…/perf/c/k3m0d9tr2q", "delete_token": "…", "shared": false, "expires_utc": "<upload + 1 h>", "bytes": 182311, "summary": {"kind": "dedicated", "seconds": 120.0, "version": "0.42.1"}}` |
| `GET /api/perf/captures/{id}` | | the raw bytes, `content-type: application/vnd.horizon.perf`, `cache-control: public, max-age=31536000, immutable` (a capture never changes) |
| `GET /api/perf/captures/{id}/meta` | | `200 {"id", "bytes", "created_utc", "expires_utc", "summary"}`, `no-store` |
| `POST /api/perf/captures/{id}/share` | header `authorization: Bearer <delete_token>` | `200 {"id", "url", "shared": true, "expires_utc"}`: pins the capture (§ retention) |
| `DELETE /api/perf/captures/{id}` | header `authorization: Bearer <delete_token>` | `204` |

- **Validation on upload:** the server decodes the capture with the same wasm/`horizon-perf` reader (built for the
  server's runtime) and refuses anything that is not a valid `.hzperf` (`415 invalid_capture`), a newer major format
  (`422 unsupported_format`) or over the size cap (`413 too_large`, default 64 MiB). It stores the bytes as uploaded,
  never re-encoded.
- **Ids:** 10 random base32 characters; links are unguessable, not listed anywhere.
- **Storage:** an S3-compatible bucket (env `PERF_S3_*`). On the umceko cluster: a RustFS server (S3-compatible) on a 1 GiB PersistentVolume (`local-path`). Not on Railway for now; staying S3-compatible keeps that move to env vars.
- **Retention, two states:** every upload starts **temporary** and is deleted 1 hour after upload (enough to open it
  in the UI from another machine or hand it to an agent). `POST …/share` pins it as **shared**: it then lives 30 days
  (`PERF_SHARED_DAYS`) or until deleted with the token. A sweep runs every 5 minutes. When the 1 GiB volume is above
  90 % full, new uploads get `507 storage_full`; shared captures are never evicted early.
- **Abuse limits:** per-IP rate limit (default 20 uploads an hour), size cap, no executable content served (the bytes
  are only ever served with the capture content type).
- **The CLI:** `horizon-perf share FILE` uploads and pins (posts to `PERF_UPLOAD_URL`, default the self-hosted API) and
  prints the link and delete token; `horizon-perf upload FILE` makes a 1-hour temporary upload; `horizon-perf summary|query|diff` accept a share link wherever they accept a file.
- **The game:** captures started with `/profiler` upload in the background after saving and print a clickable `/perf/c/<id>` link in chat. Timed captures use the same flow. The local file is retained; upload failures offer `/profiler share` to retry the latest completed capture in the session. An adjacent `.hzperf.share.json` receipt stores the endpoint and owner/delete token (mode 0600 on Unix), so a retry after a failed share pin reuses the upload. Control-socket, environment and dedicated report captures remain local unless explicitly shared through the CLI. Do not share the owner receipt.

## 8. `horizon-perf mcp`

`horizon-perf mcp` is an MCP server on stdio, in the dev tool, registered by a developer or agent with for example
`claude mcp add horizon-perf -- horizon-perf mcp`. It is a small JSON-RPC loop of its own, in the style of
`crates/horizon-server/src/mcp.rs`, without depending on that crate. Each tool calls the same library function as the
CLI subcommand of the same name, so CLI and MCP replies are identical.

| Tool | Params | Returns |
|---|---|---|
| `perf_summary` | `file`, `baseline`, `budget`, `side`, `thread`, `slow` | the summary text (it has no structured form, so there is no `json` flag) |
| `perf_query` | `op`, `args`, `files`, `filters` | §6.3's envelope |
| `perf_diff` | `a`, `b`, `min_ms`, `side`, `thread`, `limit` | `{text, reply}`: the text table and the rows `diff --json` prints |
| `perf_list` | `dir` | captures with time, sides, kind, seconds, size |
| `perf_follow` | `file`, `cursor` | `{events, cursor, done}`: every `follow --json` event (health samples unthrottled) written since `cursor`, the next cursor, and `done: true` once every side has its `Stop` |

The tools open only files ending in `.hzperf`; `perf_diff` also takes benchmark-report `.json`. Anything else is refused whatever it holds. A `perf_follow` cursor is the number of top-level fields already read; each call re-reads the file, so no state outlives it.

What leaves the game: there are no `profile-summary`, `-query`, `-diff`, `-list` or `-export` rows in `horizon-ctl`
(`crates/horizon-client/src/bin/horizon-ctl/commands.rs:314-617`). There are no such tools in `horizon-server mcp`
(`crates/horizon-server/src/mcp.rs:32-207`), and no `game_profile_*` tools in the launcher's MCP
(`launcher/src/game_tools.rs:1-4`). An agent drives the game with `horizon-ctl profile …`, as with every other
`horizon-ctl` command, and reads the result with `horizon-perf`.

## 9. Migration

| Today | After | Kept |
|---|---|---|
| `reports/perf-*.json` + `.txt` | `reports/profile-*.hzperf`; the command prints the path and the web UI's address | `horizon-report`, `horizon-server report` (same words); old files left in place |
| `horizon-report --profile` (branch) | samples by default; `--no-samples` | `--profile` accepted as a no-op for one release |
| `horizon-report monitor [MS\|off]` (branch) | sets the slow ring's threshold at runtime | same |
| `profile-*.html/.pb.gz/.folded` (branch, never shipped) | not written; the web UI and `horizon-perf export` | none needed |
| `horizon-ctl profile start\|stop\|status\|mark` (branch) | `horizon-ctl profile [server\|client] …`; no side means both | same words |
| `HORIZON_PROFILE=DIR` | writes a `.hzperf` into DIR at exit | same |
| `HORIZON_SPIKES=MS` + `slow tick … : spans` / `slow frame` lines | threshold of the slow ring; no log line at all | env var |
| "Can't keep up!" (`dedicated.rs:316`) | unchanged, exactly as vanilla prints it | — |
| `HORIZON_TRACE=FILE`, `trace FILE SECS`, `horizon-trace SECS` | a timeline-only capture; with FILE, the `.hzperf` is written there, otherwise into `reports/`; Perfetto JSON comes from `horizon-perf export --perfetto` | all three names |
| `HORIZON_SHADER_PACK_TIMING` log | `profile client start --gpu` | env var: starts pass timing and logs where the capture goes |
| `HORIZON_GPU_PASS_DETAIL` (branch) | `profile client start --gpu=detail` | env var |
| `server_bench.py --horizon-report` / `--console-at` (branches) | `--profile[=ARGS]` | `--horizon-report` as an alias for one release |
| `tools/bench_stats.py` (branch) | `horizon-bench`'s Rust tests for `ab`; `horizon-perf diff` for two captures | dropped |
| spec D `memory-diff A.json B.json` (spec) | `horizon-perf query memory --diff` | not built |
| spec D `.hzap` + `tools/alloc_profile.py` (spec) | allocation samples in `samples` (kind `alloc`) | not built |
| `pacing_bench.py` regex on `slow frame` lines | `horizon-perf query ticks --side client --worst N` on its capture, frames over the threshold only | |

Docs: `docs/hosting.md:358-379` (report, trace, slow ticks), `docs/graphics.md:134-136`, `docs/benchmark.md:79-82`,
`docs/memory.md`, `docs/agent-control.md:152`, and `docs/horizon-ctl-commands.md` (regenerated). New:
`docs/perf-platform.md` (this design), and a "Performance captures" section in `docs/hosting.md` for server owners:
how to run `horizon-report`, where the file goes, how to open it in the web UI, and how to profile from a client with
level 4.

## 10. Overhead budgets

Each producer measures its own cost during the capture and writes it to `Stop.overhead`. The summary's `cost` line
prints it, and adds a warning when a budget is exceeded.

| Producer | When on | Budget | Known measurement |
|---|---|---|---|
| Health recorder and slow ring | always, in memory | < 0.05% of the tick/render thread; one `/proc` read per second off-thread | status.rs and the supervisor already sample at this rate |
| Spans aggregate | during a capture; always on dedicated servers (it feeds the slow ring) | ≤ 1% of MSPT | "usually well under 1%" (`docs/hosting.md`), calibrated per machine (`report.rs:282-293`) |
| CPU samples, 250 Hz | during a capture | < 1% of the sampled thread; writer < 0.5% of a core | ~0.05% + 0.13% of a core (measured during the sampler work) |
| Wall samples | `--wall` | < 1% of the sampled thread | same handler |
| Allocation samples | `alloc-census` builds, `--memory` | < 3% of an allocation-heavy thread | not built yet (H 4b) |
| Context stores | always (plain stores) | < 2 ns each | `store_cost` test, ignored (`perf/sampler-viewer:crates/horizon-threads/src/work/tests.rs:149`) |
| Alloc counters | `alloc-count` builds | +1-2 ns per allocation | `docs/memory.md` on `perf/alloc-count` |
| Counters | always | one relaxed atomic per `add` | |
| Mob cost timing | during a capture | 1 tick in 4, two clock reads per mob | `tick_profile.rs:4-5`, `:16` |
| GPU pass timing | `--gpu` | ≤ 0.1 ms GPU per frame; never a CPU wait | async readback (`pack_timing.rs:16-20`) |
| GPU detail | `--gpu=detail` | half the timed frames, plus one calibration wait at enable | C's alternation (`perf/gpu-pass-detail-output:…/pack_timing.rs:342-353`, `:767`) |
| Timeline | `--timeline` | ≤ 3% while on; capped at 60 s | global mutex per span (`spans.rs:335`, `:363`) |
| Memory snapshots | during a capture | < 2 ms every 10 s, on the thread that answers `memory` | |
| Writer: encode, deflate, sync flush per field | during a capture | off the game threads; never blocks a tick; < 0.5% of a core | sync flush costs a few bytes per field; to be measured in F1 |
| Symbolization | at stop | off the game threads | already on the "horizon-profile-report" thread |
| Remote transfer | after stop | ≤ 256 KiB/s, behind gameplay packets | |

## 11. Work pieces (for the tracker issue)

One issue per piece, listed under one tracker issue (sunrise-org/horizon#1). PR size is not limited for this project; the line counts below are estimates only. Each PR carries `pr_ready.py`'s
block, the attribution lines, and docs for what it ships. Labels:

- `feature` where players get something new (S1a, C1, R1b);
- `skip-release` for the dev tool, `horizon-perf-format` internals and the docs-only pieces;
- none for fixes.

Pieces marked **contract** touch the `.proto` or the fixtures and need the contract owner's review. Spec H's producer
PRs come first and are tracked as spec H describes them:

- 1, the sampler core, split into its sampling and unwinding parts;
- 1b, stack copies;
- 3, context stores;
- 4-session, aggregation and symbolization, without `html.rs`, `viewer.rs`, `viewer.js` and the folded/pprof writers.

They land from `perf/sampler` commits 6e510a955b, 9961c6521e, 9eb16b45f5, 429688f422 (the session half) and
bd412b6d62.

| Id | Piece | Needs | Lines (est.) | Tests with signal |
|---|---|---|---|---|
| F1 | **contract** `horizon-perf-format`: `capture.proto` (top level, `Start`, `Stop`, strings, marks, warnings, health), `protox` + `prost-build` in `build.rs`, the gzip writer with a sync flush per field, the writer thread and bounded channel; the frozen fixture `v1.hzperf` | — | 450 | The frozen fixture decodes to the values listed in the test. After each flush, the bytes on disk inflate to exactly the fields written so far (checked after every field of a synthetic capture). A stream containing a field number unknown to the reader decodes, and the known fields keep their values. The descriptor-set lint rejects a test `.proto` with an enum missing `_UNSPECIFIED`, a JSON-name collision and a `group`, and passes `capture.proto`. A full channel drops and counts, and the sending call returns without waiting. |
| F2 | Health recorder (tiers), slow ring, counter registry with labels, in `horizon-perf-format` | F1 | 350 | Two synthetic hours of ticks with a known MSPT step at 45 min: each tier's means equal values computed independently in the test, and TPS 1/5/15 equal hand-computed values. Seven synthetic days leave the rings' memory size unchanged. A counter registered twice with the same labels adds up; the same name with other labels is a separate series. The ring keeps the last 256 over the threshold. |
| P1 | `horizon-perf`: whole-file reader, follow reader, `latest`, `list`, `follow`, a `summary` with header and health only | F1 | 400 | The follow reader, fed a file that grows in random-size appends, yields the fields in order and never a partial one. A file with no `Stop` is refused by the whole-file reader with the §3.2 message. `format = 2` is refused with the update message. `latest` picks the newest finished file and skips one still being written. |
| S1a | Dedicated server: `horizon-report` writes `.hzperf` (`Start`, `history`, `slow_history`, `health`, `counters`, `slow`, `Stop`), prints the path and address, retention, `--no-samples` accepted | F2 | 400 | A fixture server's capture reads back through `horizon-perf` with the tick count and TPS that the fixture forced. Retention deletes the oldest of 31 matching files and never a renamed one. The printed line contains the written path. |
| S1b | **contract** `span_blocks`, `spans`, `workers`, `net`, `world` moved from `report.rs`; `perf-*.json`/`.txt` no longer written; privacy allowlists | S1a | 480 (mostly moved) | The existing report tests' expected values (ticks, spans, mob costs, network) now read back from the capture. The §4.3 privacy fixture finds none of the forbidden strings in the inflated bytes. Block sums of `span_blocks` equal the `spans` totals. |
| Q1 | Queries without samples (`spans`, `ticks`, `tick`, `series`, `world`, `net`, `meta`, `memory`, `marks`, `warnings`), ids, filters, `summary` without hot functions | P1 (S1b for a real fixture; synthetic until then) | 500 | On a fixture with hand-made spans (the `slowticks.py` arithmetic done by hand in the test), `spans --slow` gives the known wall, cpu and waited values. The 200-bot fixture's summary stays within 1,500 tokens (bytes / 4) and keeps its rank order when `--budget 600` cuts it. Ids are equal for two captures whose string tables are in different orders. `memory --diff` reproduces spec D's documented example. `series --from -900` reaches into `history`. |
| S2 | **contract** `samples` from `horizon_threads::Profile` (in horizon-client-render); `horizon-report` samples by default | spec H, S1a | 450 | Spec H's context test, end to end: A (3 units) / B (1 unit) gives `group --by entity` shares of 0.75 ± 0.1 / 0.25 ± 0.1. The slow-tick filter keeps only the slow tick's function. The function table is deduplicated: one row per address range. `--no-samples` writes no `samples` field. |
| Q2 | Sample queries `hot`, `tree`, `callers`, `callees`, `split`, `group`, `frames` with filters and several captures; hot functions in the summary; name normalization | Q1, S2 (synthetic until then) | 480 | A synthetic tree with known weights gives `callers` equal to the known caller set and `callees` equal to the known children. `split --by state` sums to the node total. Normalization is table-driven: real mangled names from a Horizon build (`<horizon_world::level::Sim<…>>::set_block_limit` etc.) map to the expected short names. `frames` with 3 captures gives 3 columns equal to the per-capture runs. |
| D1 | `stats`: two-sample HL + exact Mann–Whitney CI, Theil–Sen drift; `diff`; `summary --baseline`; benchmark reports' `ab` read and printed | Q1 | 450 | Mann–Whitney vectors computed outside the code (by hand for n = m = 6; a table of published critical values for larger n). A capture diffed with itself gives `within_noise` everywhere. A synthetic B with +2 ms/tick on one function in every block gives `slower`, with 2 inside the CI, and `~` for the other functions. A drifting synthetic A raises the drift warning. A benchmark report pair with `ab` prints `ab`'s verdict unchanged. |
| M1 | `horizon-perf mcp` with the five tools | Q1 (more ops appear as Q2 and D1 land) | 300 | `tools/list` lists the five tools with their schemas. A `perf_query` call on a fixture returns byte-identical JSON to `horizon-perf query`. `perf_follow` with the returned cursor never repeats or skips a sample of a growing file. A non-`.hzperf` path is refused. |
| C1 | Client captures: `horizon-ctl profile client start\|stop\|status\|mark`, `/profiler`, `HORIZON_PROFILE`, client `Start` and `health` from `FrameStats`, `<game home>/reports/`, retention, the translated "saved" message | F2, S2 for samples | 450 | An offscreen game (`horizon-ctl launch --offscreen`) with a scratch `HORIZON_HOME` writes its capture there, and it reads back with the frame count the run made. `mark NAME` appears at its time. `stop` replies with the path. The `horizon.perf.*` keys exist in `en_us.json`. |
| C2 | Both sides in single player: the integrated server thread writes `side = SERVER` into the same writer; `profile` with no side means both; `profile server` in single player | C1, S1b | 300 | A single-player run captured with no side holds both sides' `Start` and `Stop`. `summary` shows both, server first. `--side client` selects only render-thread samples. |
| R1a | Remote, part 1: `horizon:perf` channel, the Horizon check, operator level 4 check, the one-at-a-time rule, the server log lines, the server's copy in `reports/` | C2 | 400 | Over `lan_e2e.py` (host on its server thread, a real guest): a guest at level 4 starts a server capture and the host's `reports/` gets it; a guest at level 3 gets `Denied`, and no file is written; the host log names the guest at start and stop. A non-Horizon server fixture (no `horizon:block_registry`) gives "this server isn't running Horizon". |
| R1b | Remote, part 2: rate-limited chunk transfer, the merge into the client's file, `SERVER_PART_INCOMPLETE` | R1a | 350 | The merged client file's server side is byte-identical, once inflated, to the server's copy without its `format` field. The transfer stays under 256 KiB/s averaged over 1 s. A disconnect mid-transfer leaves a client file that reads, with the warning. |
| T1 | Timeline: `timeline` chunks, the caps, `HORIZON_TRACE`/`trace`/`horizon-trace` as timeline captures; the old Chrome-trace writer removed | F1, C1 | 400 | A known span sequence writes chunks whose events have the recorded durations and depths. The 60 s cap stops the timeline and sets `timeline_stopped_t`. `horizon-ctl trace FILE 2` writes a capture at FILE. |
| E1 | `horizon-perf export --pprof\|--folded\|--perfetto` (pprof writer moved) | Q2, T1 | 350 | The pprof output decodes with the sampler's tiny test decoder, and its sample totals equal the capture's. Folded line counts sum to the samples. A Perfetto export of a known timeline parses as a JSON array whose `X` events have the recorded durations and nesting. |
| G1 | **contract** `gpu` from `PassTiming` (+ C's parts and counters); `--gpu[=detail]`; the `HORIZON_SHADER_PACK_TIMING` and `HORIZON_GPU_PASS_DETAIL` aliases | C1, C PRs 1-3 | 300 | C PR 3's `fragments_per_pixel` vector (6,300,000 / 2,073,600 → 3.04) reads back through `query gpu`. A fake `PassTiming` with known medians gives those medians. |
| B1 | **contract** `bench`; `--benchmark … --profile`; bare benchmark reports as captures; `horizon-perf attach`; `server_bench.py --profile` | P1, C1 | 400 | A report written by `horizon-bench`'s `Report::write_to` reads through `horizon-perf` (dev-dependency) with equal metrics. `attach` keeps a field number unknown to it. A Python unit test checks that `server_bench.py --profile` builds the right console line and calls `attach` with its run record. |
| W1 | `wasm` feature, the C-ABI exports, `horizon_perf.js`, `cargo xtask perf-wasm`, `web/perf/` skeleton (file open, wasm load, raw summary/query view), the CI job | Q1 | 400 | The **agreement test**: the C-ABI functions, called natively in a test, return byte-identical replies to `horizon-perf query --json` and `summary` on the fixture. CI builds `--target wasm32-unknown-unknown`. A malformed file gives an error reply, not a trap. |
| L1 | Retire: slow-tick and slow-frame span lists leave the log; `pacing_bench.py` reads captures; docs (`hosting.md`, `graphics.md`, `benchmark.md`, `memory.md`, `agent-control.md`), regenerated ctl docs; this design doc as `docs/perf-platform.md` | S1a, C1, Q1 | 350 | 300 slow ticks in 10 s write no Horizon slow-tick line, and all 256 most recent ones are in the ring. "Can't keep up!" still prints the vanilla text at the vanilla rate (the existing test at `dedicated.rs:2519` passes unchanged). `pacing_bench.py`'s parse test runs on a fixture capture and finds the same slowest spans as its old log fixture. |

The web UI pieces live in `sunrise-org/horizon-site`, on the platform tracker. Each needs W1:

- UI1: health bar and metadata panel;
- UI2: call tree, group by, selection, detail panel and search;
- UI3: flat view, mods view, merge mode and info points;
- UI4: flame graph and ticks tab;
- UI5: compare mode and the GPU/Network/World/Bench/Marks tabs;
- UI6: export and copy id;
- UI7: Share button (notice, upload, link), opening `…/perf/c/<id>` links, compare by two links.

Upload pieces (in `sunrise-org/horizon-site/app/api/perf/`):

- U1: the four route handlers of §7.3 with S3-compatible storage, validation through `horizon-perf`'s wasm, size cap,
  ids and delete tokens. Tests: a fixture uploads, reads back byte-identical with the immutable cache header, `meta`
  matches its summary; a non-capture gets 415, a newer major 422, an oversized body 413; DELETE with the wrong token
  is 403, with the right one 204 and a later GET is 404.
- U2: the share pin, the 5-minute sweep, the 90 % full guard and the per-IP rate limit. Tests: an unshared capture is
  gone after 1 h plus a sweep while a shared one stays; `share` with a wrong token is 403; at 90 % use an upload gets
  507; the 21st upload in an hour from one IP gets 429.
- U3: `horizon-perf share` and share links accepted wherever a file is. Test: against a local instance of U1, `share`
  then `summary <link>` equals `summary <file>`.

Each UI piece's check is that the numbers on screen equal the matching `horizon-perf query --json` on the fixture. UI7 needs U1.

Order: F1 and spec H's PRs can start at once. P1, F2, S1a and C1 follow F1. Agents have everything by Q2 + D1 + M1. L1
goes last, so nothing leaves the log before the capture that replaces it has shipped.

Branches: `perf/sampler-viewer` and `perf/sampler-tree` are never opened; delete them once UI2 has taken what it needs
from their tree logic. `perf/alloc-count`, `perf/bench-travel`, the capture stack and C PRs 1-3 land as planned.
`perf/streaming-caps` S3-0 lands without its `report.rs` and `--horizon-report` hunks (S1a and B1 redo them).
`perf/bench-ab-statistics-impl` lands without `tools/bench_stats.py` and its Python tests.

Later, outside this plan:

- a Prometheus exporter for the counter registry;
- upload and share links;
- a JSON export through the canonical mapping;
- macOS and Windows samplers (H PR 2);
- allocation samples (H 4b) into `samples`;
- `HORIZON_STARTUP_TRACE` as a startup timeline;
- `HORIZON_SPLIT_STATS` and `HORIZON_OCCLUSION_PROFILE` as counters;
- `net.by_packet`;
- compile features that gate parts of `horizon-perf-format` if it grows.

## 12. Open questions

None at the moment.

## 13. Decision log (user's review, 2026-11-07)

| # | Decision |
|---|---|
| Q1 | All three audiences (agents, server owners, developers) from day one; built by volunteers, no number stated. |
| Q2 | Protobuf, extension `.hzperf`. Chosen for schema-as-contract, decode speed and memory, and timeline scale (the 2-min profile is 547 KB as columnar JSON, 120 KB deflated). |
| Q3 | Files only in v1; the counter registry is label-ready, so a Prometheus exporter is one small PR later. |
| Q4 | The game ships only `horizon-perf-format` (generated types, gzip writer, health recorder; Q4's original "framed writer" is superseded by Q9). Everything that reads lives in the dev-only `horizon-perf`, including `horizon-perf mcp` and the wasm build. No reading commands or MCP tools in horizon-ctl or horizon-server; the game prints the path and the web UI's address. |
| Q5 | Local files only; upload and share links later, with Ludovic. |
| Q6, Q7 | One issue per piece under a tracker issue, no fixed lanes; the design doc lives in the repo. |
| Q8 | The user owns the contract (`.proto`, fixtures). |
| Q9 | One gzip stream holding one protobuf `Capture`; no frames, index, crash recovery or partial reads; our tools only. The live-follow rules (§3.2) are part of the contract. |
| Q10 | Whole-file gzip/deflate, pure Rust (`flate2`; F1 #232). |
| Q11 | The `.proto` files are the contract. No JSON export for now, but nothing that breaks the canonical JSON mapping. |
| Q12 | Codegen with `protox` in `build.rs`. |
| Q13 | No size budget for now; compile flags later if needed. |
| Q14 | `horizon-ctl profile start\|stop\|status\|mark` stays in the game as a trigger. |
| Q15 | The web UI is implemented in horizon-site; the user self-hosts it while developing. |
| Q16 | No always-on health file; if ever added, off by default. The in-memory rings stay. |
| Q17 | `horizon-report` samples by default; `--no-samples` turns it off. |
| Q18 | Client captures go to `<game home>/reports/`; retention keeps 30 files or 1 GiB, and renamed files are untouched. |
| Q19, Q23 | Horizon's slow-tick and slow-frame span lists leave the log entirely. Vanilla's "Can't keep up!" stays exactly as vanilla prints it. |
| Q20 | `tools/bench_stats.py` is dropped. |
| Q21 | The web UI loads `horizon-perf` as wasm for reading, queries and statistics; the UI only draws. |
| Q22, Q24 | `horizon-ctl profile server\|client`; no side means both, in one capture. A dedicated or remote server needs operator level 4 (as vanilla's `/perf`; this refines Q22's "admin"). The capture is sent back in rate-limited chunks over a Horizon-only channel, a copy stays in the server's `reports/`, and the server logs who profiled and when. Non-Horizon servers: "this server isn't running Horizon". |
| A | The web UI and upload API live in `sunrise-org/horizon-site`, integrated into the site and hosted in namespace `sunrise` on the umceko cluster. This supersedes the standalone `web/perf/` app. |
| B | `horizon-perf` is built from source by whoever needs it (agents included); no published dev artifact. |
| C | The `horizon-ctl profile` trigger is exposed over MCP (`mcp: true`). |
| D | `ab_bench.py`'s relaunch mode calls `horizon-perf diff` instead of the dropped `bench_stats.py`. |
| E | New dependencies `prost`, `prost-build`, `protox` accepted. |
| F | Web UI and upload API self-hosted at `https://playsunrise.net` (umceko cluster); `PERF_UI_URL` points there. |
| G | No CODEOWNERS: once merged, anyone can edit the contract (supersedes Q8's ownership after the contract lands). |
| H | Both the UI and the upload layer are built compatible with playhorizon.dev's stack (Next.js App Router on Railway, JSON `/api/…`, `no-store`) so they plug in later: `sunrise-org/horizon-site` owns `app/perf/` and `app/api/perf/`. Upload and share links are now in scope on the self-hosted instance (supersedes Q5's "local files only"); sharing is an explicit user action. |
| I | Upload storage: RustFS (S3-compatible) on the umceko cluster, 1 GiB PV on `local-path`; not Railway for now. Size cap 64 MiB; per-IP limit 20 uploads/hour. Retention: two states: temporary uploads are deleted after 1 h; Share pins a capture for 30 days or until deleted with its token; above 90 % volume use, new uploads get 507. |
| J | `horizon-perf-format::files` (capture file naming, retention, the saved line, `is_complete`) lives in the format crate although §2.1 lists three things: `horizon-server` writes and finds captures too and cannot depend on `horizon-client`. Do not move it back. |

## Capture-wide timing lows

`SpanSummary.timing` optionally records the unit count and average duration of the slowest 1% and 0.1% of all captured frames or ticks. Each tail contains `ceil(count / 100)` or `ceil(count / 1000)` units. These are tail means, not p99 or p99.9 thresholds. The recorder retains up to one million f32 durations per active side (about 4 MiB), sorts once at capture completion, and writes only the summary. Invalid durations or exceeding that bound omit the timing summary instead of silently using a partial sample.

`horizon-perf query meta` and the shared WASM reader expose `timing.count`, `slowest_1pct_ms`, `slowest_01pct_ms`, `low_1pct`, and `low_01pct`. FPS lows are `1000 / tail_mean_ms`. Tick lows are work-based TPS capacity, `min(20, 1000 / tail_mean_ms)`, and exclude scheduling delays. The website shows both lows and both tail durations, with the captured count and rounding rule. Old captures remain readable and have no timing lows; the reader never reconstructs them from health percentiles, coarse buckets, or the small worst-unit list. Benchmarks already store both FPS lows and the website exposes both.
