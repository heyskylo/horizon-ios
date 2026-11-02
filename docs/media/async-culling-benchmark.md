# Async culling benchmark

Run date: 2026-11-02.

## Setup

- Hardware: Intel Core i7-3770S (8 threads), Intel HD Graphics 4000 (IVB GT2), Intel Mesa Vulkan driver; Linux / Hyprland.
- Build: `cargo build --profile fast -p horizon-client -p horizon-client-lib --bins --jobs 8` (successful).
- Scene: a 32 × 32 stone platform at Y=199, a 32-wide stone wall at Z=-5 with one open column, and 5,000 stationary armor stands packed behind the wall. Each mode used a clone of the same saved world. Counts were reduced to 2,000, 500 and 91 between samples.
- Camera: fixed at (0.5, 200, 2), yaw 180°, pitch 0°, world time 6000; render distance 2, VSync off, 260 FPS cap. F3 reports a 671 × 725 game viewport at 100% pixel density; the desktop tiled the requested window.
- Two passes were run with opposite mode order. Every sample had a 2-second warm-up followed by a 6-second measurement. These remain single samples per pass, not a statistical benchmark suite.

## Results

CPU is the benchmark's measured average CPU time per frame. Frame percentiles are milliseconds.

| Stands | Pass order | Mode | Average FPS | 1% low FPS | CPU average | Frame p50 / p95 |
|---:|---|---|---:|---:|---:|---:|
| 5,000 | Async → Sync | Async | 67.89 | 42.45 | 14.41 ms | 14.61 / 16.98 ms |
| 5,000 | Async → Sync | Sync | 24.09 | 16.60 | 41.18 ms | 41.44 / 44.93 ms |
| 5,000 | Sync → Async | Async | 67.07 | 40.14 | 14.56 ms | 14.85 / 17.12 ms |
| 5,000 | Sync → Async | Sync | 16.06 | 12.87 | 61.97 ms | 61.82 / 67.85 ms |
| 2,000 | Async → Sync | Async | 90.44 | 69.05 | 10.75 ms | 10.97 / 12.89 ms |
| 2,000 | Async → Sync | Sync | 87.31 | 59.36 | 11.12 ms | 11.20 / 13.60 ms |
| 2,000 | Sync → Async | Async | 87.42 | 57.28 | 11.08 ms | 11.12 / 14.06 ms |
| 2,000 | Sync → Async | Sync | 71.67 | 56.34 | 13.64 ms | 14.03 / 16.04 ms |
| 500 | Async → Sync | Async | 150.21 | 89.56 | 3.31 ms | 6.20 / 9.16 ms |
| 500 | Async → Sync | Sync | 183.15 | 106.81 | 1.50 ms | 4.56 / 8.40 ms |
| 500 | Sync → Async | Async | 145.91 | 93.45 | 3.44 ms | 6.55 / 9.39 ms |
| 500 | Sync → Async | Sync | 178.21 | 93.78 | 1.44 ms | 4.73 / 8.52 ms |
| 91 | Async → Sync | Async | 173.40 | 106.05 | 1.48 ms | 5.00 / 8.41 ms |
| 91 | Async → Sync | Sync | 186.25 | 102.13 | 1.01 ms | 4.41 / 8.31 ms |
| 91 | Sync → Async | Async | 172.12 | 101.97 | 1.57 ms | 5.28 / 8.23 ms |
| 91 | Sync → Async | Sync | 186.29 | 103.69 | 0.92 ms | 4.40 / 8.25 ms |

Async consistently helped in this scene with 5,000 stands: about 67 FPS and 14.5 ms CPU, versus 16–24 FPS and 41–62 ms for Sync. At 2,000, Async was similar or modestly faster; at 91–500, Async's worker and cache overhead made it slower. This is a CPU-side result for one synthetic workload. The Intel driver returned no usable GPU timestamps, so it does not establish a GPU-time improvement.

## Cache behavior

The previous four-way result cache suffered capacity thrashing in the earlier run on this saved scene: it recorded 84,216 evictions at 500 stands and 158,952 at 2,000. The sorted bounded cache recorded zero capacity evictions on the merged-head run at all four sizes. At 5,000 it served about 3,519 queries per frame with 1,125 total misses during the six-second sample; at 2,000 it served about 1,723 queries per frame with 3,640 total misses and zero capacity evictions. The misses were invalidated results, not capacity replacement. The old-cache counter run preceded the update to current `origin/main`, so treat that as an eviction diagnostic rather than a direct FPS comparison. These counters show the targeted churn was removed; the FPS comparisons above are the merged-head mode comparisons.

## Screenshots

The F3 captures show the same camera and 2,000-stand scene. A witness stand is aligned with the wall opening so it remains visible through the occluder.

![Async mode, 2,000 stands](async-culling-async-2000.png)

![Sync mode, 2,000 stands](async-culling-sync-2000.png)

![Advanced Graphics with Async entity culling selected](async-culling-settings.png)
