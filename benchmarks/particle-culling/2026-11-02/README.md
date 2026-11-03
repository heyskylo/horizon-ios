# Particle culling review measurements — 2026-11-02

Ordinary-scene captures and measurements for PR #7054. The scene contains 24 mobs, about 9,400–10,100 rain particles, and about 50–100 campfire smoke particles. Two opposite-order passes were run with 2 s warmup and 8 s measurement per condition. Host: Intel Core i7-3770S, Intel HD Graphics 4000, Vulkan/Mesa, 675×727, render distance 4, VSync off, 260 FPS cap.

These are means of the two samples. CPU is the harness's average CPU time per frame; p99 is frame time. The normal scene is not expected to show large gains because most particles are visible.

| Condition | FPS | CPU ms/frame | Frame p99 ms |
| --- | ---: | ---: | ---: |
| Main, culling Off | 88.39 | 1.930 | 23.85 |
| Particle branch, culling Off | 88.55 | 1.828 | 24.88 |
| Main, Entity Culling Synchronous | 89.12 | 1.887 | 23.07 |
| Particle branch, Entity Culling Synchronous, Particle Culling Off | 89.06 | 1.897 | 25.18 |
| Particle Culling Synchronous | 88.87 | 1.774 | 24.94 |
| Particle Culling Asynchronous* | 89.00 | 1.754 | 24.90 |

*The asynchronous samples predate the final switch to a dedicated particle worker. They are retained only as historical context and are not a measurement of the final worker implementation. No FPS improvement is claimed from these ordinary-scene runs. The Off path delegates directly to `render`; `disabled_culled_render_matches_the_regular_particle_path` checks that it produces the same vertices.

The FPS and CPU differences are within run-to-run variability for this GPU-limited scene. The first four conditions are the reviewer's requested Main-versus-branch default and existing synchronous-mode comparisons. Particle Sync/Async rows are branch-internal mode comparisons.

## Captures

The screenshots show the same ordinary test world and FPS overlay:

- `main-off.png`
- `particle-off.png`
- `particle-sync.png`
- `particle-async.png`
