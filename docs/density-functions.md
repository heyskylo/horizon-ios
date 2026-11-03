# Compiled density evaluation

Horizon targets the repository-pinned Minecraft Java Edition **26.3**. The
`horizon_core::density` module implements selected compiled samplers and an initial
integrated graph compiler. Complete graph coverage and terrain generation remain pending.

Implemented samplers:

- Constants, including signed zero and nonfinite values.
- Context-bound blend alpha, blend offset and structure density; blend-density wrappers.
- Axis gradients with clamped, repeating and mirrored repeating coordinates.
- Unshifted noise and both XZ and XYZ shifted-noise specializations.
- Shift, shift-A and shift-B offset noise; old blended terrain noise; outer End
  island density.
- Euclidean, squared Euclidean, Manhattan and Chebyshev distance metrics and
  distance-to-point evaluation.
- X/Y/Z coordinate slices and the combined XZ bulk specialization.
- Coarse-grid interpolation with positive horizontal and vertical cell sizes.
- Surface-height searches and their compiled horizontal slice.
- Nested cubic splines, lazy coordinate evaluation and spline range analysis.
- Absolute value, negation, square, cube, square root, logarithm, reciprocal, sign, squeeze
  and leaky ReLU.
- Floor, nearest, ceiling and truncation to integer or sampled multiples.
- Addition, subtraction, multiplication, division, minimum and maximum, including
  their constant-operand specializations; clamping.
- Powers with general, constant-base and constant-exponent samplers.
- Range choice with general or constant branches, and linear interpolation with
  general or constant endpoints; single- and multi-threshold interval selection.

Immutable samplers share children and noise sources through `Arc`. A worker owns
its `SamplerContext`, which retains temporary vectors for nested bulk evaluation
and returns them on typed errors. `clear_scratch` releases the retained buffers.
This avoids allocating a temporary vector for each repeated evaluation after
warm-up; no whole-game performance improvement has been measured.

## World fields and references

`WorldFields` stores immutable sampler bindings for blend alpha, blend offset and
structure density, plus an optional `DensityBlender` provider. A worker attaches
these with `SamplerContext::with_world_fields`; changing them clears its cached
densities. Context-bound samplers resolve the binding on each scalar or bulk call
and delegate with the same context. Missing bindings use constants 1, 0 and 0,
respectively, including constant bulk tail writes. Custom fallbacks are supported.
Declared metadata remains independent of the bindings: alpha has range `[0,1]`,
offset and structure density have infinite ranges, and their axis masks are X/Z,
X/Z and X/Y/Z. Uniform-axis rewriting follows that metadata even for synthetic
bindings which vary along omitted axes.

`BlendDensitySampler` evaluates its input first. Absent or empty providers return
that result unchanged; other providers transform the logical volume in Z/X/Y
order while preserving the input's tail writes. Graph range and axes come solely
from the input. Differential checks cover absent and actual vanilla EMPTY Blender
contexts. Provider delegation/order tests do **not** validate the still missing
nonempty old-world blending algorithm or structure-density producer.

Direct reference holders compare by their value; registered holders compare by
holder identity. `register_function` creates an immutable named holder and
`function` retrieves it. Raw compilation delegates through its value. Optimization
inlines one holder layer before detecting cache nodes, preserving vanilla's
ordering and nested-wrapper behavior. A holder whose child changes during
rewriting is replaced by that child directly. These choices affect constant
specializations and cache allocation, and are checked against actual vanilla
holder records, named registries and the compiler.

## Graph compilation

`density::graph` defines immutable constants, gradients, eleven unary operations,
six binary operations, powers, rounding, clamps, range choices, lerps, interval selection,
distance-to-point, slices, caches, interpolation, surface searches, nested splines,
registered normal/shifted noise, offset noise, old blended noise and outer End
island density, context-bound world fields, blend-density wrappers and direct/named
reference holders.
Definitions reference earlier nodes in the same graph;
construction rejects nesting beyond 256 nodes. For ordinary float fields,
structural interning distinguishes signed zeros and canonicalizes NaN payloads,
matching Java record equality.
Precompiled input bindings carry caller-supplied ranges and axis dependencies;
the compiler trusts this metadata. Bindings have distinct identities.

`DensityCompiler::get_sampler` prepares shared caches, slices uniform axes, then
compiles and memoizes the root. Cache IDs belong to that compiler: worker contexts
must not reuse cache cells across compiler instances. Failed preparation consumes
its allocated ID and retains any successfully prepared children; retrying a failed
root does not return a cached failure. `compile_raw` bypasses rewriting and rejects
unprepared cache definitions.

Compilation integrates range propagation with min/max specialization and scalar
short-circuit bounds. Strictly disjoint min/max ranges can eliminate a sampler,
but both children compile before that decision. Touching ranges retain the branch.
Literal operands select dedicated arithmetic paths, including reciprocal
multiplication for a literal right-hand divisor. These choices affect float bits,
child evaluation and buffer tails. Uniform-axis rewriting introduces zero slices
in X/Z/Y order, combines adjacent X/Z slices, and reduces child volumes while
retaining coordinate strides.

The differential suite compares these passes with vanilla's actual
`DensityFunctionCompiler` across 368 independent unseeded graphs, 64 mixed seeded
graphs and six seeded failure cases, with 18,282 successful root compilation
requests. It checks declared ranges and axes, structural equality, cache-ID counts,
memoized sampler identity, failure state, scalar/bulk values and child call counts.
Some bound inputs deliberately use synthetic metadata to exercise compiler
decisions; those cases do not validate terrain metadata or seeded terrain output.
Serialized codecs, concrete world-field producers and complete terrain integration
remain pending.

`DensityCompiler::new_seeded` fixes the world seed and random algorithm for the
compiler's lifetime. Registered noise definitions use canonical namespaced registry
identifiers; duplicate names are rejected. Ordinary noise instances reuse a sampler
created from the world positional factory and the identifier's hash. The
`minecraft:nether/temperature` and `minecraft:nether/vegetation` keys bypass that
map and create legacy Nether biome samplers from the world seed and seed plus one,
respectively. The addition wraps as Java `long`. `get_noise_sampler` exposes this
lookup for callers needing the same registered sources.

Old blended terrain noise uses the world seed directly in legacy mode and the
positional `minecraft:terrain` stream in modern mode. Noise compilation retains
literal positive-zero tests when choosing unshifted, XZ and XYZ implementations;
negative-zero shifts retain the shifted path. Shift functions compile in X/Z/Y
order, while the XYZ sampler evaluates them in X/Y/Z order. Declared noise ranges
come from their registered definitions; axis dependencies combine nonzero scales
with all shift functions.

The seeded oracle constructs vanilla's actual `RandomState` with a frozen test
noise registry, then uses its compiler and compile context. Constructor-only
material inputs use zero-noise fixtures; this does not validate material rules or
surface generation. The corpus includes extreme signed seeds, both algorithms,
instance identity, special Nether failures, offsets, blended noise and mixed
cached/interpolated/spline graphs. The graph API still expects registered,
canonical identifiers. The [world-generation codec suite](world-generation.md)
now loads and compares all bundled noise and density registry definitions against
the actual registry-aware codecs. Complete chunk generation remains pending.

`EndIslandSampler` uses a legacy stream initialized directly from the world seed,
regardless of the world noise algorithm. It discards 17,292 random draws before
constructing simplex noise with skipped offsets. The island search retains
vanilla's strict noise threshold, float arithmetic, integer wrapping and division
toward zero for negative coordinates. Its declared range is `[-0.84375, 0.5625]`,
and its dependency mask is X/Z. Bulk sampling computes each horizontal column
once, repeats the result over Y and preserves the output tail. The center island
is supplied by the surrounding terrain graph; this source supplies outer islands.

Spline coordinates can reference graph functions directly through
`CubicSpline<FunctionId>`. Graph rewriting visits parent coordinates before nested
values, retaining repeated coordinate occurrences and point order. Compilation
shares structurally identical coordinate inputs, validates each unique coordinate
range immediately after compiling it, and then creates the existing spline
sampler. Graph range analysis remains lazy: a NaI coordinate can stop range lookup
before visiting a nested value even when compilation later rejects that value's
coordinate. Errors follow their respective traversal paths. Spline point arrays
use Java's bit-based equality, including canonical NaNs and distinct signed zeros.
The graph nesting bound includes paths through nested spline values and coordinate
definitions.

Interval-selection definitions retain threshold order, duplicates and nonfinite
values. Their declared range covers all branches without asking for the selector's
range; their axis dependencies include both the selector and every branch. The
Rust definition API requires one more branch than thresholds and limits the list
to 256 branches. The JSON decoder separately requires ordered thresholds and a
minimum of two branches. Its ordering retains Java's float comparator, including
distinct signed zeros; equal consecutive thresholds are allowed.
Threshold lists retain vanilla's separate equality and hashing rules: opposite
zeros compare equal but hash differently; separate lists containing NaN compare
unequal. Thresholds use an immutable `Arc` so rewritten definitions preserve list
identity. Interning tracks equality classes and signed-zero hash differences,
including cancellation through parent definitions. `DensityGraph::java_equals`
exposes equality separately from interned IDs and compiler cache reuse.
Distance-to-point definitions declare all three axes and `[0, infinity]`, and
compile to the checked wrapping-coordinate metric sampler.

## Scalar and bulk behavior

Power graphs compile both children in base/exponent order before selecting a
specialization. A literal base takes precedence over a literal exponent. Otherwise,
literal exponents with absolute values one half, one, two or three select square
root, identity, square or cube. Negative exponents wrap those specialized samplers
in a reciprocal. Other exponents return directly from the fallback power sampler,
which uses the original signed exponent. These paths retain distinct float
arithmetic and signed-zero behavior.
General samplers evaluate both operands without zero/one shortcuts and retain
base writes if exponent evaluation fails. Constant-operand bulk samplers also
transform buffer tails.

`RoundSampler` implements floor, nearest, ceiling and truncation to integer or
sampled multiples. Nearest uses Java's float-to-int rounding: ties go toward
positive infinity, NaNs become zero, and extreme values saturate to the signed
32-bit integer domain before conversion back to float. Floor, ceiling and
truncation retain nonfinite float values. A zero multiple returns the input,
preserving its signed zero and NaN behavior; the multiple still evaluates after
the input. Other multiples use float division, integer rounding and float
multiplication in that order.

Only a literal-one graph multiple selects the integer specialization. It modifies
the input buffer, including its tail. A sampled multiple uses a volume-sized
temporary after the input and requires an exactly volume-sized output through
this Rust API. Failure in the multiple leaves input writes visible. Range
propagation evaluates the multiple's range first, divides the input range, maps
the rounding operation over the endpoints and multiplies by the multiple range.

All densities are `f32`; noise coordinates and scales are `f64`. Gradient
coordinate arithmetic wraps as Java `int`, including overflow of the endpoint
difference and `MIN / -1`. Repeating gradients use floor division, not truncation.
A zero repeating period returns a typed error at evaluation time. Clamped
zero-width gradients retain the reference floating-point behavior.

Scalar and bulk evaluation deliberately remain separate. Unshifted noise bulk
sampling calls the noise source's bulk kernel, whose arithmetic can differ from
scalar Perlin evaluation. Shifted noise evaluates child volumes first and then
samples its source using those values. XYZ evaluates shifts in X, Y, Z order;
XZ omits the Y addition entirely. Inserting a synthetic zero would change signed
zero behavior.

Scalar multiplication and division return positive zero without evaluating the
right child when the left child is zero. Scalar minimum and maximum can skip the
right child using their supplied compiled range bounds. Bulk binary operations
evaluate both child volumes. Bulk minimum and maximum replace a value only on a
strict comparison; their handling of NaNs and equal signed zeros therefore differs
from scalar `Math.min` and `Math.max`. Constant-operand min/max retain that same
scalar/bulk distinction. Constant multiplication and division do not inherit the
generic scalar zero shortcut.

Range choice samples its selector first and evaluates only the selected branch
for a scalar query. Bulk range choice evaluates the in-range branch first, then
the selector, then the out-of-range branch, retaining earlier output writes when
a later child fails. Interpolation evaluates only the selected endpoint at alpha
zero or one in scalar queries; bulk interpolation evaluates its dynamic endpoint
volumes before selecting or blending values. Constant endpoints avoid allocating
a scratch vector. Neither path clamps alpha to the unit interval.

Interval selection preserves threshold order and chooses the first strictly
higher threshold, or the last branch. NaN comparisons fall through. Scalar
queries run only the selected branch; bulk queries evaluate the selector and
then every branch in order before replacing output values. A contiguous scratch
vector holds branch volumes, with an exact-volume slice passed to each child.
The Rust constructor requires one more branch than thresholds and caps the
number of branches at 256. The JSON decoder validates threshold order; it does
not sort the list.

A slice's length is its logical density-buffer size. Constants fill that whole
slice. Unshifted noise clears it before accumulation. Gradients overwrite only
the volume's cells; shifted noise retains whatever its X child wrote outside the
volume. Unary operators transform the whole logical buffer after their child
returns. Binary, general range-choice, interval selection and interpolation bulk evaluation require exactly a volume-sized buffer,
matching the valid size of its temporary right-hand buffer. Horizon rejects an
oversized multi-input output before evaluating children; Java's accidental reads beyond
the temporary array and backing-pool capacity are outside this bounded API.
Short buffers are always rejected before writes. Errors from evaluated children
retain earlier writes, and scratch storage is returned for reuse.

## Coordinate slicing

`SliceSampler` fixes an X, Y or Z coordinate, or both X and Z. Scalar requests
replace those coordinates before calling the child. Bulk requests sample a
reduced volume once, then copy its samples into the repeated columns or planes.
The reduced descriptor retains the original steps on all axes, including fixed
axes, so cached children see the same descriptor as vanilla. Expansion uses
column copies or fills and does not repeat child sampling.

When every fixed axis already has size one and the requested minimum coordinate,
the sampler passes the original output directly to its child. This distinction
preserves child writes outside the volume and partial writes on errors. Otherwise
the child receives scratch storage; its failure leaves the caller's output
untouched, and successful expansion writes only the volume's cells. Graph-level
slice insertion and combining adjacent X/Z slice definitions remain compiler work.
The corpus checks all four compiled forms, nested slices, cached children,
unchanged and reduced volumes, call counts, buffer tails and integer boundaries.

## Grid interpolation

`InterpolatedSampler` accepts positive XZ and Y cell sizes. A scalar request at
an exact grid point delegates to the child's scalar method. Other points sample
a `2 × 2 × 2` corner volume through the child's bulk method, then interpolate X,
Y and Z in that order. Negative positions use floor division and remainders;
corner coordinates retain Java integer wrapping.

An aligned bulk volume whose steps equal the cell sizes delegates directly to
the child; size-one axes may have other steps. Other volumes sample a coarse
grid covering the requested block extent. The bulk path blends Z, then X, and
advances each Y column through repeated float additions. Scalar interpolation
uses division for its fractions; bulk interpolation uses precomputed reciprocal
multiplication. These differences can change low bits even with simple linear
input and are retained intentionally. End corners are clamped to the last coarse
sample when the output ends exactly on a grid point.

For strided output, vanilla first evaluates a full block-resolution volume with
sizes equal to output sizes times steps, then selects the strided cells. Horizon
preserves that extent, including trailing blocks, the child's volume descriptor,
and the repeated-addition rounding. Scratch vectors are retained in the context.
The interpolating paths write only volume cells; direct delegation retains the
child's tail behavior. A failed coarse-grid child leaves the caller unchanged.
Invalid derived volumes are rejected through the bounded volume API; accidental
Java array behavior for overflowing or unallocatable extents is outside this
scope. The JSON decoder loads interpolation nodes with positive cell sizes.
Automatic insertion by the chunk-generation pipeline remains pending.

## Surface-height search

`FindTopSurfaceSampler` evaluates its upper bound, rounds it downward onto the
cell-height grid using the pinned float-to-integer floor behavior, then samples
density at descending grid heights. It returns the first strictly positive
sample, or the lower bound. Zero and NaN densities do not stop the search. When
the aligned upper height is at or below the lower bound, it returns immediately
without sampling density, including at the equal-height boundary.

`compile_surface_search` adds the Y=0 slice used by vanilla's function compiler.
This forces the upper bound to be evaluated at Y=0 and repeats horizontal results
through the requested volume. The raw sampler accepts only one Y layer for bulk
requests. It first samples all upper bounds into the output, then searches columns
in Z/X order, replacing each successful result. A later child failure preserves
earlier results and the remaining upper-bound values. The surrounding slice can
instead isolate those partial writes in scratch storage.

The constructor requires positive cell height. Manually constructed searches
whose descending height overflows return a typed error; wrapped nonterminating
searches are outside this bounded API. The differential corpus covers ordinary
finite bounds, exceptional float bounds with immediate crossings, grid alignment,
Y slicing, invalid multi-layer raw calls, cached children, NaN/zero crossings,
short circuits, buffer tails and child failures.

## Cubic splines

`CubicSpline` represents a constant or an immutable set of points. Each point has
a location, derivative and nested spline value. `SplineSampler::new` resolves
coordinate IDs against a density-sampler table and assigns compact local IDs in
parent-first traversal order. Repeated IDs share one coordinate cache. Structural
deduplication of equivalent density definitions remains graph-compiler work.

Evaluation retains vanilla's binary interval search, including behavior for
manually supplied equal, unsorted or NaN locations. An endpoint extends linearly;
a zero derivative returns the endpoint value without multiplying a possibly
nonfinite coordinate. Interior evaluation samples the left nested value before
the right, then performs the float cubic correction in the reference order.
A constant NaN may occur as a nested value, but vanilla rejects it as the root of
a compiled constant sampler; Horizon preserves that distinction.

Scalar queries compute coordinates lazily and retain non-NaN values for the
remainder of that query. NaN results are queried again when the same coordinate
is needed. Bulk evaluation walks output cells, evaluating a coordinate's entire
child volume only when a selected branch first needs it. Those buffers remain
available through the call, including NaN cells. Unselected coordinates are not
evaluated. A later lazy child failure retains completed output cells, and Horizon
returns acquired buffers to the context on both success and error.

Constant roots fill the entire logical output slice. Nonconstant splines use an
exact-volume output; oversized output would read beyond coordinate buffers in
the reference and is rejected by the bounded Rust API. Spline nesting is bounded
to 256 levels, and empty point lists or unresolved coordinate IDs are rejected.

Range analysis accepts coordinate ranges from the enclosing graph. It preserves
endpoint extension, child range evaluation order, all-point bounds and the
conservative derivative correction used by vanilla. A NaI coordinate stops
analysis before child ranges are evaluated. Invalid finite/nonfinite arithmetic
returns an interval error; the implementation does not substitute tighter bounds
or a numerically different cubic-extrema calculation. The JSON decoder loads
nested spline graphs, preserves their point order and requires explicit
derivatives. Spline builders and complete terrain integration remain pending.

## Cached evaluation

`SamplerContext::cached()` enables context-owned cache cells; the default context
continues to evaluate cache wrappers directly. `CachingDensitySampler` shares
immutable child samplers while keeping cached data in the context. IDs are
bounded to `0..=65535`, and the API rejects recursive evaluation of the same cache
cell. `DensityCompiler` allocates IDs for supported graph definitions. Callers
constructing cache samplers directly must supply consistent IDs themselves.

A scalar request first checks its last-value cache, using vanilla's packed block
position as the key. This intentionally retains key collisions for coordinates
outside the packed domain. A NaN value is never a last-value hit. Next, a request
may read an exact lattice sample from the cached volume. Off-lattice positions
miss; lookup retains Java's wrapped relative coordinates and extent products.
A volume hit does not update the last scalar key, and replacing a volume does not
invalidate that scalar entry. These details matter because bulk noise can round
differently from a fresh scalar sample.

A bulk request reuses its cached buffer when every volume descriptor field
matches. Otherwise it evaluates the child into a private retained buffer and
copies to the caller only on success. Vanilla installs that cache entry before
calling the child: a failed child can leave a partially written cached volume
that a later request reads without recomputation. Horizon preserves the writes
and the entry. Unwritten values are initialized to zero; failure comparisons use
a vanilla context with a zero-initializing arena. Unwritten data from vanilla's
global recycling pool is not claimed to match. Cached bulk output must have
exactly the volume's size; invalid lengths are rejected before child evaluation.
`clear_caches()` releases stored values and buffers for context lifecycle use.

The differential corpus tests enabled and disabled caches, nested nodes, scalar
key collisions, NaN misses, repeated volumes, replacing volumes, on/off-lattice
queries and child failures. Counting wrappers report actual child scalar and
bulk calls as well as output bits, so a correct value produced through the wrong
cache path cannot silently pass the trace comparison.

## Terrain noise composition

The offset-noise compilers preserve the pinned version's evaluation paths. Shift
and shift-A use a noise sampler with quarter-scale coordinates followed by a
separate multiplication by four. Shift-A sets the vertical scale to zero.
Shift-B samples `(z / 4, x / 4, 0)` in scalar calls. Its bulk path constructs a
transposed horizontal plane, accumulates noise into that plane with amplitude
four, then repeats each result through its Y column. It leaves the output tail
untouched. These paths are checked with Perlin, simplex, smeared and modern
normal-noise stacks, from both random streams.

`BlendedNoise` creates the minimum-limit, maximum-limit and main stacks in that
order from the caller's random stream. The two limit stacks contain 16 smeared
layers each; the main stack contains eight. Layers are created from high to low
frequency, retaining the version's double-precision amplitude calculation before
the float conversion. The compiled sampler adds `0.5` to the main result, clamps
it to `[0, 1]`, then interpolates the limit samplers. This composition reuses the
checked scalar/bulk sampler implementations, including their evaluation order.
Its declared range is computed separately from the limit-stack parameters.

The public `create_blended_fbm` helper also supports first octaves from `-4095`
through zero. It rejects requests outside that work bound before consuming RNG
state. Invalid ranges are rejected at stack build time after layer construction,
matching vanilla's draw consumption within the supported bound. Serialized
parameter restrictions remain integration work; the graph compiler now supplies
the compile context's `terrain` seed lookup. The comparison corpus includes the bundled overworld/nether
and End parameter sets, valid random parameters, nonfinite inputs, constructor
failures, post-construction RNG state, octave metadata and scalar/bulk outputs.

Distance-to-point evaluation subtracts coordinates as wrapping Java integers
before converting to floats. It retains the reference float arithmetic and
square-root conversion order, rather than using a double-precision hypot.
Metric tests also exercise arbitrary float inputs, including NaNs and infinities.

## Interval analysis

`Interval` implements checked exact, symmetric and general bounds; NaI and
infinite ranges; endpoint and interval hulls; addition, subtraction,
multiplication, division, min/max, reciprocal, absolute value, square, sign,
powers, logarithms, clamp, monotonic endpoint mapping and linear interpolation.

Power propagation handles exact and interval exponents separately, including
signed-zero bases, negative bases with integer exponent bounds, infinite
exponents and NaI endpoint hulls. Negative-base ranges preserve vanilla's
floating-point neighbor checks when probing odd/even integer exponents.
Logarithmic ranges intersect the valid nonnegative domain before endpoint
mapping. A fully negative range becomes NaI. Square-root graph ranges use the
power interval with exponent one half, retaining valid endpoints even when
some input values produce NaN.

These operations preserve vanilla's special rules. Multiplication treats a zero
bound times infinity as positive zero. The reciprocal of an exact zero interval
is NaI; an interval spanning both sides of zero maps to infinite bounds. Hulls
ignore NaI members unless all members are NaI. Monotonic mapping skips NaI and
otherwise evaluates both endpoints before rejecting a NaN result. It accepts a
caller-supplied transformation; neither implementation proves monotonicity.

Interpolation range analysis has distinct finite- and infinite-endpoint paths.
It uses range-specific zero products and infinity handling, rather than calling
the scalar density interpolator. Interval construction also canonicalizes a
negative-zero lower bound when the upper bound is one, matching the reference's
cached `[0, 1]` instance. NaI compares equal to itself, and signed-zero endpoints
compare numerically equal. `java_hash_code` exposes vanilla's endpoint-bit hash
separately: implementing Rust `Hash` with that function would violate Rust's
requirement that equal keys have equal hashes for some signed-zero intervals.

## Validation and limits

Run `python3 tools/test_density_parity.py` after preparing the pinned artifacts
with `python3 tools/test_rust_parity.py --prepare`. The original Java adapter
instantiates the actual vanilla sampler classes; it does not duplicate their
arithmetic. Both the client and server JARs process the same deterministic corpus
as the release Rust binary. Reference artifacts are checksum-verified, and source
hashes must remain unchanged during a run. Finite outputs and signed zeros are
compared by bits, with NaNs canonicalized.

The corpus includes all axes and tiling modes, raw floating-point inputs,
nonfinite values, overflowing block coordinates, both RNG streams, Perlin,
simplex and smeared noise, nested shift graphs, repeated bulk evaluations,
logical buffer tails, unary and binary combinations, scalar short circuits,
zero-period failures and partial writes. The runner removes an old success report
before artifact verification, so a failed run cannot retain a stale success.

The [recorded local comparison](../benchmarks/rust-density-2026-10-05/scoped-parity.json)
matched **15,524,627 output lines per reference JAR**.

The separate [float-log comparison](../benchmarks/rust-float-log-2026-10-05/scoped-parity.json)
exhaustively matched **all 4,294,967,296 float input bit patterns** for the compiled
scalar logarithm against both JARs on the recorded Linux/JDK runtime. Signed zeros,
subnormals, infinities and NaNs are included; NaN results use Java's canonical
`Float.floatToIntBits` representation. This does not establish double-precision
logarithm parity or other-platform math parity. Reproduce it with:

```bash
python3 tools/test_float_log_parity.py --exhaustive
```

The default command checks 1,048,576 inputs around domain and rounding boundaries
and is configured in CI. The density suite covers bulk composition and graph
range propagation separately.

Private inputs, output dumps and reference classes remain under
`.local/rust-density-parity/`. Only the metadata report belongs in
`benchmarks/rust-density-2026-10-05/`. Local results are Linux evidence;
configured Windows/macOS CI checks are not a claim that those remote jobs ran.

Pending work includes remaining graph definitions, serialized graphs,
serialized registries, concrete structure-density and nonempty old-world blending
providers, and full seeded chunk generation.
This suite establishes only the operations and inputs it exercises;
it does not establish whole-game parity or an FPS/render-distance improvement.

## Related docs

- [World generation](world-generation.md)
- [Noise generation](noise-generation.md)
- [Terrain filling](terrain-filling.md)
