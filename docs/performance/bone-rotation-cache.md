# Exact local bone rotation reuse without TAA

**The cache runs only while the TAA setting is off.** When TAA is enabled, the GPU skinning walk uses its original per-part rotation and normal calculations. Changing the existing anti-aliasing setting updates this policy on the next frame; no restart or additional setting is needed. The policy is selected once per model, with separately compiled cached and original tree walks so the bypass has no per-bone cache-policy branch.

Visible mobs often repeat exactly the same local rotations. `SkinModels` keeps 64 bounded direct-mapped entries keyed by all three angles’ exact `f32` bits, with the geometry `Mat4` and separate JOML-compatible normal `Mat3`. An exact positive-zero identity entry stays outside the table. Collisions recompute and replace an entry; angles are never approximated.

Entity/camera translations, scale, matrix multiplication order, animation and normal normalization remain per draw. The CPU fallback is unchanged. There is no additional resource-pack or native-mod restriction.

## Final production-build measurements

Ryzen 5 7500F / RTX 4060, Linux Vulkan with native Wayland, 1920×1080, render distance 4, native resolution, VSync/OIT/dynamic lights off. A normal seed 7 snapshot contains 256 frozen NoAI/NoGravity zombies, cows, villagers and spiders at Y=110, with zero particles. Several rows overlap on screen; this measures crowded mob preparation rather than normal exploration.

Four neighboring A/B pairs per TAA state use separately compiled original/final binaries in ABBA/BAAB order. Each process warms for 3 seconds and measures 12 seconds. Actual size, camera, entity/particle counts, completed streaming and unchanged physical-input counts are asserted. Games run silently on workspace 9. No builds, tests, profiles or heavy analysis run during timing.

| TAA | Original mean FPS | Final mean FPS | Paired mean FPS change | Original 1% low | Final 1% low | Paired 1% low change | CPU time change |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| off (cache enabled) | 1698 | 1907 | +12.30% | 771 | 794 | +2.43% | -12.30% |
| on (cache bypassed) | 1657 | 1652 | -0.31% | 643 | 646 | +0.39% | +0.40% |

Absolute FPS/low columns average the four process samples per build. Changes are geometric means of candidate/original ratios within complete neighboring pairs. Every individual pair is in [the sample record](bone-rotation-cache-samples.json). Four pairs on one computer do not establish performance on other hardware or workloads. Other applications remained available to the owner, and GPU clocks were not locked.

The four 1% low pair changes are mixed: without TAA −4.32%, +25.34%, −11.99%, +4.31%; with TAA +4.38%, −1.61%, +7.81%, −8.25%. A repeatable 1% low improvement is not established. The mean-FPS gain without TAA is positive in all four pairs.

## Why TAA bypasses the cache

The earlier unrestricted cache improved mean FPS but regressed 1% low with TAA: +5.72% mean FPS and −9.88% 1% low in the original Vulkan/X11 crowd series, with all four low pairs negative. The unrestricted TAA-off series improved mean FPS 13.49% and 1% low 19.91%. Those original crowd samples, including all negative pairs, are retained separately in the sample record; they are not pooled with the final Wayland series.

Follow-up controls localized long waits to window-image acquisition and showed a dependency on overlapping GPU/presentation work. The underlying scheduling cause is not fully established. This PR conservatively selects the original arithmetic whenever the TAA setting is on; it does not claim to fix TAA or the renderer queue.

Two earlier bypass prototypes were slower with TAA and were rejected. Their complete measurements are retained in the sample record. The final implementation keeps the original `Walk`, `render::posed` and `ModelNormals::part` bodies intact, alongside the cached walk.

## Correctness and validation

- Six skin tests pass with the prepared models present, including 465 GPU-path draws, 75 CPU-fallback draws and 512 synthetic nested draws covering signed zero, adjacent float values, cache churn, animation, hidden/skipped parts and mirrored/nonuniform scale.
- A new test switches rotation reuse off/on/off/on between changing poses, checks that bypass leaves cache entries untouched, and compares every expanded vertex byte against the CPU reference.
- Independently compiled original/final libraries produce byte-identical model/part/job buffers, bounds and CPU-fallback vertices across 540 draws per mode (90 species × 3 ages × 2 layer settings), both with the cache enabled and bypassed: 2,158,020 bytes per version.
- The final TAA-off before/after screenshots match every RGB byte. The TAA-off screenshots after both UI toggle cycles also match the initial image. TAA history-dependent screenshots are not used as equality proof.
- The actual game’s existing Anti-aliasing control was exercised through off→TAA→off→TAA→off in one running final binary. Every step returned to playing with 256 mobs and zero particles.
- `tools/pr_ready.py --base upstream/main`: 3160 Rust tests, 708 Python tests, rustfmt and Clippy with warnings denied passed. Vanilla Java/JAR harnesses were not run because their data is unavailable locally.

Production code uses only the existing Horizon arithmetic. Investigation of the rejected unrestricted TAA result also inspected the vendored wgpu-hal 30.0.1 Vulkan implementation under MIT/Apache-2.0; no code from it was copied or adapted into this change. No game resources are included.

Binary/source hashes, per-sample statistics, all paired outcomes and quality-proof identities are in [bone-rotation-cache-samples.json](bone-rotation-cache-samples.json).
