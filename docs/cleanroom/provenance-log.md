# Provenance log: shader pack specification, revision 1

> Published on 2026-11-08 as the record of the shader-pack clean room (#6739, #1555). The text is as written in
> October 2026, except that local paths, machine names and the names of private files are replaced by a
> description of what they were. Nothing else was changed.

What the spec team read to write [shader-packs-spec.md](shader-packs-spec.md), and nothing else. Section 11 of the
spec maps each section to these sources.

Session: one AI agent (Claude Opus 5.5) working for Ludovic, 2026-10-24, on the maintainer's machine.
Downloads went to a scratch folder outside every repository; they are not kept here.

## 1. Documents fetched

| Source | How | Revision | SHA-256 of the fetched file |
|---|---|---|---|
| OptiFine `OptiFineDoc/doc/shaders.txt` | raw.githubusercontent.com, `sp614x/optifine` `master` | `07ec2ca62f81aa52cb9ed1a7fd49a4489c131b1e` | `159455dc6ab331acebd035c3322e07271faed65ef767e4175b7b7c645aa4cb86` |
| OptiFine `OptiFineDoc/doc/shaders.properties` | same | same | `a0bd75f21932b47f6b4303bf4590e86b2a3a79b302c4a55852d687bf25536f46` |
| OptiFine `OptiFineDoc/doc/properties_files.txt` | same | same | `74fe689551673f5c4a12e8962252a8011d869fdbd289c9d4f0da8e6951a079b3` |
| OptiFine `OptiFineDoc/doc/texture.properties` | same | same | `bead5ccacafae6ac4de5f7176ad979f3def77051684a7656ef27764d80de106d` |
| OptiFine `OptiFineDoc/doc/block.properties` (resource-pack feature, read and not used) | same | same | `813f8562e1713e9dfd96405bf9fa782be8edc3dfb26d9cce22f496f601d17118` |
| Documentation site `shaders.properties`: the 119 `.mdx` pages under `src/content/docs/current/` | raw.githubusercontent.com, `IrisShaders/docs` `main` | `98f84cbe456e7e367afe788885869be46681be8c` | per-page hashes not taken; the commit pins them |
| ShaderDoc: `uniforms.md`, `const-directives.md`, `iris-features.md`, `vertex-format-extensions.md`, `passes/composite.md`, `unsupported-features.md`, `directives/properties.md`, `README.md` | raw.githubusercontent.com, `IrisShaders/ShaderDoc` `master` | `ddf473726710c6504422d192784c1652f8aece98` | `6e8b73a9…` (uniforms), `27a9c7c6…` (const-directives), `a9a8ca5f…` (iris-features), `b739b06a…` (vertex-format-extensions), `f6e1fd4d…` (composite), `e81a07e1…` (unsupported), `7709c3a9…` (properties), `78ab2517…` (README) |
| shaderLABS wiki: Rendering Pipeline (OptiFine, ShadersMod); Shader Stages; Compiler Behavior Notes; LabPBR Material Standard; Legacy Shaderpacks; Main Page (licence) | `index.php?action=raw` | as of 2026-10-24 | `c8e66b61…`, `fe432020…`, `bb841334…`, `294be212…`, `e008b31f…` |
| OptiDocs uniform page | optifine.readthedocs.io, HTML | renders OptiFine doc commit `83d482c` | not kept (only its list of uniform names was compared with `shaders.txt`) |
| GLSL 4.60.8 specification | registry.khronos.org, HTML | 4.60.8 | `795e42c5f71eb709a23035228cd4b328a7cae23766ebd0bbfc6b2705028fe1a5` |
| `gl.xml` | `KhronosGroup/OpenGL-Registry` `main` | `6af574a14089ccfee87efe230ebcdd8742859813` | enum values only |
| LearnOpenGL "About" page | learnopengl.com | 2026-10-24 | licence statement only |

Web searches: two (`shaders.properties` documentation; `MC_VERSION` for 26.1). See incidents.

## 2. Pack files read (non-code only)

From local copies of the BSL, Complementary and Sildur's Vibrant packs (their `shaders/` folders): `shaders.properties`,
`block.properties`, `item.properties`, `entity.properties`, `dimension.properties`, `lang/en_US.lang`, and the folder
and file-extension layout. Read with `grep`/`sed` for key names, directive forms and syntax. No `.vsh`, `.fsh`, `.gsh`,
`.csh` or `.glsl` file was opened. `complementary-unbound` (zip only) was not opened.

## 3. Deliberately not opened

- Any source or binary of Iris, OptiFine, Oculus, Canvas, Sodium or any other loader or renderer.
- Horizon's shader code and docs (`crates/horizon-shaderpack`, `java_pack.rs`, `gfx/java_stages.rs`,
  `gfx/java_terrain.rs`, `gfx/pack_stages.rs`, `shader_pack.rs`, `docs/shader-packs.md`, `docs/java-packs.md`), their git
  history and their PRs. No file of the Horizon repository was read.
- A private folder holding an earlier experiment with another loader's code (seen in a directory listing; not opened).
- Links inside the documents that lead to code: the documentation site's links to its own GLSL transformer and to the
  loader's repository (`Reference/Miscellaneous/patcher`), the tutorial's links to example pack code, ShaderDoc's link to
  LearnOpenGL's source file.

## 4. Incidents and caveats

1. **A search result summarised another project's PRs.** The second web search returned two pull requests of a
   third-party project (`cinnabar-mc/cinnabar`) about the `MC_VERSION` and `IRIS_VERSION` macros, and the search tool's
   summary quoted their content (a proposed 26.x value, a remark about another loader's macros, and one pack's reaction
   to `MC_ANISOTROPIC_FILTERING`). The pages were not opened and nothing from that summary is used. The spec's OQ-1
   derives the 26.3 value only from the documented `MC_VERSION` rule and leaves it open; `MC_ANISOTROPIC_FILTERING`
   is described only as `shaders.txt` documents it.
2. **ShaderDoc's `vertex-format-extensions.md` describes how an old loader computed some attributes.** The spec uses only
   the attribute meanings it states (normal from the quad, tangent handedness in `w`, mid-texture coordinate as the
   average of the quad's UVs) and cites the standard tangent technique to LearnOpenGL. Its description of internal code
   paths is not reproduced. Likewise `const-directives.md` mentions where an old loader hooked vanilla for
   `ambientOcclusionLevel`; the spec keeps only the documented brightness curve.
3. **Session context.** The session started with Horizon's `CLAUDE.md` loaded automatically. It names the shader crate
   and doc paths in a table (which is how the forbidden list is known) but contains no shader code. Nothing else from
   the repository was read.
4. **Model training.** The spec writer is a language model whose training data may include public source code of the
   projects above. In this session no such code was consulted, and every statement in the spec is tied to a document
   in section 1 or 2. If Ludovic considers prior training exposure disqualifying for the spec writer, the spec can be
   re-derived by a reviewer from the same sources: the tags make every statement checkable.

---

# Implementation team: `cleanroom-impl-1`

Session: one AI agent (Claude Opus 5.5) working for Ludovic, 2026-10-24, on the maintainer's machine. Its own
git worktree, branch `feat/shader-packs-discovery`, from origin/main `29c5e65d1` (after the
removal, #1561). Its implementation plan stayed with the private notes (not published).

## What was read

| Source | What for | Sections it fed |
|---|---|---|
| `shader-packs-spec.md` revision 1, `README.md` (orchestrator decisions 2026-10-24), the maintainer's private agent rules | the only description of the feature and the process | all |
| Horizon main `29c5e65d1`: `AGENTS.md`/`CLAUDE.md`, `docs/design-system.md`, `docs/benchmark.md`, `Cargo.toml`, `crates/horizon-core/src/version.rs`, `crates/horizon-client/src/{graphics.rs (tests), options_ui.rs (Page/Event/OptionsUi), zip.rs, gfx.rs (pass labels, depth compare, surface formats), gfx/world_post.rs (header)}`, `crates/horizon-client/src/benchmark.rs` (header), `crates/horizon-threads/Cargo.toml`, `crates/horizon-server-files/Cargo.toml` | where the runtime plugs in (plan 2.1, 2.4), settings (2.5), targets (1), the `MC_VERSION` source (macros.rs) | plan; 1.5.1 |
| naga 30.0.1 (crates.io), through its public API only: a scratch crate outside the repository parsing 18 GLSL snippets I wrote with `naga::front::glsl` and validating them | what naga's GLSL frontend accepts (plan 2.2) | 1.6, OQ-29 |
| GLSL 4.60 §3.3 (preprocessor operators and precedence), from knowledge of the published standard | `condition.rs` | 1.5 |
| Public test packs (local copies) as black boxes: zip listings (folder layout), and the program in `tests/real_packs.rs` reading their `shaders.properties`, `block/item/entity/dimension.properties` and `lang/`. By hand: lines 66–80 of one pack's `shaders.properties` (to see the `#define` the test reported) | parse rates and timings; the decision on `#define` in `shaders.properties` | 1.1, 3.1, 4.1, 8.4; OQ-16 |

No `.vsh`, `.fsh`, `.gsh`, `.csh` or `.glsl` file of any pack was opened.

## Seen, not opened

- Directory listings showed other worktrees of the horizon repo named `shaders-1` and similar, and `glsl-lang-*` crates in
  the local cargo registry. None was opened; nothing from them is used.
- The session context listed Horizon's old shader file names (from the crate table of a cached `CLAUDE.md`) and the memory
  note that the feature was removed after a licence complaint. No content of those files.

## Incidents

None.

### Slices 2 to 5 (PRs #1599, #1600, #1601 and the slice 5 PRs), 2026-10-24

| Source | What for | Sections it fed |
|---|---|---|
| naga 30.0.1 and wgpu 30.0.1 (crates.io): public API, docs and source of `naga::front::glsl`, `naga::valid`, `wgpu::ShaderSource::Naga`, `Device::push_error_scope`, `TextureFormat::sample_type`; scratch probes outside the repository with GLSL I wrote | which GLSL naga reads and which gaps the rewriter covers (`modf`, global initializers, unsized arrays); how the runtime builds pipelines | 1.6, 2.2, OQ-29 |
| Horizon main: `crates/horizon-client/src/{camera.rs (perspective, view_matrix, depth_far), gfx.rs (Frame, render's world target and passes, device request), gfx/world_post.rs, environment.rs (Sky), fog.rs (FogData), world.rs (light, weather levels), game.rs (Frame construction, celestial_geometry, day_time, hide_gui), game/fov.rs (camera_fluid), game/packs.rs (packs_dir)}`, `docs/benchmark.md`, `docs/horizon-ctl-commands.md` | where the runtime plugs in, reverse-Z to GL depth, sun and moon positions, frame inputs | 5.2, 5.5, 6.2 |
| Public test packs as black boxes through `tests/real_packs.rs` (preprocess every stage file, translate every composite-style program, load each dimension's post programs) and by running BSL in the game with `HORIZON_SHADER_PACK` (log lines and a screenshot, kept on this machine only) | pass rates, timings, failures to fix | 1.4–1.7, 2.1, 6.1 |

No `.vsh`, `.fsh`, `.gsh`, `.csh` or `.glsl` file of any pack was opened to read its code. Horizon's own test pack
(`crates/horizon-shaders/tests/packs/post-test`) was written from the spec alone.

**Caveat (slice 4):** naga failed on one public-pack stage file with an internal handle error that its message did not
locate. To find the construct, the test printed **one line** of that file with every identifier and number masked
(its token shape, `T x = x * ((x < N || x > N) ? N : -N);` in effect), which showed a global initializer with a
ternary; I then reproduced it with my own GLSL in a probe and fixed it in the rewriter. No other line was printed and
the shape was not kept. Flagging it so the auditor can judge.

**Bug found by the first draw (slice 5):** `gl_FragCoord` in programs drawing to buffers was never set (the translator
only set it for `final`); fixed in the slice 5 uniforms PR with a test.

**Slice 6 debugging (2026-10-25):** with the coordinator's agreement that a pack's GLSL is input data, I ran locally
modified copies of BSL (in a scratch folder, never committed) to find why its terrain was black or the whole image went black:
single lines of its lighting function, its deferred program and its TAA library were replaced with outputs of
intermediate values (split-screen colours, `isnan` checks), and I read the few lines around them to place the edits.
The causes found were all ours, fixed from the spec and GL/naga behaviour: naga dropping non-constant global
initializers, comma declarations losing their later declarators, program-enable expressions not seeing the pack's
default-on switches (8.1), and float stand-ins for normalized buffer formats keeping NaN. Complementary was only run as
a black box (its log and screenshots), plus one property line and one GLSL declaration block read to locate a translator
error. No other loader's code was read; nothing from the packs went into Horizon beyond the fixes described.


## 2026-10-25 cleanroom-impl-1, slices 21–23 (#1721–#1723)
- Sources: the spec (rev 1) §2.1, §2.3, §5.3, §5.4; Horizon's current main (`terrain.wgsl` sky, `gfx/stars.rs`, `Game::celestial_geometry`) for the sky geometry.
- Test packs used as data only: grepped single `renderStage` lines in BSL's and Complementary's sky programs to confirm that they read it per draw; no GLSL used as a model.
- No other loader, renderer or pre-removal Horizon history read.

## 2026-10-25 cleanroom-impl-1, #1742–#1744
- Sources: the spec (rev 1) §4.1, §5.1, §5.3; Horizon's current main (arena, flat draws, block models, mesher's vertex format).
- Test packs as data: their `block.properties` id lists (names only) to measure coverage; a local, uncommitted copy of Horizon's own terrain-test pack with a normal visualiser.
- No other loader, renderer or pre-removal Horizon history read.

## 2026-10-25 cleanroom-impl-1, slices 26–28 (#1754–#1756)
- Sources: the spec (rev 1) §1.6, §3.3, §3.4, §5.2, §6.1.7, §8.1; Horizon's current main (`biome.rs` biome registry, `fog.rs` fog data); Minecraft 26.3's biome list (`data/minecraft/worldgen/biome`) and its 1.18 biome renames for legacy `BIOME_` names.
- Test packs used as data, for debugging only: Complementary's fog, volumetric light, FXAA and final lines, and BSL's/Sildurs' `shaders.properties` biome and enable keys, read to locate why their output was wrong. Local, uncommitted copies with single calls switched off or values shown as colours (in a scratch folder). No GLSL used as a model; nothing from the packs went into Horizon.
- No other loader, renderer or pre-removal Horizon history read.

## 2026-10-25 cleanroom-impl-1, slices 29–31 (#1769–#1771: entity spans, alpha test, entities and block entities)
- Sources: the spec (rev 1) §2.3, §3.2 (`alphaTest.<program>`), §4.2, §5.1, §5.3, §5.4, OQ-23; Horizon's current main (`Game::entity_geometry`, `entity.wgsl` overlay codes, `overlay.rs`, `EntityBatch`); the terrain face normal and tangent code of #1744 (ours).
- Test packs used as data, for debugging only: BSL's and Complementary's `entity.properties`/`block.properties` id lists and `alphaTest.*` keys; in BSL's entity program the uniform/attribute declaration lines, the `discard`/alpha lines, and a handful of lighting lines (`NoL`, the lightmap, the `GetLighting` call) read to place a local, uncommitted debug edit that output its albedo. Complementary only as a black box. Two local test packs of ours (`entityId` colours, normal and lightmap visualisers), never committed.
- No other loader, renderer or pre-removal Horizon history read.

## 2026-10-25 cleanroom-impl-1, slices 32–34 (#1784–#1786: translucent entity parts and particles, clouds, rain and snow)
- Sources: the spec (rev 1) §2.1, §2.3, §3.2 (`separateEntityDraws`, `particles.ordering`, `particles.before.deferred`), OQ-3, OQ-23; Horizon's current main (`game/names.rs`, the entity shadow, `clouds.rs`, `weather.rs` and the weather math of `entity.wgsl`); naga's behaviour on code after `discard`/`return`.
- Test packs used as data, for debugging only: BSL's `gbuffers_clouds` program (read whole once, about 200 lines, to see why naga refused the code after its `discard` and why vanilla clouds vanish; nothing taken from it), Complementary only as a black box; screenshots of both with name tags, a campfire, clouds, rain and snow (local, never committed).
- No other loader, renderer or pre-removal Horizon history read.

## 2026-10-25 cleanroom-impl-1, slice 8 group 1 (#1885–#1888: shadow settings and camera, shadow programs, shadow buffers, shadow terrain)
- Sources: the spec (rev 1) §2.1, §2.4, §2.5, §3.2 (`shadowTerrain`, `shadowTranslucent`), §5.2, §6.1.6, §6.3, §7.1, OQ-6, OQ-19; Horizon's current main (`gfx.rs` section lists, `flat.rs`, `page_runs.rs`, `resident.rs` visibility, the translucent sorter); wgpu's docs for comparison samplers, depth copies and load/store operations; first-principles geometry for the shadow camera (look-at from the light, snapping in its own axes).
- Test packs used as data, for debugging only: BSL's `settings.glsl` shadow constants (`shadowMapResolution`, `shadowDistance`, the lines that set them, to read which values the loader should report) and its `shaders.properties` profiles; BSL, Complementary and Sildurs otherwise as black boxes (screenshots, `HORIZON_SHADER_PACK_TIMING`, the loader's notes). Two local debug packs of our own (`shadow.vsh/fsh` writing the occluder height, a `final` that tests it) checked the camera and the map.
- No other loader, renderer or pre-removal Horizon history read.

## 2026-10-25 cleanroom-impl-1, slice 8 PR 41 #1900 (entities and block entities in the shadow pass) and the shadowtex1 copy measurement
- Sources: the spec (rev 1) §2.4, §3.2, OQ-19; Horizon's current main (`pack_entities.rs`, `game/entity_culling.rs`); wgpu's docs for depth attachments, `frag_depth` and depth texture bindings (the dropped fill variant).
- Test packs used as data: `shaders.properties` lines with `shadowEntities`/`shadowBlockEntities` in BSL (under `#ifndef SHADOW_ENTITY`) and Complementary (under `ENTITY_SHADOW`), read to explain which entity shadows each pack asks for; otherwise black boxes (screenshots, timing).
- No other loader, renderer or pre-removal Horizon history read.

## 2026-10-25 cleanroom-impl-1, slice 8 PR 42 (shadow casters: culled entities, first-person player, shadowPlayer, shadowLightBlockEntities)
- Sources: the spec (rev 1) §2.4, §3.2 (`shadowPlayer`, `shadowLightBlockEntities`); Horizon's current main (`entity_culling.rs`, `game/entity_culling.rs`, `game/perspective.rs`, `Game::entity_geometry`, block state light emission in `StateFacts`).
- Test packs: not opened; only our own debug packs (shadow map and height visualisers, local, uncommitted) used for the checks.
- No other loader, renderer or pre-removal Horizon history read.
