# Entity name-tag layout cache

Unchanged entity names used to repeat font width, style/bidi processing and glyph expansion
on every frame, separately for the normal and see-through passes. Retain only their local
HUD quads. Text/style, pass alpha/background, vertical text offset, atlas revision, font epoch
and bidi direction determine reuse. World placement, camera orientation, lighting, cosmetic
marks and visibility/hooks still update each frame. This does not change TextDisplay's existing
line cache or the player-list/skin paths.

Obfuscated text (including legacy §k) and inline objects always use the original preparation.
Keep at most 512 lines, each at most 512 HUD vertices and 1024 text bytes; larger text and
excess lines still draw through the original path. Slots follow submission order and compare
complete styled text, so reordered/removed entities, scores and teams cannot reuse another
name. Unused slots are dropped at the frame end. No quality setting is needed.

## Measurement

Base `2b74332fb`, Linux/Vulkan/X11, Ryzen 5 7500F, RTX 4060, 1920×1080,
render distance 4, VSync off/unlimited FPS, OIT off. Fresh isolated homes and game folders,
same copied save, frozen server ticks, fixed time/weather and camera. No other game or
build ran during the measurements; existing desktop apps stayed running and power settings
were unchanged. A/B/B/A/B/A/A/B launch order, 3 s warmup then 8 s measured per scene:
four pairs, 24 samples including the empty control. This isolates rendering of static names;
it is not a general gameplay or low-FPS/GPU-bound benchmark.

100 invisible marker armor stands, unique gold bold `Rank NNN Player` custom names,
10×10 grid with 1.5-block spacing at y=81. Camera feet (0.5,80,-9.5), pitch 8°;
yaw 0° faces them, yaw 180° faces away. Chat visibility is 2. Normal HUD remains enabled,
since hiding it also hides name tags. The empty control removes only entity regions from
a scratch copy of the same save. Values below are medians of four runs.

| Scene | Baseline FPS | Cached FPS | Paired gain (approx. 95% t interval) | Prepare ms, before → after |
|---|---:|---:|---:|---:|
| front | 1096 | 1746 | +56.2% [+35.1, +80.5] | 0.701 → 0.369 |
| behind | 1172 | 2020 | +72.7% [+29.3, +130.7] | 0.647 → 0.285 |
| empty | 4319 | 4385 | -0.5% [-7.6, +7.1] | 0.091 → 0.091 |

The interval uses paired log FPS ratios (four pairs, t critical 3.182), not individual frames
as independent samples. The empty control does not establish an FPS improvement. Gains
apply to many unchanged names; changing/animated text and GPU-bound scenes may benefit less.
[Raw samples and summaries](name-tag-layout-cache-samples.json) include CPU/GPU and 1% lows.

## Correctness

Five independent 1280×720 image comparisons against the baseline had zero changed pixels:
plain/styled/legacy/Arabic names with below-name scores and a team; live rename/style change;
score/team update; camera translation; force-Unicode font option. These were four separated
invisible marker stands, not a pile of overlapping models. Unit tests additionally compare
cached/fresh vertex bytes after text/style/pass changes and a font reset that moves glyph UVs,
check that §k keeps animating with seeded fresh-path comparisons, and update a live inline
sprite without replacing the atlas. The ordinary rendering path and its visibility remain
the reference; this changes preparation reuse, not vanilla name rules.

The dense front-view stress fixture has two draw orders when entity chunks arrive in a
different order. Three paired images differ in 1377 pixels where names overlap. An additional
unmodified-baseline launch reproduced the candidate's alternate image exactly (0 differences),
while two baseline launches reproduced the first order. This is existing overlap compositing,
not a pixel oracle; the five separated-name scenes above avoid it.
