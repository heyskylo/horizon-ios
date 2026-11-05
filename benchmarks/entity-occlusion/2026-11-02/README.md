# Entity occlusion split measurements — 2026-11-02

These measurements came from the pre-split integration branch for PR #7054. That build contained the entity-occlusion rewrite and particle light lookup cache; Particle Culling was Off. They are useful comparison evidence, but they are not a benchmark of this isolated entity-only commit. The cache-only run in PR #7391 measured effectively the same CPU time on the ordinary scene, but the combined result still cannot isolate interactions.

Host: Intel Core i7-3770S, Intel HD Graphics 4000, Vulkan/Mesa, 675×727, render distance 4, VSync off, 260 FPS cap. Two opposite-order passes measured for 8 seconds after warmup. Values are means. CPU is harness average CPU ms per frame; p99 is frame time. The driver supplied no usable GPU timing.

## Ordinary scene

The scene had 24 mobs, roughly 9,400–10,100 rain particles and campfire smoke.

| Build and mode | FPS | CPU ms/frame | Frame p99 ms |
| --- | ---: | ---: | ---: |
| Main, entity culling Off | 88.59 | 1.829 | 24.51 |
| Pre-split integration branch, all culling Off | 88.33 | 2.063 | 24.51 |
| Main, entity culling Synchronous | 89.34 | 1.965 | 23.99 |
| Pre-split integration branch, entity Synchronous | 89.13 | 2.113 | 24.15 |

This normal-scene run does not show a large FPS change; its CPU differences are small relative to frame time and not attributable to the entity-only change by themselves.

## Opaque-wall scene

A 32×32 platform at Y=300, a 7-block wall with a one-block vertical opening, and 192 pigs spawned behind the wall (216 mobs in the captured scene). The player looked toward the opening from the near side. The same world and camera path were used for the main and integration builds.

| Build and mode | FPS | CPU ms/frame | Frame p99 ms |
| --- | ---: | ---: | ---: |
| Main, Off | 89.08 | 2.423 | 13.52 |
| Integration branch, Off | 89.23 | 2.355 | 13.27 |
| Main, Synchronous | 92.81 | 2.566 | 14.60 |
| Integration branch, Synchronous | 93.21 | 2.550 | 12.84 |
| Main, Asynchronous | 92.73 | 2.364 | 13.04 |
| Integration branch, Asynchronous | 92.62 | 2.314 | 13.18 |

The asynchronous mode reduced measured CPU time slightly in this wall scene but did not improve FPS beyond run-to-run spread. This is a synthetic occlusion-heavy scene, not a broad performance claim.

## Captures

The HUD performance overlay is visible in each capture. The integration-build screenshots were recorded with the same wall and camera position in Off, Synchronous, and Asynchronous modes.

- Main, Off: `main-off.png`
- Integration branch, Off: `branch-off.png`
- Integration branch, Synchronous: `branch-sync.png`
- Integration branch, Asynchronous: `branch-async.png`
