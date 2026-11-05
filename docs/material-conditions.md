# Numeric material conditions

Horizon evaluates the pinned Java Edition **26.3** hole, steepness, height,
water, stone-depth, vertical-gradient, noise-threshold, biome-membership, temperature
preliminary-surface and negated material conditions.
`MaterialConditions` owns each worker's context and compiled conditions, keeping
cache lifetimes tied to that worker. Column updates invalidate both column and
block conditions; block updates leave column caches intact. Secondary surface
noise is shared and sampled lazily once per column revision.

Height and layer arithmetic preserves signed integer wrapping. Vertical gradients
resolve anchors during compilation, derive named positional random factories and
preserve the boundary order even with equal or reversed bounds. Stone depth uses
the appropriate floor/ceiling depth and Java's floating-point mapping and casts.
`MaterialSurface` implements the surface input interface used by this worker.

Run `python3 tools/test_material_condition_parity.py`. The
[local report](../benchmarks/rust-material-condition-2026-10-06/scoped-parity.json)
records **5,627,426 matching output lines per reference JAR** across **245 fixtures**,
**11,288 initially compiled conditions** and **217,969 context operations**.
The original Java transport constructs actual vanilla conditions and calls their
compiled evaluators against actual context updates. Secondary sampler queries
are traced to compare lazy evaluation and cache reuse. Tests include both random
algorithms, integer extremes, nonfinite secondary values, floor/ceiling selection,
reversed gradient bounds, repeated evaluations and late compilation.

For isolation, the native context's surface depth is supplied after each column
update, and secondary noise comes from a traced constant sampler. Full seeded
surface queries have a [separate comparison](material-surfaces.md). Sources and
fixtures are authenticated, and raw reference artifacts stay under `.local`.

The comparison also covers **8,609 codec inputs**, all **eight bundled material
condition definitions**, forward/nested references, canonical encoding and numeric
conversions. Immutable registry loading rejects missing/cyclic references and
duplicate canonical names without publishing partial results.

This scope excludes live biome-tag reloads, registry-owner
validation and arbitrary provider failures. Chunk application has a
[separate comparison](material-surfaces.md#chunk-surface-stage). Runtime trees
are bounded to 256 levels and 8,192 registered conditions per worker; these limits
are explicit resource bounds, not vanilla gameplay claims.

The same comparison now includes **6,271 initially compiled material rules**:
block results, ordered sequences, conditional rules and seeded terracotta bands.
It checks empty/singleton/nested sequences, first-match short circuiting, late
compilation, band-index failures and separate context/application coordinates.
[Bundled rule loading](material-rule-libraries.md) now integrates these runtime
rules; rule encoding remains pending.

Noise thresholds share lazy suppliers by canonical noise name and dimension mode.
Column-only suppliers still call the three-coordinate sampler with Y fixed at
zero. The comparison includes all 64 bundled noises under both random algorithms,
traced mutable inputs, repeated updates, late binding and nonfinite thresholds.
`MaterialNoiseSources` supplies lazy named samplers from the world positional
factory. Encoded numeric values are compared as double bits to preserve values
independently of JSON number formatting, including Gson NaN/Infinity output.

Biome membership and temperature checks use the climate fields of all 67 bundled
biomes. Tests cover possible-biome constant folding, shared biome lookup caches,
column/block updates, null lookups, thrown lookup failures and same-revision
retries that retain earlier condition results. A missing result after a failed
first evaluation remains an error. Worker condition evaluation returns typed errors.

Named biome-tag conditions resolve all **86 bundled tags** through the tag loader
and retain their tag identity when encoded. Comparisons cover nested references,
optional missing members, duplicate entries, empty tags, namespace aliases,
possible-biome folding and conditional-rule application. Registry snapshots are
immutable; live tag reloads remain pending.

Preliminary heights use a lazily populated unit-spaced XZ density plane, with
scalar sampling outside it. `MaterialDensitySources` accepts the compiled
preliminary-surface root and its world sampler context. The comparison checks
column minimum caching, Y-only updates, ignored original volume strides, partial
failed fills, retries, floor conversions, infinities/NaN and integer wrapping.
The preliminary plane is bounded to 1,048,576 samples.

Ore-vein rules accept three compiled density roots. A vein's density is never
positive outside the y bounds that `DensityGraph::positive_y_bounds` reads from its shape: a
range choice on a clamped y gradient whose out-of-range branch is a constant
<= 0, as vanilla's vein densities are. The surface bands drop the vein rule outside those
bounds. Density and richness fill the expected chunk volume at compilation, cut
at the highest y any vein reaches. Gap sampling stays lazy. Cached
lookups require an exact lattice position. Density queries use the block context;
random choices use application coordinates and the seeded `minecraft:ore` stream.
Tests cover both random algorithms, draw order, repeated calls, independent
coordinates, nonfinite runtime values, raw-ore chances and failed compilation.
Debug ore-vein visualization flags are outside this gameplay scope.

Material density storage is bounded to 32,768 bindings, 1,048,576 samples per
eager volume and 16,777,216 retained eager samples per context. Bundled rule
decoding has a [separate comparison](material-rule-libraries.md); rule encoding
remains pending. The surface-stage comparison integrates these density bindings.
