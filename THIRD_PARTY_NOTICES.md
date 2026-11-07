# Third-party notices

## Code removed for its license

Horizon once shipped three pieces of code under MIT that came from projects whose licenses don't allow that. All
three are gone from main. They are listed here for anyone who holds an older commit, tag or release: in those
versions, that code is not covered by Horizon's MIT license. It stays under its authors' copyright and its original
license.

- **Shader packs written from Iris's source.** `crates/horizon-shaderpack` and the render stages that used it were
  written from the source of [Iris](https://github.com/IrisShaders/Iris) (LGPL-3.0, copyright the Iris contributors).
  They were added from 15 October 2026 (`b22583d057`), shipped in the game releases `v2026.10.16.02` to `v0.2.46`,
  and were removed on 24 October 2026 ([#1561](https://github.com/horizon-rust/horizon/pull/1561)) after a developer
  of Iris showed us. Shader packs came back in `crates/horizon-shaders`, first written from a description of the
  pack format.
- **Math functions translated from OpenJDK.** In `crates/horizon-core/src/jmath.rs`, `fdlibm::{log, log10, cbrt,
  hypot}` were translated from OpenJDK's `java/lang/FdLibm.java` and `round_f32` from its `java/lang/Math.java`
  (GPL-2.0 with the Classpath Exception), and `aarch64::{sin, cos}` from HotSpot's `macroAssembler_aarch64_trig.cpp`
  (GPL-2.0); copyright Oracle and/or its affiliates, and Cavium for that file. The `fdlibm` and `aarch64` functions
  were added on 13 October 2026 (`558d140fca`, first released in `v2026.10.13`). `round_f32` was added on 14 October
  2026 as `java_round` in `crates/horizon-client/src/mipmap.rs` (`95db0fce9d`) and moved into `jmath.rs` on 3
  November ([#7770](https://github.com/horizon-rust/horizon/pull/7770)). All of them were replaced in
  [#11944](https://github.com/horizon-rust/horizon/pull/11944) by code written without OpenJDK's sources, from Sun's
  fdlibm C sources, the Java SE API documentation and a description of the behaviour, and checked against a JVM
  ([#6741](https://github.com/horizon-rust/horizon/issues/6741)).
- **Java's `HashMap` translated from OpenJDK.** `crates/horizon-world/src/java_hash_map.rs`, which reproduces the
  iteration order of `java.util.HashMap`, was translated from OpenJDK's `java/util/HashMap.java` (GPL-2.0 with the
  Classpath Exception; copyright Oracle and/or its affiliates). It was added on 7 October 2026 (`118e93ea51` and
  `194db0dbb4`, first released in `v2026.10.12`). It was replaced in
  [#12043](https://github.com/horizon-rust/horizon/pull/12043) by code written without OpenJDK's sources, from a
  description of the behaviour, the Java SE API documentation and the textbook red-black tree, and checked against a
  JVM ([#6741](https://github.com/horizon-rust/horizon/issues/6741)).

## Catnip / Ponder

`crates/horizon-client-render/src/fluid_render.rs` translates the still-fluid face tiling and
UV rules of Catnip's `FluidRenderHelper` from Ponder 1.0.82 for Minecraft 1.21.1
(`net.createmod.ponder:ponder-neoforge:1.0.82+mc1.21.1`). Copyright (c) 2022 The Create Team,
licensed under MIT (`LICENSE_Ponder` in that artifact); the full license heads the Rust file.
The native renderer uses Horizon's own block-sprite arrays, pose and light packing. The
development-only oracle calls the unchanged helper from the player's local artifact. No
original classes, game shaders, textures or prepared resources are distributed here.

## wgpu-hal

`third_party/wgpu-hal` is [wgpu-hal](https://github.com/gfx-rs/wgpu) 30.0.1 by the gfx-rs
developers, with upstream fixes applied
([gfx-rs/wgpu#10124](https://github.com/gfx-rs/wgpu/pull/10124),
[gfx-rs/wgpu#10714](https://github.com/gfx-rs/wgpu/pull/10714)). It is licensed under MIT or
Apache-2.0; both licenses are in that folder (`LICENSE.MIT`, `LICENSE.APACHE`).

## naga

`third_party/naga` is [naga](https://github.com/gfx-rs/wgpu) 30.0.1 by the gfx-rs developers, with
fixes to its GLSL frontend that shader packs need, each applied in the commit that needed it. It is licensed under MIT or Apache-2.0; both licenses
are in that folder (`LICENSE.MIT`, `LICENSE.APACHE`).

The GLSL frontend also resets the common type for each comma-separated declarator so one
variable's array specifier does not change later variables.
It also reads a struct member outside a constant expression through a pointer to the member,
not out of a loaded copy of the whole struct, so a member an atomic function uses can be read.

## libmimalloc-sys and mimalloc

`third_party/libmimalloc-sys` is [libmimalloc-sys](https://github.com/purpleprotocol/mimalloc_rust)
0.1.49 by Octavian Oncescu, with the [mimalloc](https://github.com/microsoft/mimalloc) v3 sources it
bundles (Copyright (c) 2018-2025 Microsoft Corporation, Daan Leijen), both under the MIT license
(`LICENSE.txt` and `c_src/mimalloc/v3/LICENSE` in that folder). It holds only the v3 sources. One
change: `mi_popcount` in `include/mimalloc/bits.h` uses mimalloc's portable bit count on MSVC
x86/x64 instead of the POPCNT instruction, which CPUs before Nehalem lack (#7264).

## Pixelify Sans

`crates/horizon-theme/fonts/PixelifySans-*.ttf` is [Pixelify Sans](https://github.com/eifetx/Pixelify-Sans)
by The Pixelify Sans Project Authors, licensed under the SIL Open Font License 1.1
(`crates/horizon-theme/fonts/OFL-PixelifySans.txt`).

## Pixelarticons

`crates/horizon-theme/icons/pixel/*.svg` are icons from
[Pixelarticons](https://github.com/halfmage/pixelarticons) by Gerrit Halfmann, under the MIT
license (`crates/horizon-theme/icons/pixel/LICENSE-pixelarticons.txt`). `star.svg` is Horizon's own, drawn on
the same grid.

## Hugeicons

`crates/horizon-theme/icons/glass/*.svg` are icons from the free set of
[Hugeicons](https://github.com/hugeicons/hugeicons) (`@hugeicons/core-free-icons` 4.4.0), under
the MIT license (`crates/horizon-theme/icons/glass/LICENSE-hugeicons.txt`), exported with a 1.6
stroke in `currentColor`.

`crates/horizon-theme/icons/glass/discord.svg` is Discord's logo as drawn by
[Simple Icons](https://github.com/simple-icons/simple-icons) 13.21.0 (CC0 1.0). The logo is Discord's
trademark, used only on the button that opens Horizon's Discord server.

## AMD FidelityFX SDK (FSR 3.1 frame generation)

`crates/horizon-client-render/src/gfx/frame_gen/optical_flow.wgsl` and `interpolation.wgsl` are
ports to WGSL of the optical flow and frame interpolation shaders of the
[AMD FidelityFX SDK](https://github.com/GPUOpen-LibrariesAndSDKs/FidelityFX-SDK) 1.1.4
(`sdk/include/FidelityFX/gpu/{opticalflow,frameinterpolation}`, at commit
[c6efa6bf7f20](https://github.com/GPUOpen-LibrariesAndSDKs/FidelityFX-SDK/tree/c6efa6bf7f2027b3ec94f28578bb5965eabb9e55)),
and `optical_flow.rs` follows the SDK's `sdk/src/components/opticalflow/ffx_opticalflow.cpp`
(its passes, their order and their dispatch sizes). Copyright (C) 2024 Advanced Micro Devices,
Inc., under the MIT license, whose text heads each file. AMD and FidelityFX are AMD's
trademarks; Horizon is not affiliated with AMD.

## Mod loader logos

The launcher shows the logo of the mod loader an instance plays with, and offers the logos as
instance icons.

- `launcher/assets/logos/{fabric,quilt,neoforge,forge}.png` are the loaders' logos as the launcher
  mockup on [playhorizon.dev/launcher](https://playhorizon.dev/launcher) shows them (horizon-site
  `public/media/launcher/*.webp`, converted to PNG), each its project's own mark:
  [Fabric](https://fabricmc.net/), [Quilt](https://quiltmc.org/) (its art is
  [CC0 1.0](https://creativecommons.org/publicdomain/zero/1.0/)),
  [NeoForged](https://neoforged.net/) (its art, by Ridanisaurus, is
  [CC BY 4.0](https://creativecommons.org/licenses/by/4.0/)) and
  [Minecraft Forge](https://minecraftforge.net/). They name the loader an instance runs.

Each logo belongs to its project. Showing it says which loader an instance runs; it does not mean
the project endorses Horizon.

## Sign in with Microsoft

The launcher's sign-in buttons follow Microsoft's published branding guidelines for apps that sign in
with Microsoft accounts
([Sign in with Microsoft branding](https://learn.microsoft.com/en-us/entra/identity-platform/howto-add-branding-in-apps)).
The four-square Microsoft symbol in `launcher/src/microsoft.rs` reproduces the squares, positions and
colours of Microsoft's official `ms-symbollockup_mssymbol_19.svg` from that page, unmodified. Microsoft
and its logo are trademarks of the Microsoft group of companies; Horizon is not endorsed by Microsoft.

## CIT Resewn

`crates/horizon-optifine/src/cit.rs` and `crates/horizon-client-render/src/cit.rs` follow the
condition semantics of [CIT Resewn Continuation](https://github.com/EthanVisagie/CIT-Resewn-Continuation-1.21.11)
(at `b792f9a`), a continuation of [CIT Resewn](https://github.com/SHsuperCM/CITResewn) by SHsuperCM: the
1.21 component paths, enchantment levels, percent damage, text matching and rule ordering. It is
licensed under the MIT license:

> Copyright (c) 2021 SHsuperCM
>
> Permission is hereby granted, free of charge, to any person obtaining a copy of this software
> and associated documentation files (the "Software"), to deal in the Software without
> restriction, including without limitation the rights to use, copy, modify, merge, publish,
> distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the
> Software is furnished to do so, subject to the following conditions:
>
> The above copyright notice and this permission notice shall be included in all copies or
> substantial portions of the Software.
>
> THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING
> BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
> NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM,
> DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
> OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

## FusionExamples

The tile tables in `crates/horizon-optifine/src/fusion/layout.rs` were read off the connecting
texture templates (`templates/template <layout>.png`) of
[FusionExamples](https://github.com/SuperMartijn642/FusionExamples) (at `8c9c56e`), which is
licensed under the MIT license:

> Copyright (c) 2024 Martijn van den Brand
>
> Permission is hereby granted, free of charge, to any person obtaining a copy of this software
> and associated documentation files (the "Software"), to deal in the Software without
> restriction, including without limitation the rights to use, copy, modify, merge, publish,
> distribute, sublicense, and/or sell copies of the Software, and to permit persons to whom the
> Software is furnished to do so, subject to the following conditions:
>
> The above copyright notice and this permission notice shall be included in all copies or
> substantial portions of the Software.
>
> THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING
> BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
> NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM,
> DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
> OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

## BlueMap

`crates/horizon-client-render/src/bluemap.rs` reads the lowres tiles of a server's
[BlueMap](https://github.com/BlueMap-Minecraft/BlueMap) web map. It follows three of BlueMap's files:
`core/src/main/java/de/bluecolored/bluemap/core/map/lowres/LowresLayer.java` (`saveTile`) for the
tiles' pixel layout, `common/webapp/src/js/util/Utils.js` (`pathFromCoords`) for their paths, and
`core/src/main/java/de/bluecolored/bluemap/core/map/MapSettingsSerializer.java` for a map's
`settings.json`, at commit
[96ae320d1d39](https://github.com/BlueMap-Minecraft/BlueMap/tree/96ae320d1d39581c10058d8222f2290f2fcb5333).
BlueMap is licensed under the MIT license:

> Copyright (c) Blue (Lukas Rieger) <https://bluecolored.de>
> Copyright (c) contributors
>
> Permission is hereby granted, free of charge, to any person obtaining a copy
> of this software and associated documentation files (the "Software"), to deal
> in the Software without restriction, including without limitation the rights
> to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
> copies of the Software, and to permit persons to whom the Software is
> furnished to do so, subject to the following conditions:
>
> The above copyright notice and this permission notice shall be included in
> all copies or substantial portions of the Software.
>
> THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
> IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
> FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
> AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
> LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
> OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
> THE SOFTWARE.

## JOML

`crates/horizon-client-render/src/camera.rs` (`joml::Mat4::rotation`, `invert_orthonormal`) and
`crates/horizon-client-render/src/gfx/pack_uniforms.rs` (`invert_generic`, `invert_affine`, the
formulas of `pack_projection` and `shadow_ortho`) adapt the float arithmetic of
`Matrix4f.rotation(Quaternionfc)`, `invertOrthonormal`, `invertGenericNonThis`, `invertAffine`,
`setPerspective` and `setOrtho` from
[JOML](https://github.com/JOML-CI/JOML/tree/faf9eef2511242122beb6055fe3da92daed214c5) 1.10.9, the
library vanilla 26.3 uses, so that shader packs get the same matrices to the last bit. Changes:
ported to Rust; the pack_uniforms.rs functions drop JOML's property flags, and
`pack_projection` works the formula in doubles. The copyright line and license notice also head
both files.

`camera.rs` also adapts `Quaternionf.rotationX`, `rotationY` and `rotationZ`,
`Matrix4f.rotateAffine` and `Vector4f.mulAffine` (`joml::Quat::rotation_x`/`_y`/`_z`,
`joml::Mat4::rotate_affine`, `transform_affine`), for the sky's positions shader packs read.
Changes: ported to Rust; `rotate_affine` keeps only the affine case.

```
The MIT License

Copyright (c) 2015-2026 Richard Greenlees

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in
all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
THE SOFTWARE.
```

## Just Enough Items

The Recipes GUI sprites and settings icon under `crates/horizon-client-render/src/jei_recipes` are from
[Just Enough Items 31.17.0.108](https://github.com/mezz/JustEnoughItems/tree/0371fa6fafa14635a356019e4604f5580b26a34f)
by mezz and contributors, under the MIT license in that directory (`LICENSE.txt`).

## Flywheel native model directional shading

The two-light diffuse expression and transformed-normal policy in
`terrain.wgsl`'s native model entries follow Flywheel 1.0.6
(`internal/diffuse.glsl`, `instance/transformed.vert`, `internal/common.vert`).
No Flywheel classes, compiled shaders or original model/texture assets are bundled.

Copyright (c) 2021-2024 Jozufozu

Permission is hereby granted, free of charge, to any person obtaining
a copy of this software and associated documentation files (the
"Software"), to deal in the Software without restriction, including
without limitation the rights to use, copy, modify, merge, publish,
distribute, sublicense, and/or sell copies of the Software, and to
permit persons to whom the Software is furnished to do so, subject to
the following conditions:

The above copyright notice and this permission notice shall be
included in all copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND,
EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF
MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND
NONINFRINGEMENT. IN NO EVENT SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE
LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION
WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE SOFTWARE.

## fdlibm

`crates/horizon-core/src/jmath/fdlibm.rs` and the `fdlibm_asin`, `fdlibm_atan` and
`fdlibm_atan2` functions in `crates/horizon-core/src/geometry/trig.rs` are Rust ports of
[fdlibm](https://www.netlib.org/fdlibm/) 5.3, Sun's freely distributable math library, whose
results the Java SE API specifies for `StrictMath`. `fdlibm.rs` ports `e_log.c`, `e_log10.c`,
`s_cbrt.c` and `e_hypot.c`, and, with fused multiply-adds, `s_sin.c`, `s_cos.c`, `k_sin.c`,
`k_cos.c` and `e_rem_pio2.c`; `trig.rs` ports `e_asin.c`, `s_atan.c` and `e_atan2.c`. This
notice also heads both files:

```
Copyright (C) 1993 by Sun Microsystems, Inc. All rights reserved.

Developed at SunSoft, a Sun Microsystems, Inc. business.
Permission to use, copy, modify, and distribute this
software is freely granted, provided that this notice
is preserved.
```
