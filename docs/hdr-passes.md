# HDR: what a render pass keeps

Read this before adding or changing anything that reads, filters or redraws the world's picture: upscaling (FSR,
DLSS in a native mod), anti-aliasing and TAA, sharpening, frame generation, post effects and screen-space effects,
and anything that captures the picture (screenshots, recordings, streams). With HDR the frame holds light above
white, and a pass written for SDR alone breaks it, mostly without any test failing. HDR's work is tracked in
[#12989](https://github.com/horizon-rust/horizon/issues/12989); the design behind it is
[design/hdr-linear-light.md](design/hdr-linear-light.md), and what players see is in [graphics.md](graphics.md).

## The frame

| | Frame | Holds |
| --- | --- | --- |
| `HORIZON_HDR_FRAME=float` under a 0.2 pack, or with HDR on and no pack where the device can keep the HUD on a layer of its own (the only HDR frame once HDR is on by default) | `Rgba16Float` | light: 1.0 is paper white, more is brighter |
| HDR off, otherwise | the swapchain's 8-bit format | vanilla's picture, clipped at 1 |
| HDR on, otherwise | `Rgba16Unorm`, or 8-bit | the picture clipped at 1, with HDR's marks in alpha |

On the float frame Horizon's own light reaches several times paper white, the sun the most, and a shader pack of
API 0.2 may write any value half float holds. It's stored in vanilla's gamma (the 2.2 power), as the other frames are.
A 0.2 pack's `[frame] blend = "linear"` is to store linear light instead (it passes the check, but isn't served yet),
so a pass doesn't hard-code the gamma.

## Where a pass goes

Each frame without a shader pack (`Renderer::render` in `gfx.rs`), in order:

1. The world.
2. `world_post` (`gfx/world_post.rs`): resolution scale, FXAA or TAA, sharpening. An upscaler goes here.
3. Post effects: vanilla's post chains, entity outlines.
4. The tone curve (`Hdr::tonemap` in `gfx/hdr.rs`): the light eases into the display's peak.
5. Camera overlays: vignette, pumpkin, powder snow, portal, nausea.
6. The HUD and menus, on an 8-bit layer of their own, with how much of the world they cover.
7. HDR output's last pass (`hdr.wgsl`): the HUD over the world, encoded for the display (PQ or scRGB). The HDR10
   swapchain's 10 bits get dither there, noise of one code fixed to the pixel, so slow gradients such as a clear sky
   don't band; a capture's HDR10 copy (the `-hdr.png`) keeps the exact codes.

Under a shader pack its post passes take step 2's place. A 0.2 pack's `[tonemap]` runs at their end and takes step
4's, so the post effects after it get a tone-mapped picture.

A pass on the world's light belongs in steps 2 and 3, before the tone curve, and never sees the HUD. After the tone
curve the frame is what the display will show, so no pass there treats it as light. Frame generation interpolates
the world without the hand and the HUD, and is off with HDR output until #12989 makes the two work together.

## Rules

1. **Keep light above 1.** No `saturate`, `clamp(c, 0, 1)` or `min(c, 1)` on a colour, and no unorm target in
   between: what a pass keeps (history, mips, pyramids) is a float format, as TAA's `Rgba16Float` history is.
   Clipping cuts the sun and every light source off at paper white.
2. **Don't assume a top of 1.** Limits written for 0..1 misbehave above it: the sharpening in `world_post.wgsl`
   (`2.0 - hi`) eases off above 1 and stops at 2. Scale by the pixels' own range instead.
3. **Blend frames tone-weighted.** TAA, a temporal upscaler and anything else that keeps a history blends
   `c / (1 + m)`, where `m` is the largest of `c`'s channels, clamps the history in that space too, and turns the
   result back with `c / (1 - m)`. Raw light lets a few very bright pixels win the history: torches and the sun's
   edge sparkle, and smear when the camera moves.
4. **Follow a library's colour contract.** Temporal upscalers such as FSR 2 and 3 and DLSS take HDR input as
   linear light: tell them it's HDR, give them 1.0 at paper white (exposure 1), and on a gamma frame decode the 2.2
   power before and encode it again after. Any other library goes where its documentation says its input must come from, and keeps
   these rules.
5. **Write no NaN, infinity or negative values.** Half float's largest value is 65504, and a history keeps a NaN
   and spreads it.
6. **Leave alpha alone.** Return each pixel's own alpha; a filter returns its centre pixel's. The 16-bit HDR frame
   marks light sources in alpha, and a filtered alpha moves a pixel into another mark's band.
7. **Drop history when the frame changes.** Its format and encoding change with HDR, Smooth Gradients and the
   shader pack, and blending across the change mixes two meanings.
8. **Captures map the picture.** A screenshot, recording or stream takes the world through HDR's mapping to SDR, as
   `Hdr::sdr_screenshot` does, or as HDR10, as the `-hdr.png` does; never the frame's values as they are.

## Checking a pass

- **HDR off stays byte-identical** unless the pass means to change it:
  `python3 tools/test_frame_identity_parity.py --base origin/main`, and the `frame-identity` label
  ([release-flow.md](release-flow.md)). It also draws the float frame with HDR on, and holds it to SDR.
- **Its sessions run with anti-aliasing and frame generation off,** as by default: TAA and frame generation build
  on earlier frames, which `HORIZON_STILL` doesn't repeat. Check a temporal pass by hand: offscreen with HDR on
  (`hdr:true` in `horizon-graphics.txt`), `HORIZON_HDR_FRAME=float` and `HORIZON_HDR_OUTPUT=pq`, with the camera
  moving over torches, lava and the sun.
- **The `-hdr.png` beside an HDR screenshot** (F2, or `horizon-ctl screenshot`) keeps the whole picture, and its
  `cLLI` chunk gives its brightest pixel in nits. With the sun in view offscreen it is near the peak, 1000 nits; a
  pass that clips brings it down to paper white, 203.
- **Mention #12989 in the pull request**, so HDR's side gets reviewed.
