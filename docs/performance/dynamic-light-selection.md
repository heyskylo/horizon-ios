# Streaming dynamic-light selection

With the Dynamic Lights extra enabled, source gathering previously allocated item-name strings,
an owned source list, another source vector, and a full stable sort every frame to retain 16 lights.
The candidate streams borrowed descriptions in the original player/mob/drop order and keeps the
nearest 16 in a sorted stack buffer. It evaluates distance once and reuses the output vector.
The public slice selector remains. The colored-light consumer borrows names; its selector is unchanged.

The 96-block inclusive range, level clamp, hand/fire precedence, interpolation and stable ties remain.
The existing extra stays off by default. There are no new settings or TAA/mod/shader restrictions.

## FPS method and results

Ryzen 5 7500F / RTX 4060, Linux Vulkan/X11, NVIDIA 615.78.08, optimized `fast`, 1280x720,
render distance 4, VSync/OIT off. The same copied save contains 512 frozen, NoAI/NoGravity invisible
zombies actually equipped with torches via `/item replace`; hands are checked after every load.
Grid: 32x16, spacing 0.7, y=80; player feet (0.5,80,-10.5), pitch 8, yaw 180 behind / 0 front;
midnight. Disabled controls retain the entities; empty controls have no mobs and the extra enabled.
Scratch game/home/XDG folders are isolated. Ambient particles are recorded, not removed.

There are 104 primary samples: 32 ordinary offscreen, 32 TAA offscreen, then 40 follow-ups
(8 offscreen TAA, 16 window TAA, 16 ordinary window). Each series has four adjacent A/B pairs
per scene: AB, BA, BA, AB, 3 seconds warmup and 8 seconds measured. There are no builds, tests,
traces or another game running during timing. Desktop applications and power settings are unchanged.
Window samples explicitly float and resize only the game's own Hyprland window to 1280x720;
size, camera, source count and empty streaming queues are checked before/after each sample.

Intervals below are approximate Student t 95% intervals of paired log ratios (4 pairs, df=3).
Independent frames are not treated as independent observations. The real-window result is the
main FPS claim; offscreen numbers are a separate workload, not an interchangeable estimate.

| Series | View | Median FPS A → B | Paired FPS change [95% interval] | 1% low change [95% interval] |
|---|---|---:|---:|---:|
| initial_offscreen_ordinary | behind | 4184 → 4726 | +14.9% [+2.8, +28.4] | +6.0% [-7.4, +21.4] |
| initial_offscreen_ordinary | front | 4025 → 4391 | +10.4% [+5.8, +15.3] | -1.2% [-9.2, +7.5] |
| initial_offscreen_ordinary | disabled | 5037 → 5232 | +3.3% [-0.6, +7.3] | +7.5% [-2.0, +18.0] |
| initial_offscreen_ordinary | empty | 6054 → 5953 | -1.6% [-5.1, +2.1] | +1.9% [-4.1, +8.3] |
| initial_offscreen_taa | behind | 3764 → 3760 | +1.0% [-6.4, +9.0] | -4.7% [-7.8, -1.5] |
| initial_offscreen_taa | front | 3549 → 3587 | +2.7% [-7.0, +13.3] | -7.3% [-16.6, +2.9] |
| initial_offscreen_taa | disabled | 3874 → 3970 | -0.7% [-10.5, +10.1] | -4.2% [-31.7, +34.4] |
| initial_offscreen_taa | empty | 3889 → 3908 | +0.6% [-0.7, +1.9] | -1.1% [-8.1, +6.3] |
| offscreen_taa | behind | 3569 → 3863 | +8.0% [+3.4, +12.8] | +0.8% [-14.3, +18.7] |
| window_taa | behind | 3017 → 3068 | +1.0% [-1.6, +3.8] | -2.5% [-9.0, +4.4] |
| window_taa | front | 2856 → 2914 | +1.2% [-1.1, +3.7] | -3.1% [-7.6, +1.6] |
| window_ordinary | behind | 3395 → 3612 | +6.3% [+3.9, +8.6] | +1.8% [-2.7, +6.4] |
| window_ordinary | front | 3200 → 3366 | +5.3% [+3.6, +7.0] | -0.8% [-7.3, +6.2] |
| combined_offscreen_taa | behind | 3631 → 3814 | +4.4% [+0.1, +9.0] | -2.0% [-7.9, +4.3] |

These concentrated source-gathering scenes support about 5.3–6.3% higher ordinary window FPS,
not a general game multiplier. Window TAA FPS and 1% low changes are inconclusive. Negative
estimates and disabled/empty controls are retained. A separate profile (not active during FPS)
reduced median light gathering 0.05582 → 0.03093 ms, about 45% of that CPU step.

## Investigating the initial TAA 1% low decrease

The first offscreen behind-camera TAA series decreased 1% low by 4.7% (95% interval -7.8 to -1.5),
with all four pairs negative. This prompted follow-ups rather than treating a later interval
spanning zero as proof of no regression. Repeated offscreen TAA gave +0.8% [-14.3,+18.7]; the
combined eight pairs gave -2.0% [-7.9,+4.3]. Window TAA estimates and negative pairs are above.

Temporary, identically instrumented baseline/candidate binaries localized long window frames:
`Surface::get_current_texture()` occupied about 0.69 ms of 0.83–0.84 ms in the slowest 1% of
traced renderer calls. CPU-clock probes found about 0.021 ms CPU in 0.45 ms wall time inside long
acquisitions (~95% waiting). GPU timer readback was not the dominant wait. Disabling timer reads
or changing frame-latency hints did not remove the difference in exploratory single pairs.
Disabling the submit worker lowered average FPS substantially; it is not shipped as a workaround.
At a diagnostic 240 FPS cap both versions gave about 230 FPS 1% low; acquisition p99 fell from
about 0.56–0.58 to 0.035 ms. Offscreen probes also found rare long queue submissions.

A single executable then switched original/candidate gathering within the same process, preserving
TAA history and the GPU queue. All 16 selected lights matched bit-for-bit. Window paired 1% low
changes were -2.4%, +0.5%, +2.4%, then +2.3% and +0.1% in a final ABBA run. Borrowing-only and
selection-only probes were also positive in their single pairs. Four unchanged-original blocks
gave 870, 890, 891 and 897 FPS 1% low (~3.1% range). CPU time improved with the candidate.
A deliberate 50-us CPU delay raised 1% low in one pair while lowering average FPS; 25-us padding
in an earlier pair did not restore it. Neither delay is a fix or part of the candidate.

This localizes tail sensitivity to synchronization/pacing under unlimited load and shows that the
initial decrease is not stable across controlled repeats. It does not prove a compositor/driver
bug, identify a unique cause for the original four negative pairs, or guarantee zero TAA tail cost.
No TAA FPS/1% low improvement or regression fix is claimed. Diagnostic changes, spin delays,
queue changes and FPS caps are excluded from the patch. The samples retain every diagnostic case.

## Image and behavior checks

Every comparison covers the full frame, with independent baseline repeats as controls and no pixel
mask. TAA matches the nearest baseline phase on an 8-pixel grid before comparing every channel.
A lights-off control must show >1,000 pixels brightened by >16 levels in intended lit views;
the behind-camera view is exempt. Counts below include every retained phase comparison.

- ordinary: 4 candidate / 4 repeat comparisons; maximum 8-bit channel delta 0 / 0.
- mixed_color_pack: 2 candidate / 2 repeat comparisons; maximum 8-bit channel delta 0 / 0.
- taa: 159 candidate / 160 repeat comparisons; maximum 8-bit channel delta 1 / 1.
- native_observer: 32 candidate / 32 repeat comparisons; maximum 8-bit channel delta 1 / 1.

Views include front/behind, inside the grid and third-person front; TAA at native and Performance
(2x), a mixed scene with player/mob main/offhands, a burning mob, dropped torch/glowstone/lava
bucket, an original colored-light material pack (successful load asserted), and an actual native
camera-observer mod (load and callback asserted). This does not cover arbitrary rendering mods.

The initial Performance reference missed phases (31/32 unique images in two views), causing
10/17-level unmatched differences in both candidate and baseline-repeat groups. Additional
independent baseline captures supplied 32 phases per view. All original references were retained
and unioned with the new ones; no mask/tolerance changed. Original differences and reference
coverage are retained alongside final comparisons, which have maximum channel delta 1 in both groups.

Seven focused tests include an independent full stable-sort reference over 2,048 varied sources
in both orders, growing/shrinking inputs, stable ties at exact reach, invalid coordinates and levels.
[Samples](dynamic-light-selection-samples.json) include all primary FPS summaries, negative estimates,
diagnostic summaries, actual light bits, image comparisons, reference provenance, original probe
source, measured patch and executable hashes. Raw trace/frame arrays stay in the local audit.
No copied game files, textures, prepared resources or another project's source are included.
