# Entity occlusion pipeline — release/OpenGL run (2026-11-04)

This is the final local smoke benchmark for the entity-occlusion pipeline before opening the PR. It was run from the branch after the release build completed, with the OpenGL backend selected explicitly. The same process created each scene, switched the culling mode, flew the camera for two seconds, collected frame/CPU telemetry, and captured the screenshot after the scene settled.

**Host:** Intel Core i7-3770S, Intel HD Graphics 4000, Mesa OpenGL 4.6
**Window:** 1280×720, offscreen, VSync disabled
**World:** fresh temporary home, 32×32 platform at Y=300; wall scenes add a solid stone wall and an optional one-block slit
**Entities:** 24 pigs in the normal scene, 192 pigs in wall scenes, 512 pigs in stress scenes
**Settings:** render distance 8, `entityCullingPrecheck:64`, FPS overlay on, no resource-pack occlusion assumptions
**Build:** `cargo build --release -p horizon-client`

## Results

The values below are branch-only measurements. They are useful for comparing the three modes under identical geometry and for checking that the worker is doing useful work. A fresh main binary was not built in this short pass, so these numbers do not claim a controlled main-to-branch delta.

| Scene | Off FPS / CPU p50 | Sync FPS / CPU p50 | Async FPS / CPU p50 | Async entities culled |
| --- | ---: | ---: | ---: | ---: |
| `entity_normal` (24 visible) | 240.5 / 0.586 ms | 233.5 / 0.762 ms | 239.3 / 0.686 ms | 0 |
| `entity_wall` (192, solid) | 228.8 / 1.245 ms | 318.6 / 1.342 ms | **324.5 / 0.478 ms** | 124,608 |
| `entity_wall_slit` (192, one-block slit) | 231.4 / 1.102 ms | 265.0 / 1.387 ms | **281.2 / 0.716 ms** | 102,826 |
| `entity_stress` (512, solid) | 190.3 / 2.467 ms | 299.2 / 2.593 ms | **325.5 / 0.691 ms** | 290,234 |
| `entity_stress_slit` (512, one-block slit) | 190.4 / 2.476 ms | 262.2 / 2.573 ms | **262.1 / 1.297 ms** | 227,301 |

Async telemetry confirmed that the render thread stayed non-blocking (`try_recv`/`try_send`, `worker_blocking_wait_ms: 0`). The latest wall/slit run completed 562 worker jobs, queued 3,982 queries, avoided about 4.0 million entity bytes, and had 379 ns recent p50 worker-job time. The solid 512-pig run culled 15,552 generated vertices; the slit run culled 120,290 generated vertices. The normal scene recorded no pre-model hides and kept the opt-in path close to Off.

## Captures

Every image includes the FPS performance overlay. The names are `<mode>-<scene>.png`:

- [Off — normal](screenshots/off-entity_normal.png), [wall](screenshots/off-entity_wall.png), [wall slit](screenshots/off-entity_wall_slit.png), [stress](screenshots/off-entity_stress.png), [stress slit](screenshots/off-entity_stress_slit.png)
- [Sync — normal](screenshots/sync-entity_normal.png), [wall](screenshots/sync-entity_wall.png), [wall slit](screenshots/sync-entity_wall_slit.png), [stress](screenshots/sync-entity_stress.png), [stress slit](screenshots/sync-entity_stress_slit.png)
- [Async — normal](screenshots/async-entity_normal.png), [wall](screenshots/async-entity_wall.png), [wall slit](screenshots/async-entity_wall_slit.png), [stress](screenshots/async-entity_stress.png), [stress slit](screenshots/async-entity_stress_slit.png)

## Reproduction

From the repository root, with prepared 26.3 resources available:

```sh
python3 tools/entity_occlusion_bench.py
```

The script uses fresh temporary game data, the release binary in `target/release`, OpenGL, the FPS overlay, and writes JSON plus captures under `/tmp/horizon-entity-occlusion-release-quick/`.
