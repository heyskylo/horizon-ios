# Entity blob-shadow visibility

Blob shadows used to scan blocks, collision bounds and light for every nearby caster, including shadows entirely outside the camera frustum. The new check uses the shadow's projected bounds rather than its caster's model: a caster behind the camera can still project a visible shadow. The existing radius/strength/distance gates run before the frustum test. Shader packs keep the original path; native mods do not disable the optimization.

This changes no quality setting or visible shadow geometry, and needs no new toggle. The benefit depends on nearby shadow casters outside the view; it is not a general FPS multiplier.

## Setup

Ryzen 5 7500F, RTX 4060, NVIDIA 615.78.08, Linux x86_64, Vulkan/X11, Rust 1.99.1, optimized `fast` profile. Baseline: `c4e4800d4`. Both builds used 1920×1080, render distance 4, VSync off, unlimited FPS and entity shadows on. Shader packs, native mods and LOD were off. Desktop applications stayed running; no power/governor/clock settings were changed.

Each comparison uses four adjacent process pairs, in the order baseline/candidate, candidate/baseline, candidate/baseline, baseline/candidate. Each sample lasts eight seconds after warmup. Every launch uses fresh homes and a fresh game folder copied from one saved fixture. No builds or other game instances run during measurements. The empty control copies the same terrain/time without its entity-region files, avoiding death particles and dropped items.

Raw samples and binary SHA-256 hashes: [measurement data](entity-shadow-culling-samples.json). The tables show medians across four launches per build; paired change is the geometric mean of adjacent FPS ratios, with an approximate two-sided t interval on four log ratios. Median ratios and paired ratios need not match.

## Dense 400-cow stress fixture

Time is frozen; cows have `NoAI` and `NoGravity`. A 33×33 stone platform has its top at y=80. The player stands at (0.5,80,-9.5), pitch 15°, with yaw 180° (behind), 0° (front) or 55° (edge). The fixture contains overlapping cows, including duplicate positions: it intentionally stresses per-caster work rather than representing a typical survival farm.

### OIT off

| View | Baseline FPS | Candidate FPS | Paired change (approx. 95% interval) | CPU ms, baseline → candidate |
|---|---:|---:|---:|---:|
| behind | 2765 | 4674 | +73.2% (+57.9…+90.1%) | 0.299 → 0.124 |
| front | 993 | 966 | -1.2% (-6.0…+3.7%) | 0.947 → 0.970 |
| edge | 1168 | 1240 | +6.3% (-0.2…+13.2%) | 0.795 → 0.748 |
| empty | 4608 | 4704 | +1.5% (-2.7…+5.8%) | 0.084 → 0.081 |

### OIT on

| View | Baseline FPS | Candidate FPS | Paired change (approx. 95% interval) | CPU ms, baseline → candidate |
|---|---:|---:|---:|---:|
| behind | 1502 | 1699 | +5.8% (-8.3…+22.1%) | 0.326 → 0.146 |
| front | 966 | 973 | +1.8% (-5.3…+9.4%) | 0.981 → 0.975 |
| edge | 1145 | 1246 | +6.7% (+1.5…+12.1%) | 0.814 → 0.755 |
| empty | 1490 | 1642 | +8.5% (-2.8…+21.1%) | 0.111 → 0.105 |

The OIT-off behind-camera result establishes a throughput gain in this CPU-bound stress case. Its 1% lows did not improve (median 751 → 736 FPS). With OIT on, GPU/launch variation was substantial even in the empty control; these runs do not establish an isolated behind-camera FPS gain despite the CPU saving. The edge result should also be read alongside that control, rather than treated as a general OIT speedup.

## Spaced 100-cow farm

One saved fixture holds 100 cows on a 10×10 grid with 1.1-block spacing (`NoAI`, `NoGravity`), with natural spawning disabled and entity cramming disabled. The player faces away from the farm at (0.5,80,-2.5), yaw 180°, pitch 15°. OIT is off. Each launch first measures the frozen scene, then unfreezes the game, warms up for five seconds and measures normal simulation. Population is checked before and after each sample.

| Simulation | Baseline FPS | Candidate FPS | Paired change (approx. 95% interval) | CPU ms, baseline → candidate |
|---|---:|---:|---:|---:|
| frozen | 4399 | 4589 | +6.5% (-1.4…+15.0%) | 0.147 → 0.095 |
| live | 4213 | 4655 | +9.9% (+3.4…+16.7%) | 0.155 → 0.098 |

## Image and geometry checks

- Nine resource-backed shadow tests pass, including the new bounds check: every generated vertex stays inside the tested bounds on stepped terrain, fractional positions, varying strength and radii up to/beyond the existing 32-block clamp.
- A separate scene places an empty text display at (0,80,-12), shadow radius 4, strength 5, behind the player at (0.5,80,-9.5). Four views (toward the projected shadow, edge, upward, straight down) match the baseline exactly: zero changed pixels. Disabling shadows in the baseline changes 619,420 / 507,639 / 0 / 921,600 pixels respectively, confirming that the test retains substantial visible shadow geometry.
- All dense-fixture behind-camera and empty-control image pairs match exactly. Its front/edge images have two states even between unmodified baseline launches; the model-region differences persist with entity shadows disabled. Those images are not used as a pixel-exact oracle for this change.
- Windows, macOS, other GPUs and shader-pack paths were not measured. Shader-pack paths bypass the new check.

The resource-backed test can be repeated with your own prepared resources:

```sh
HORIZON_RESOURCES=/path/to/prepared/26.3 cargo test --profile fast -p horizon-client-lib --lib shadows::tests::
```

To reproduce FPS measurements, build the baseline and candidate with the same optimized profile, create the fixtures above through `horizon-ctl command`, save once and copy that save for each launch. Keep all game/home directories isolated as described in `AGENTS.md`. Use `options-set entityShadows true`, select `improvedTransparency` identically in both builds, and collect `bench 8` after warmup. For image comparisons use frozen time and the separate single-display fixture, rather than overlapping model surfaces.

A follow-up compatibility run registered one native trait mod through `entry::main_with`.
Its camera hook asserted that the mod was loaded and observed the camera without changing it.
All four single-display views matched the updated ordinary executable exactly, including the
619,420 visible shadow pixels from the caster behind the camera. Native mods no longer cause
a blanket bypass. The original FPS samples above were collected without mods; this checks
rendering compatibility rather than arbitrary mixins that rewrite shadow geometry or passes.
