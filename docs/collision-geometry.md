# Horizon collision geometry

The version target is Minecraft Java Edition **26.3**. Geometry is a simulation
foundation; Horizon has a native client for inspecting a controlled block scene.
Live gameplay and entity simulation remain unfinished.

`horizon-core::geometry` provides `Vec3`, `Aabb`, `Axis`, `Direction`, `BlockHit`,
and immutable `VoxelShape` values. Coordinates are doubles. Arithmetic keeps
vanilla's operation order, signed zero, NaN comparisons, normalization threshold,
and collision epsilon. It does not enable approximate or fused simulation math.

Implemented vector operations include addition/subtraction, vector-to, component
products, scaling, normalization, dot/cross products, interpolation, projection,
distance/length, horizontal vectors and finite/equality checks. Rotations, look
directions, alignment, axis replacement, direction offsets, and pitch/yaw extraction
are also implemented. The full Vec3 API and entity integration remain absent.

Trig uses vanilla's 65,536-entry float sine table with double angle scaling and
Java long narrowing. The table is generated from the pinned formula in Rust;
the geometry suite compares every generated entry against the runtime table.
Inverse trig narrows to float at vanilla's conversion points. These comparisons
validate the tested platform; other platforms need the same bit comparisons.

Bounding boxes normalize endpoints with Java-compatible min/max. Queries include
half-open containment, strict intersection, transforms, distances, ray clipping,
block-coordinate construction with wrapping integer endpoints, setters, next-value
deflation, and swept-box collision tests. A disjoint `intersect` preserves vanilla's
constructor normalization rather than introducing a different empty-box rule.

Shapes keep exact coordinate arrays and a packed occupancy grid. Greedy boxes are
cached once in vanilla order. Collision-distance queries use occupied grid cells;
ray and closest-point queries reuse the cached boxes. Column minima/maxima use
vanilla's cyclic perpendicular coordinates and infinity for empty/outside cells.
Cube shapes retain arithmetic
lookup; moved shapes use array lookup even for a zero offset. Ray hits preserve
face tie ordering, endpoint exclusion, tolerances, and the 0.001 inside-hit nudge.
Boolean shape joins and overlap predicates preserve cube, identical,
disjoint and epsilon-merger behavior. Shape operations reject truth tables that
fill two empty cells, as vanilla does. Per-axis cube-coordinate metadata survives
partial merges and is checked when importing cached shapes. Box construction
preserves thin-box rejection and up to 1/8 cube quantization. Optimized joins
reconstruct greedy boxes in vanilla order. Face extraction is implemented; edge extraction and shape rotations remain absent.

Discrete axis extrema are retained separately from occupied cells. A box can
quantize to no occupied cells while retaining a finite minimum/maximum on other
axes. That state affects vanilla's lower-block floor-height lookup. Empty joins
also preserve their integer sentinel extrema; optimization and moving an empty
shape return the canonical empty shape.

Shape clones preserve identity and share immutable coordinate, occupancy and box
storage. New shapes keep separate identities even when their grids are equal.
This preserves alias-sensitive join shortcuts and avoids copying large grids
when an operation returns an existing shape. The cache importer shares equal
grid storage while retaining the distinct vanilla shape identities.

`resolve_shape_movement` ports vanilla's movement resolver over a supplied shape
list: Y first, then horizontal axes in movement-magnitude order, translating the
box by resolved motion before each axis query. `candidate_step_heights` preserves
float narrowing, duplicate identity, current-Y exclusion, and sorted candidate
heights. `resolve_step_movement` adds the initial collision and step-up decision,
including landing-height adjustment, a second collider query, and the first
candidate improving horizontal movement. `entity_collision_bounds` preserves
the initial entity query bounds. These helpers take caller-selected colliders;
they do not choose world/entity shapes, set grounding, change velocity, or apply
inside-block effects.

The step-up corpus calls the actual private `Entity.collide` method for **17,680
cases** per JAR. Reference fixtures supply entity inputs and controlled
block/entity/border shape lists; vanilla performs the movement decisions. Query
bounds and results are compared bit for bit. This verifies the decision with
those inputs, not live world selection or entity state transitions.

`block_floor_height` returns the selected shape's maximum Y, or lazily queries
the lower shape only when the current one is empty. Its 12,000-case corpus also
checks the lower supplier's invocation count against `BlockGetter`.

`traverse_blocks` visits blocks in vanilla ray order, including its endpoint nudge,
strict corner comparisons, early callback return, integer wrap and signed-zero
endpoint equality. It accepts finite rays and an explicit visit budget. Invalid
geometry or exceeding that budget returns a typed error; earlier callbacks remain
applied. Actual block/fluid ray selection is a separate integration task.

Inclusive block-box iterators preserve X/Y/Z order, while directional iterators
preserve vanilla's movement-dependent axis order and corner signs. AABB iteration
includes floored maximum faces. They return independent position values and
reject true volumes above the positive Java-int domain, instead of reproducing
overflowed Java iteration counts.

`sweep_intersected_blocks` ports movement-path visits from
`BlockGetter.forEachBlockIntersectedBetween`: tiny movement, the initial box,
ray-clipped footprints, packed-position deduplication, final box, step indices,
and early callback stops. Stationary/tiny movement avoids a deduplication-set
allocation. Inputs and computed geometry must be finite; a caller-selected work
budget counts ray steps and iterator candidates, including suppressed duplicates.
Earlier callbacks are not rolled back on an error. Block effects still need
their own implementations and integration with entity movement.

`horizon-world::collision::CachedCollisionRegistry` imports a private local export
of vanilla cache data. The export includes DataVersion, all state descriptors,
deduplicated shapes, coordinate-kind metadata, discrete extrema, and a per-state
cache index. Import verifies every state ID
against the block registry. Section lookup also requires the same borrowed registry.
Imported nonempty extrema must agree with occupancy; empty extrema must be in
the grid's domain or match vanilla's empty-join sentinel triple. Extrema form part
of the grid-sharing key, so equal occupancy does not erase different metadata.
There is no fallback shape for an unsupported state: `None` means block behavior
must select it, while a cached empty shape is a valid `Some` value.

This covers the cached branch of the **no-context** `BlockState.getCollisionShape`
method. Vanilla's overload taking `CollisionContext` always calls block behavior;
the cache cannot establish entity collision parity. Dynamic and ordinary entity-context selection are implemented as described below.
Experimental minecart contexts, world borders, movement integration, fluid
simulation, block effects and live collision broad-phase queries remain pending.

Run the comparisons after preparing the pinned artifacts:

```sh
python3 tools/test_geometry_parity.py
python3 tools/test_collision_parity.py
```

Both suites invoke actual vanilla methods from the authenticated client and server
JARs. They compare canonical double bits, including infinities/NaNs, rather than
using a numerical tolerance. The geometry corpus includes empty and sparse grids,
greedy decomposition, overlapping boxes, rays, tiny movements and IEEE boundaries.
It compares **1,320,187 output lines per JAR**, including **2,400 integer-box
iterator cases**, **2,400 AABB iterator cases**, and **9,022 movement-path traces**.
The registry suite exercises cached shape queries through resident section IDs,
translation at negative and world-border coordinates, and invalid import rejection.
Its export records **35,524 cached states**, **199 uncached states**, **6,093
shape identities**, and **330 distinct grids**. The comparison runs **213,144 queries** plus state/box/identity checks,
producing **1,097,456 matching output lines per JAR**. These counts describe this
branch and corpus; they are not a gameplay coverage percentage.

Raw grids admit at most 1,048,576 cells with strictly increasing, non-NaN
coordinates. Oversized or malformed imports/joins return typed errors. This
bounded library API does not reproduce Java's complete exception/object surface.

All exported data, test inputs, reference output, and compiled adapters remain
under gitignored `.local/`. Success reports record exact sources, inputs, outputs,
and reference hashes. A runner removes its prior success report before running
and rejects edits to the captured sources during validation. Passing these suites
does not prove whole-game parity or any FPS/render-distance improvement.

Checked-in [geometry metadata](../benchmarks/rust-geometry-2026-10-05/scoped-parity.json)
and [cached-collision metadata](../benchmarks/rust-collision-2026-10-05/scoped-parity.json)
identify their tested source versions. Reports cover their recorded snapshots;
later changes require fresh validation.

Entity obstruction now evaluates ordered candidate snapshots, excluded entity
identity, spectator/removal/building flags, shared root-vehicle identity and
voxel-shape intersection. Empty shapes skip the query; candidate collection
finishes before obstruction evaluation. Unit-block queries translate in floating
point, including integer-coordinate boundaries. Run
`python3 tools/test_entity_obstruction_parity.py` to compare against the actual
vanilla `EntityGetter` default methods and entity metadata queries.

The [entity-obstruction report](../benchmarks/rust-entity-obstruction-2026-10-06/scoped-parity.json)
records **15,121 matching output lines per JAR** over composite shapes, boundary
contacts, thin boxes, metadata filters, vehicle roots and injected query failures.
The Java fixture privately allocates entity snapshots without running their
constructors. Entity lifecycle, spatial indexing and vehicle-tree maintenance
remain separate implementation work. Workspace tests, warning-free Clippy and
**138 Python tests** passed at this snapshot.

Legacy block solidity now uses explicit definition overrides and cached collision
bounds, with force-on precedence and native mean-size/height thresholds. The
no-context replaceable and liquid flags are kept independently. Solid rendering
uses the occlusion definition flag and a full-shape test over imported raw
occlusion geometry. Import validates the pinned
DataVersion, full state identities and complete block definitions before building
a shared property table. Predicates query this table in constant time.

`python3 tools/test_block_properties_parity.py` compares all **35,723 states** and
**18,096 synthetic cache/flag/shape cases**, including adjacent floating-point values
around solidity thresholds and both occlusion flag values. The [block-properties report](../benchmarks/rust-block-properties-2026-10-06/scoped-parity.json)
records **197,180 matching lines per JAR**. Definition flags, collision and raw
occlusion data are private runtime inputs; Rust calculates solidity and solid
rendering and caches full-face occlusion masks. Every state and synthetic shape
is compared on all six faces; source and fixture hashes are recorded.
Item/fluid-context replacement and collision with live entities remain separate work.

Face selection now preserves native cube-axis shortcuts, epsilon sample points,
slice coordinates and clamped parent extrema, including sparse slices whose
native bounds differ from occupied boxes. Repeated queries share face identities.
The FULL, CENTER and RIGID support tests execute against the selected shape.
`python3 tools/test_face_support_parity.py` compares **1,615 input shapes** and
nested faces, producing **595,686 matching lines per JAR** in the
[face-support report](../benchmarks/rust-face-support-2026-10-06/scoped-parity.json).
The [refreshed geometry report](../benchmarks/rust-geometry-2026-10-06/scoped-parity.json)
also retains **1,320,187 matching lines per JAR** after face slicing was added.
Block-specific support-shape selection is a separate layer.

Registered support caches now derive FULL/CENTER/RIGID masks from privately
imported support geometry and share results by shape identity. The importer
checks the data kind and full state identities. `None` explicitly requests
uncached behavior; it never becomes an assumed unsupported face.
`python3 tools/test_block_support_parity.py` compares **35,524 cached states**
across six directions and three support kinds, while retaining **199 uncached
states**. The [cached-support report](../benchmarks/rust-block-support-2026-10-06/scoped-parity.json)
records **248,867 matching lines per JAR**.

Face-cache storage is now lazy and attached to shape identity. Clones share it;
new identities and moved shapes start with an empty cache. A regression test
checks that cached faces do not retain their parent after it is dropped. The
face and geometry differential suites passed again after this allocation change,
as did workspace tests, Clippy and **140 Python tests**. No FPS claim is inferred
from these library checks.

Dynamic support selection now handles bamboo collision offsets, dripstone and
sulfur-spike thickness/direction, the empty-context scaffolding support shape,
powder snow, and shulker animation-state support faces. Moving-piston lookup
preserves missing/wrong block-entity behavior and delegates active entity shape
calculation to the world. The [dynamic-support report](../benchmarks/rust-dynamic-support-2026-10-06/scoped-parity.json)
records **4,867,780 matching lines per JAR** over **107,308 fixtures** covering all
**199 uncached states**, coordinate boundaries, block-entity snapshots and query
failures. Run `python3 tools/test_dynamic_support_parity.py` to reproduce it.
The current suite also compares no-entity-context collision geometry in **60,161
fixtures**, including **13,014 shulker progress fixtures**. Shulker collision
bounds retain native float interpolation and differ from animation-state support
faces. `BlockCollisionRegistry` dispatches cached and dynamic collision queries;
entity movement contexts and block-entity lifecycle remain separate work.
Workspace tests, warning-free Clippy and seven focused Python tests passed.

Moving-piston collision now selects stationary extended bases and moving heads,
translates shapes using native float progress arithmetic, and honors the
movement-direction collision exclusion. Interpolation preserves float operation
order, including negative partial ticks and IEEE edge cases. The separate
collision-related state used for entity pushing retains its own sticky/head
rules; entity pushing itself is not implemented by this module.

`python3 tools/test_piston_shape_parity.py` compares **102,315 fixtures**,
including every registered moved state and **10,000 raw float interpolation
cases**. The [piston-shape report](../benchmarks/rust-piston-shape-2026-10-06/scoped-parity.json)
records **1,096,811 matching lines per JAR**. This corpus uses absent block
entities for moved-block collision selection. Workspace tests, warning-free
Clippy and **142 Python tests** passed. Tick progression and live world mutation
remain separate work.

The refreshed dynamic-support corpus also includes **9,216 active piston
snapshots**, composing block-entity lookup with piston collision generation and
all support tests. It compares extension/retraction, source heads, progress
boundaries, collision exclusions and query failures. These snapshots do not
exercise piston ticks, pushed entities or chunk updates.

The archived block-survival snapshot executes 51 rule kinds with 63 distinct rule/tag
bindings over registry-bound solid flags, air identity, support tags and
FULL/CENTER/RIGID geometry and empty-context collision. The verified snapshot covers
**all 1,286 pinned block definitions and 35,723 states**. Candidate
properties select attachment direction, hanging state, door/plant halves and
pale moss carpet's serialized `bottom` property. Pressure plates and cactus
flowers preserve repeated neighbor reads. Downward center support applies the
unstable-bottom-center tag only where native helpers do. Upper doors require the
same block below; upper double plants also require its lower half. Crop survival
checks brightness against 8 before querying support; farmland preserves its
conditional second read above, and dripleaf stems read both neighbors. Sugar cane,
lily pads, frogspawn, spore blossoms, small dripleaves and tall seagrass preserve
their independent fluid queries, source/full-water requirements and tag checks.
Growing plants preserve head/body identity and kelp support restrictions.
Scaffolding recomputes neighbor distance in native order; chorus plants and flowers
preserve branching reads and early exits. Piston heads validate base type, facing
and extension. Mangrove propagules, speleothems, pitcher crops, bells and shelf
mushrooms use their version-specific attachment rules. Multiface blocks, vines,
hanging moss, snow layers and sea pickles preserve support/collision query order.
Fire uses imported ignition odds and waterlogging; cactus checks horizontal solids
and lava before its support and upper liquid state. Paths recognize fence gates,
and mushrooms preserve the light-override tag and solid-render requirement.

Interrupted runs can be resumed with `python3 tools/test_block_survival_parity.py --resume`.
The runner verifies source, environment, fixture and compiled-artifact hashes
before reusing completed stages. A changed output is regenerated; changed inputs
require a fresh run without `--resume`. Checkpoints stay under `.local`, and
a parity report is written only after both native comparisons pass.

`python3 tools/test_block_survival_parity.py` compares actual native `canSurvive`
calls in both pinned JARs. The [block-survival report](../benchmarks/rust-block-survival-2026-10-06/scoped-parity.json)
records **35,998,488 matching output lines per JAR** across **9,229,463 fixtures**.
Each supported candidate state is exercised; behavior representatives are tested
against every neighbor state, changing repeated reads, integer-coordinate
wrapping, absent/closed/open shulker snapshots and injected block/entity query
failures, plus light thresholds, independent fluid sequences and fluid-query
failures. Native method ownership
and support-tag fields supply private
definition metadata; survival results are calculated in Rust. Workspace tests,
Clippy and the **147-test Python suite** passed. This establishes the recorded
survival scope; full placement, neighbor updates, scheduled behavior and complete
block gameplay remain pending.


Collision contexts now preserve empty-context fallback heights, position-only
queries, and captured entity position, descending, main-hand and placement flags.
Height comparisons retain the native float epsilon widened to double. Null
placement factories use `-Double.MAX_VALUE`; `withPosition` sets the placement
flag and ignores the supplied height when the entity is absent. Empty item stacks
match AIR in entity/empty contexts, while position-only contexts never report a
held item. Living-entity fluid callbacks run before the same-family exclusion,
with the second fluid as their argument, and propagate failures.

`python3 tools/test_collision_context_parity.py` exercises actual native factories
and queries over **8,338 context snapshots**, comparing **225,126 output lines per
JAR**. The [context report](../benchmarks/rust-collision-context-2026-10-06/scoped-parity.json)
records the pinned artifacts and exact source/input/output hashes. The corpus
includes adjacent representable values around height thresholds, empty shapes,
integer-coordinate extremes, NaN/infinity, null entities, item count boundaries,
all five fluid types and callback failures. Native entity and stack snapshots are
allocated privately without their constructors; this checks query behavior, not
entity construction or inventory lifecycle. Inventory changes after context
capture, experimental minecart contexts and live movement integration remain
pending. Contextual block-shape selection is being added separately.


`ScaffoldingCollision` now selects stable scaffold geometry, the unstable bottom
platform, or an empty shape using the collision context. Placement contexts are
always empty. Descending affects the top surface check; the bottom platform also
requires nonzero support distance, the bottom flag and the lower height check.
No-context support and contextual collision share the same immutable stable shape.

`python3 tools/test_scaffolding_collision_parity.py` compares all **32 states**
across **56,448 fixtures**, producing **524,300 matching lines per JAR** in the
[scaffolding report](../benchmarks/rust-scaffolding-collision-2026-10-06/scoped-parity.json).
The native context dispatch executes against a world proxy that rejects all
queries. Full shape coordinates, bounds, ordered boxes and cube metadata match
for empty, position, entity and placement contexts, including adjacent doubles
at both height thresholds. Block placement, distance updates, collapse and live
entity motion through scaffolding are separate work.


`LiquidCollision` implements contextual water/lava collision selection. The
always-fluid flag returns a full cube before querying entities or neighbors.
Otherwise, flowing block levels and nonliving contexts return empty. Source
blocks request the living entity's standing shape, check its height, then read the
fluid above and invoke the entity's standing predicate. Empty candidate shapes
still follow those query rules. Above-block coordinates wrap as Java integers.

`python3 tools/test_liquid_collision_parity.py` compares all **32 water/lava block
states**, all **33 possible above-block liquid/empty states**, full selected shape
data and ordered callback traces, including failures at each query. The
[liquid-collision report](../benchmarks/rust-liquid-collision-2026-10-06/scoped-parity.json)
records **571,179 matching lines per JAR**. Standing shapes and fluid abilities are
controlled inputs in this suite; each entity type still needs its implementation.
Fluid spreading, swimming, drowning and live movement remain pending.


Powder snow now selects the native `0.9f`-high falling shape when fall distance
exceeds 2.5, a full cube for falling-block entities, and a full cube for entities
that can walk on it, are above it and are not descending. Placement and absent
entity contexts are empty. Walkability checks the entity-type tag first, then
leather boots only for living entities; successful tags skip equipment queries.
The API takes current entity metadata and lazy tag/equipment providers.

`python3 tools/test_powder_snow_collision_parity.py` compares **63,336 fixtures**
and **599,180 matching lines per JAR** in the
[powder-snow report](../benchmarks/rust-powder-snow-collision-2026-10-06/scoped-parity.json).
The native block method and walkability predicate execute over controlled entity
snapshots. The corpus covers fall-distance and top-height boundaries, nonfinite
values, falling-block instances, item counts, query failures and skipped queries.
Entity tag loading, freezing, sinking and live movement remain separate work.


`BlockCollisionRegistry::collision_shape_with_context` now combines the contextual
scaffolding, liquid and powder-snow rules with the existing cached/dynamic paths.
It dispatches these overrides before consulting cached geometry. The pinned
native audit checks **35,492 context-independent cached states** at seven positions
with both interface and entity contexts that reject every query: each returns the
identical cache object without world reads. The **32 cached liquid states** query
the context and take their separate behavior path. The **199 uncached states**
continue through explicit behavior implementations.

`python3 tools/test_contextual_collision_parity.py` validates integrated dispatch
for all **35,723 states**, **11 contexts** and both fluid-flag inputs:
**785,906 fixtures** and **2,391,334 matching output lines per JAR** in the
[contextual-registry report](../benchmarks/rust-contextual-collision-2026-10-06/scoped-parity.json).
Outputs compare cache-relative shape equality or full uncached geometry, plus
ordered world/entity callbacks. This integration corpus supplies missing block
entities; active shulker and piston snapshots retain their separate suites.
These checks establish the recorded collision-query scope, not entity movement
or whole-game parity.


Private shape exports now carry explicit canonical-identity markers. The importer
restores the global empty/full-block shapes only when their complete grid metadata
matches, and rejects duplicate or malformed canonical claims. Equal unmarked
shapes keep distinct identities. Older exports remain readable and report
`has_canonical_identities() == false`; they cannot establish the canonical identity
needed by vanilla's block-collision iterator fast path.

The [canonical-identity report](../benchmarks/rust-canonical-collision-2026-10-06/scoped-parity.json)
adds identity checks for every cached state: **5,566 canonical empty**, **3,348
canonical full-block** and **26,610 other cached states**, with **1,132,980 matching
output lines per JAR**. The integrated contextual comparison also passes again
with **2,391,334 lines per JAR** in its
[regression report](../benchmarks/rust-canonical-collision-2026-10-06/contextual-regression.json).
Five import tests cover legacy data, equal-but-distinct grids, canonical restoration
and malformed identity metadata. Workspace Clippy passes without warnings.


`BlockCollisionScan` now implements lazy cursor selection around entity bounds.
It retains the native epsilon-expanded cursor, X/Y/Z visit order, skipped corners,
large-shape face filtering, moving-piston edge filtering and suffocation-before-
shape query order. Only successful chunk lookups are reused; absent chunks are
queried again. Shape selection uses the global collision world, while suffocation
uses the selected chunk. Canonical full blocks use native AABB intersection;
other shapes use moved voxel intersection. Returned positions are independent
values. Providers supply current block metadata and contextual shape selection.

`python3 tools/test_block_collision_scan_parity.py` compares **4,176 fixtures**
and **517,820 matching lines per JAR** in the
[collision-scan report](../benchmarks/rust-block-collision-scan-2026-10-06/scoped-parity.json).
The native iterator runs over controlled metadata, shapes and chunk availability;
traces cover early termination, missing chunks, query failures, thin/zero-width
bounds, integer-coordinate wrapping and equal shapes with different canonical
identities. Two Rust API tests cover finite/bounded input requirements, cursor
visit budgets and termination after errors. Unsupported cursor volumes return
errors; Java's overflowing iteration counts are outside this bounded API.
Registry/world integration and live movement still need implementation.


`BlockCollisionRegistry` now prepares native cursor-filter flags for every state:
uncached shapes are large, while cached shapes are large when any axis extends
outside the unit block. Moving-piston identity is bound to the same registry.
`SuffocationRegistry` binds the native predicate definitions and resolved
`causes_suffocation` tag. Default predicates combine tag membership with full
collision geometry; overrides preserve piston extension, constant predicates and
lazy shulker block-entity checks. Missing or wrong-type shulker block entities
count as suffocating. Tag changes require rebinding the suffocation registry.

`python3 tools/test_suffocation_parity.py` checks all **35,723 states** with natural,
empty and all-block tag bindings at four positions, including integer extremes.
Every shulker state also covers absent/wrong block entities, all animation states
and query failures. The [suffocation report](../benchmarks/rust-suffocation-2026-10-06/scoped-parity.json)
records **1,697,193 matching lines per JAR**, including scan flags and query traces.
These are registry predicates; live collision-world binding, movement and
suffocation damage remain separate integration work.
The [contextual regression](../benchmarks/rust-suffocation-2026-10-06/contextual-regression.json)
also passes again with **2,391,334 lines per JAR**, and its all-state native audit
now compares large-shape and moving-piston flags. Workspace Clippy passes.


`RegistryCollisionWorld` now binds `BlockCollisionScan` directly to registered
metadata, contextual shapes and suffocation rules. Chunk providers return current
state IDs and a chunk-local block-entity provider; contextual collision uses the
global world. Construction rejects legacy collision exports without complete
canonical identity metadata, because their unit-block fast paths can differ.

`python3 tools/test_registry_collision_scan_parity.py` compares native
`BlockCollisions` over resident `ProtoChunk` sections with the Rust binding over
`TerrainChunk` sections. All registered states cover ordinary and suffocating-only
selection. Additional scenes cover partial/dynamic blocks, chunk and build-height
boundaries, missing chunks, replacements, early termination and query failures.
Shulker fixtures deliberately give the chunk and global providers different
entities to check routing. The [resident collision report](../benchmarks/rust-resident-collision-scan-2026-10-06/scoped-parity.json)
records **3,046,929 matching output lines per JAR**. Six collision import/binding
tests pass, including legacy-metadata rejection; workspace Clippy passes.
Live chunk loading, entity lifecycle, movement and the playable client remain
separate integration work.


`resolve_world_movement` now joins registry-backed block scanning to the native
step-up decision. It selects entity colliders once, including for zero/underflow
movement, then collects applicable border and block shapes in native order for
the initial and optional step query. `try_resolve_step_movement` accepts owned or
borrowed collider lists and propagates provider failures without returning a
partial displacement. The shared implementation avoids allocating reference
lists for owned shapes; existing borrowed callers retain the same API.

`python3 tools/test_world_movement_parity.py` runs actual `Entity.collide` over
registered blocks and resident Java sections, comparing the Rust world collector
and step solver. Every registered state appears as an obstacle; additional cases
cover step heights, grounded/falling motion, ceilings, partial/dynamic blocks,
chunk boundaries, zero/underflow motion and failures during collection. The
[world movement report](../benchmarks/rust-world-movement-2026-10-07/scoped-parity.json)
records **7,062,798 matching lines per JAR**. Entity collider lists, border
proximity/shapes and collision contexts are controlled fixture inputs. Actual
entity selection, border state, travel/gravity/velocity, grounded side effects
and playable integration remain pending.

The [geometry regression](../benchmarks/rust-world-movement-2026-10-07/geometry-regression.json)
passes again with **1,320,187 lines per JAR**, including the existing native step
fixtures. All six movement unit tests and workspace Clippy pass. New tests cover
owned-query failures during both initial and step collection and signed zero
preservation when collection is skipped.


`entity_collision_shapes` now implements `EntityGetter.getEntityCollisions`
selection over ordered entity snapshots: average-size short circuit, epsilon
inflation, exact bounds/excluded-identity filtering, spectator suppression and
lazy collision-policy callbacks. With a source entity it invokes that entity's
`canCollideWith`; without one it invokes each candidate's
`canBeCollidedWith(null)`. Selected bounds become shapes after policy evaluation.
Providers retain responsibility for live storage and each entity type's policy.

`python3 tools/test_entity_collision_parity.py` compares the actual native getter,
default vehicle-root checks and controlled policy overrides, including failures
at query/callback boundaries. Tiny/zero-width and nonfinite query boxes, inflated
boundary contacts, spectators, shared vehicles, null sources and ordered shape
outputs are covered. The [entity collision report](../benchmarks/rust-entity-collision-2026-10-07/scoped-parity.json)
records **158,558 matching lines per JAR**. Workspace Clippy passes. This adds the
selection helper needed by movement; live entity indexing, lifecycle and
entity-type policy implementations remain pending.


`BorderGeometry` now models stationary and moving world-border extents, cached
stationary collision geometry, center/absolute-limit changes, tick progression,
partial-tick bounds and movement proximity. The pinned version decrements size
transition progress once per tick. Ordinary bounds and collision shapes use the
previous size while moving; completing the transition installs a stationary
extent at the target. Collision geometry subtracts the floor/ceil-rounded inner
column from the infinite outer shape. Boundary predicates retain their asymmetric
inclusive minimum/exclusive maximum tests and native floating-point order.

`python3 tools/test_world_border_parity.py` compares actual `WorldBorder` mutation
sequences, extent metadata, complete shape geometry and proximity predicates.
The [border report](../benchmarks/rust-world-border-2026-10-07/scoped-parity.json)
records **1,138,830 matching lines per JAR**, including zero/large durations,
wrapping start timestamps, completion, retargeting, signed zero, extreme finite
centers/sizes and nonfinite query/interpolation inputs. The supported construction
and mutation domain requires finite nonnegative sizes, finite centers and
nonnegative absolute limits/durations. A unit test verifies invalid parameters
leave the existing extent unchanged; workspace Clippy passes. Saved-data codecs,
listeners, dirty-state events, damage, synchronization and live movement binding
remain separate work.


`restitute_movement` now implements the pinned post-collision velocity calculation
with lazy entity/block bounce queries, gravity thresholds, nonliving float
scaling and air-drag interpolation. It preserves horizontal/vertical collision
branches, callback ordering, the position-sync field write and final velocity
assignment. Errors retain already performed effects instead of rolling back the
world or returning a fabricated successful velocity.

`python3 tools/test_restitution_parity.py` executes actual
`Entity.restituteMovementAfterCollisions` on living and nonliving snapshots with
controlled block restitution and suppression tags. The
[restitution report](../benchmarks/rust-restitution-2026-10-07/scoped-parity.json)
records **177,090 matching lines per JAR**. Fixtures cover changing lazy getter
values, signed zero/nonfinite arithmetic, bounce callbacks that alter velocity,
and failures before/after each side effect. Workspace Clippy passes. Registered
block property/tag provisioning, actual bounce sounds/events, `Entity.move`
guards, fall damage, grounding and full player simulation remain pending.


`find_supporting_block` now selects the nearest colliding block center using the
native distance arithmetic and wrapping Y/Z/X position tie-break. `GroundState`
implements the thin foot query, previous-horizontal-footprint fallback and both
ground/collision flag updates. Null movement preserves the prior support in the
native branch where no fallback query can run; query failures retain preceding
flag writes.

`python3 tools/test_ground_support_parity.py` compares actual
`CollisionGetter.findSupportingBlock`, `Entity.checkSupportingBlock` and both
`setOnGroundWithMovement` overloads. The
[ground support report](../benchmarks/rust-ground-support-2026-10-07/scoped-parity.json)
records **342,709 selection lines and 151,444 state-transition lines per JAR**,
covering ties, nonfinite entity positions, missing chunks, query errors, nullable
movement and sequential changes of support. Shape/context snapshots are controlled;
registry-backed scanning has separate integration coverage. Workspace Clippy
passes. Full live entity movement, support-dependent effects and gameplay remain
pending.
The [shared scanner regression](../benchmarks/rust-ground-support-2026-10-07/scan-regression.json)
also passes with **517,820 matching lines per JAR** after extending its fixtures.


`movement_support` now implements support-dependent positions, the three public
offset variants and base-entity block jump/speed factors. It preserves the support
X/Z coordinates, conditional fence/wall/gate lookup, float-to-double offset
widening, cached entity block position and lazy block queries. Jump-factor lookup
always reads the lower block; speed lookup can short-circuit for water, bubble
columns or a non-unit current factor.

`python3 tools/test_movement_support_parity.py` compares the actual native methods
with controlled state/tag/property snapshots. The
[movement support report](../benchmarks/rust-movement-support-2026-10-07/scoped-parity.json)
records **51,572 matching lines per JAR**, including float boundaries, nonfinite
positions/offsets/factors, tag/type short circuits and errors at every query.
Workspace Clippy passes. Registered movement-property provisioning, living-entity
efficiency and the full movement lifecycle remain pending.


`MovementProperties` imports version-bound raw friction, speed, jump, bounce and
fall-distance-reduction fields plus fence-gate type. `BoundMovementProperties`
binds resolved fence/wall/bounce-suppression tags; rebinding leaves previous
snapshots unchanged. `RegistryMovementWorld` connects real state IDs from a block
provider to the support-position and factor calculations, rejecting unknown IDs.

`python3 tools/test_movement_registry_parity.py` independently checks native
property getters for every state, then runs actual entity support/factor queries
over all **35,723 states / 1,286 blocks** under normal, empty and universal tags.
Mixed-block scenes and failing providers check query order. The
[registered movement report](../benchmarks/rust-movement-registry-2026-10-07/scoped-parity.json)
records **2,300,888 matching lines per JAR**. Three import/binding tests and
workspace Clippy pass. This provisions movement data; full player travel, live
entity state and block bounce effects remain separate integration work.


`PistonMovement` now accumulates displacement per axis and resets it when the
game tick changes. It preserves the initial squared-length short circuit, lazy
time lookup, first nonzero axis, the +/-0.51 cumulative clamp and float-derived
small-displacement threshold. A failed time lookup leaves the accumulator intact.

`python3 tools/test_piston_movement_parity.py` runs actual
`Entity.limitPistonMovement` across sequential tick/push inputs. The
[piston movement report](../benchmarks/rust-piston-movement-2026-10-07/scoped-parity.json)
records **325,197 matching lines per JAR**, including repeated/opposing pushes,
threshold neighbors, signed zero, nonfinite inputs and injected failures.
Workspace Clippy passes. Piston block events, entity pushing and the surrounding
`Entity.move` lifecycle remain pending.
The [registered movement regression](../benchmarks/rust-piston-movement-2026-10-07/movement-registry-regression.json)
passes again with **2,300,888 lines per JAR** on the integrated sources.


`no_world_collision` implements the entity-bearing collision-free query over the
registry-bound world. It consumes block collisions lazily, then checks whether
the selected entity list is empty, then tests applicable border geometry. Each
stage short-circuits the later providers; a nonempty entity list blocks the query
even if its supplied shape is empty. Collision contexts are supplied explicitly.

`python3 tools/test_world_collision_check_parity.py` executes native
`CollisionGetter.noCollision` over registered blocks and resident Java sections.
The [world collision-check report](../benchmarks/rust-world-collision-check-2026-10-07/scoped-parity.json)
records **4,793,095 matching lines per JAR**. All states, context variants, empty
and disjoint controlled entity/border shapes, missing chunks, boundaries and
provider failures are covered. Workspace Clippy passes. Null-entity overloads,
actual entity-list/border providers and live player ledge behavior remain pending.
The [shared movement regression](../benchmarks/rust-world-collision-check-2026-10-07/movement-regression.json)
also passes with **7,062,798 matching lines per JAR**.


`back_off_from_edge` implements the player ledge guards, above-ground test and
X/Z/diagonal displacement reductions. It retains lazy step/staying/grounded
queries, float step widening, signed-zero behavior and the exact thin collision
probe arithmetic. A caller-supplied query budget bounds nonconverging inputs and
returns an error without fabricating a successful displacement.

`python3 tools/test_edge_backoff_parity.py` executes actual player methods with
controlled collision-free query results. The
[edge backoff report](../benchmarks/rust-edge-backoff-2026-10-07/scoped-parity.json)
records **46,578 matching lines per JAR**, covering all mover types, flight/crouch
guards, fall/step and 0.05 thresholds, nonfinite values and query failures. Two
query-budget unit tests and workspace Clippy pass. Registered-world ledge probes,
live crouching/attribute state and the complete player movement loop are separate
integration work.


Registered-world ledge integration now runs `back_off_from_edge` through
`no_world_collision` and registry-bound resident chunk scans. The native side
executes actual `Player.maybeBackOffFromEdge` and its collision probes over
`ProtoChunk` sections. Every state is used as a supporting block, with additional
falling, step-height, chunk-edge, diagonal, dynamic-block and guard/error cases.

`python3 tools/test_world_edge_backoff_parity.py` passes with
**27,109,699 matching lines per JAR** in the
[world ledge report](../benchmarks/rust-world-edge-backoff-2026-10-07/scoped-parity.json).
The shared fixture now supports player probes alongside direct movement and
collision checks; workspace Clippy passes. Entity lists, border applicability and
collision contexts remain controlled snapshots. A complete player movement/travel
loop and playable client remain pending.


`check_fall_damage` now implements base-entity fall-distance accumulation and
landing order. Downward displacement narrows to float before being subtracted
from the double fall distance. Grounded landings call the block, read the
possibly changed position/support, emit the ground event and reset fall distance.
Errors preserve completed field writes and callback effects.

`python3 tools/test_fall_damage_parity.py` runs actual `Entity.checkFallDamage`
with controlled water, landing and event providers. The
[base fall report](../benchmarks/rust-fall-damage-2026-10-07/scoped-parity.json)
records **44,566 matching lines per JAR**, including nonfinite arithmetic, float
underflow, callback mutations and failures at every boundary. Workspace Clippy
passes. Living-entity fluid/particle behavior, registered block landing effects,
health/damage and full movement remain pending.


`living_fall` implements default living-entity fall power/damage calculation,
impulse-height exemption and landing damage callback order. It retains separate
immunity checks for passenger propagation and damage calculation, double/float
arithmetic and saturating floor-to-int conversion. Positive damage resets the
impulse context, selects the fall sound, plays block landing sound and dispatches
hurt. A rejected hurt callback does not change the native positive-damage result.
`ImpulseContext` also models setter, grace-extension and reset behavior; disabling
ignore clears grace time while retaining the impact position until a later reset.

`python3 tools/test_living_fall_parity.py` compares actual living methods and
sequential impulse changes. The
[living fall report](../benchmarks/rust-living-fall-2026-10-07/scoped-parity.json)
records **153,025 matching lines per JAR**, including changing immunity, passenger
callback mutations, nonfinite arithmetic, sound threshold selection and failures
after partial state changes. Workspace Clippy passes. Attribute provisioning,
passenger storage/recursion, actual health processing, tick/save/network behavior
and the complete movement lifecycle remain pending.


`push_entities_up` implements the block-replacement push: query old/new shapes,
subtract old occupied space without optimizing the join, translate to world
coordinates and select entities using the added shape's bounds. It reads each
current entity box before calculating its upward displacement and invokes a
relative teleport even when that displacement is zero.

`python3 tools/test_block_push_parity.py` compares actual `Block.pushEntitiesUp`
with controlled shapes and selected entity lists. The
[block push report](../benchmarks/rust-block-push-2026-10-07/scoped-parity.json)
records **101,006 matching lines per JAR**, including empty/redundant geometry,
partial shapes, large coordinates, nonfinite entity boxes, mutations of later
entities and failures after partial effects. Workspace Clippy passes. Registered
replacement dispatch, actual entity selection and teleport/network semantics
remain separate integration work.


`block_landing` binds audited native landing/bounce families to every registered
state. It implements fall-distance reduction, hay/honey damage multipliers,
slime suppression, powder-snow fall sounds, upward dripstone tips, farmland
replacement with occupant pushes, turtle-egg destruction/decrement and shelf
mushroom bounce sounds. Random draws, server-class checks, client flags, spawn
protection, mob griefing, sounds, writes and events retain native order.

`python3 tools/test_block_landing_parity.py` invokes registered native methods for
all **35,723 states / 1,286 blocks** with eight entity class families. The
[registered landing report](../benchmarks/rust-block-landing-2026-10-07/scoped-parity.json)
records **3,524,267 matching lines per JAR**, including float/double boundaries,
NaN/infinity, failed writes, callback failures, egg hatch preservation and
farmland collision pushes. Targeted Clippy passes. Actual health, live entity
selection/storage, teleport/network behavior, audio playback and full movement
lifecycle integration remain separate work.


`movement_emission` accumulates native movement/flight distances, evaluates
climbable and powder-snow states, and dispatches step/swim/flap effects. Legacy
and current support blocks retain their separate sound/event rules. Threshold
updates use Java float-to-int saturation and wrapping integer addition, and
sound/event callbacks can change fields observed by subsequent operations.

`python3 tools/test_movement_emission_parity.py` compares actual native movement
emission, block vibration dispatch, flapping and next-step arithmetic. The
[movement emission report](../benchmarks/rust-movement-emission-2026-10-07/scoped-parity.json)
records **598,611 matching lines per JAR** across **53,036 fixtures**, including
predicate combinations, sound/event modes, differing support blocks, nonfinite
numbers, mutations and provider failures. Targeted Clippy passes. Sound callbacks
are controlled and the crystal-sound predicate is false in this gate; registered
sound selection, audio playback and full movement integration remain pending.


`entity_move` sequences native movement phases: no-physics/piston early exits,
stuck multiplier reset, edge backoff, collision resolution, fall-reset clipping,
position/recording writes, collision/ground flags, landing, removal, restitution,
movement emission and block speed multiplication. `MovementHistory` retains at
most 100 records by merging the oldest pair and dropping their original-axis
movement metadata. Callbacks can change fields read by later phases.

`python3 tools/test_entity_move_parity.py` invokes actual `Entity.move` with native
shape resolution and zero restitution. The
[movement lifecycle report](../benchmarks/rust-entity-move-2026-10-07/scoped-parity.json)
records **543,653 matching lines per JAR**, covering signed zero, tiny/nonfinite
movement, collisions, early exits, recording cap, authority guards, landing
mutations/removal and failures after partial writes. Targeted Clippy passes.
This gate controls piston/edge decisions, ground support, ray hits and landing
callbacks; step height is zero and non-passenger emission is tested separately.
Binding all phases to a live entity/world and player travel remains pending.


`world_ray` implements ordered block/fluid ray clipping, nearest-hit selection
with block precedence on ties, and interaction-shape overrides that replace only
the hit face. It also implements native fluid pick modes and fall-reset shape
selection, including player type identity, end portals/gateways and server-side
zero-delay Nether portals. Traversal limits and invalid geometry return errors.

`python3 tools/test_world_ray_parity.py` compares actual `BlockGetter.clip` and
native `ClipContext` modes. The
[world ray report](../benchmarks/rust-world-ray-2026-10-07/scoped-parity.json)
records **248,725 matching lines per JAR**, including inside hits, ties, misses,
interaction geometry, short/signed-zero rays, lazy predicates/rules and provider
failures. Targeted Clippy passes. Separate limit tests distinguish a miss from
nonfinite geometry, exhausted traversal budget and failed world reads. Shapes
and state predicates are controlled; registered shape binding, actual fluid
shape caching and full entity/world integration remain pending.


`FluidShapeCache` matches native registered-fluid shape caching. The first
successful lookup of a nonempty fluid state samples the fluid above and retains
its resulting shape across later positions/world views. Failed reads leave the
state uncached. Empty and full cube shapes preserve canonical identity; distinct
noncanonical cached shapes retain their own identities.

`python3 tools/test_fluid_shape_parity.py` runs every registered native fluid state
in changing query sequences. The
[fluid shape report](../benchmarks/rust-fluid-shape-2026-10-07/scoped-parity.json)
records **387,950 matching lines per JAR** across **5,181 sequences**, comparing
geometry, cache population, canonical/alias identities, query suppression and
failures. Targeted Clippy passes. The pinned registry has no amount-nine state;
unsupported custom fluid definitions are outside this module. Fluid propagation,
entity immersion and live world/ray integration remain pending.


`InteractionShapes` imports a complete state-bound table with a distinct table
kind and mandatory canonical identity metadata. The pinned owner audit finds
only default empty, cauldron, composter, hopper and scaffolding implementations;
all are static per state. Geometry uses the existing validated discrete shape
importer while preserving eight native shape identities.

`python3 tools/test_interaction_shape_parity.py` compares every registered state
and actual native interaction-face overrides with supplied collision geometry.
The [interaction shape report](../benchmarks/rust-interaction-shape-2026-10-07/scoped-parity.json)
records **188,682 matching lines per JAR** across all **35,723 states**. Native
interaction-shape world queries are rejected by the fixture. Import checks reject
wrong table kind, partial mappings, missing canonical metadata, foreign state
ordering and invalid IDs. Targeted Clippy passes. Registered outline/visual
shapes, complete ray-world binding and gameplay integration remain pending.


`FallRayRegistry` binds fall-reset tags, portal identities, interaction geometry
and fluid states to the complete registered block-state table. `fall_reset_ray`
uses that binding with the shared fluid-shape cache and native water selection.
Tag reloads produce new validated registry snapshots; unknown tag members and
invalid state IDs are rejected.

`python3 tools/test_registry_fall_ray_parity.py` compares actual registered
`BlockGetter.clip` behavior with native block/fluid tags and shape getters. The
[registered fall-ray report](../benchmarks/rust-registry-fall-ray-2026-10-07/scoped-parity.json)
records **3,024,623 matching lines per JAR** across **120,297 fixtures**, including
all **35,723 states**, tag reloads, server/client distinctions, player type checks,
portal delays, changing above-fluid states, shared cache reuse and failures.
Targeted Clippy passes. World storage is controlled; live entity movement and
complete player/world gameplay integration remain pending.


`living_landing` wraps base fall handling with fluid refresh, server-only changed
block notification and landing particles, followed by climbing-position cleanup.
Particle positions preserve the native support-block edge projection and capture
entity coordinates before later callbacks. Attribute, fluid, particle and block
callbacks can mutate fields; failures retain preceding writes.

`python3 tools/test_living_landing_parity.py` invokes actual
`LivingEntity.checkFallDamage` and its base implementation. The
[living landing report](../benchmarks/rust-living-landing-2026-10-07/scoped-parity.json)
records **244,844 matching lines per JAR**, including server/nonserver levels,
water changes, numeric boundaries, particle locations/counts, callback failures,
field mutations and climbing cleanup. Targeted Clippy passes. Fluid immersion,
changed-block enchantments, particle rendering and live gameplay binding remain
pending; those providers are controlled by this gate.


`living_air` implements input normalization/rotation, friction-influenced speed,
ladder/scaffolding limits, relative movement, collision-triggered climbing and
air travel. Ground friction is captured before movement while levitation,
gravity and drag are sampled afterwards. Player class identity controls ladder
slide suppression. Native float arithmetic and quantized trigonometry are kept.

`python3 tools/test_living_air_parity.py` invokes the actual native private air,
relative movement, climbing, speed, friction and input-vector methods. The
[air travel report](../benchmarks/rust-living-air-2026-10-07/scoped-parity.json)
records **443,554 matching lines per JAR**, including both player/nonplayer
classes, changed ground/climbing predicates, missing client chunks, levitation,
nonfinite boundaries, callback mutations and failures. Targeted Clippy passes.
Extreme effect amplifiers are injected directly for integer-boundary checks;
normal constructor clamping is outside this gate. Collision movement, gravity,
attributes and immersion remain controlled providers; live gameplay binding and
complete player travel are pending.


`living_fluid` adds native water/lava acceleration, slowdown, falling adjustment,
shallow-lava handling, collision escape and ridden buoyancy. It shares input
rotation with air movement. Fluid heights and escape queries remain lazy, with
movement mutations visible to subsequent velocity updates.

`python3 tools/test_living_fluid_parity.py` compares the actual native travel and
fluid helper methods. The
[fluid travel report](../benchmarks/rust-living-fluid-2026-10-07/scoped-parity.json)
records **265,100 matching lines per JAR**, including sprint changes, movement
efficiency, dolphin effects, shallow-fluid boundaries, escape probes, ridden
float guards, nonfinite values, callback mutations and failures. Targeted Clippy
passes. Collision movement, immersion depths, free-space queries, attributes and
gravity are controlled; live fluid propagation and gameplay binding remain
pending.


`living_motion` implements default living jump strength/boost, sprint jump
impulses, gravity and slow falling, vertical liquid input, rider flight speed
and air drag. Native finite-addition filtering and delayed synchronization flags
preserve partial writes when providers fail.

`python3 tools/test_living_motion_parity.py` compares the corresponding actual
living methods plus `Entity.applyGravity` and `addDeltaMovement`. The
[jump and gravity report](../benchmarks/rust-living-motion-2026-10-07/scoped-parity.json)
records **113,623 matching lines per JAR**, including float thresholds, extreme
raw effect amplifiers, signed zero/nonfinite values, rider classes, callback
mutations and failures. Targeted Clippy passes. Entity-type jump/flight overrides,
effect and attribute storage, registered jump-factor binding and live gameplay
integration remain pending.


`player_travel` wraps living travel with passenger bypass, swimming pitch
adjustment, head-fluid checks and preservation of creative-flight vertical
velocity. It also binds player flight speed to sprint/passenger/ability rules
and implements the flying ability's fluid-affection predicate.

`python3 tools/test_player_travel_parity.py` invokes actual `Player.travel`,
`getFlyingSpeed` and `isAffectedByFluids`. The
[player travel report](../benchmarks/rust-player-travel-2026-10-07/scoped-parity.json)
records **120,933 matching lines per JAR**, including ability and velocity
mutations, callback failures, signed zero/nonfinite positions and the distinction
between direct ability fields and virtual ability access. Targeted Clippy passes.
Parent travel is selected through a controlled fluid callback; connecting the
verified living phases and collision movement to the live player/world remains
pending.


`ClimbingRegistry` binds resolved climbing/glide-through tags and actual trapdoor
class identities to all registered states. `on_climbable` preserves spectator
and gliding guards, open-trapdoor/ladder facing checks, wrapping below-position
arithmetic and last-climbable position writes. Negative or failed checks retain
the previous climbing position. Unknown members/properties and IDs are rejected.

`python3 tools/test_climbing_parity.py` invokes actual registered
`LivingEntity.onClimbable` and trapdoor support checks. The
[climbing report](../benchmarks/rust-climbing-2026-10-07/scoped-parity.json)
records **3,820,054 matching lines per JAR** across all **35,723 states**, including
all trapdoor states against every registered ladder facing, tag reloads,
spectator/gliding guards, captured positions, mutations and failures. Targeted
Clippy passes. Entity/world storage is controlled; live movement binding and
equipment-based glide eligibility remain pending.


`fluid_occupancy` implements section-ordered liquid checks over resident block
sections with a complete registered fluid-presence table. `BlockSection.any_state`
checks active palette entries so dry sections skip cell unpacking. Bounds use
inclusive floored endpoints, and chunk requests retain native behavior even when
no requested vertical section is within world height. Invalid geometry, foreign
registries and query-budget exhaustion return explicit errors.

`python3 tools/test_fluid_occupancy_parity.py` compares actual
`LevelReader.containsAnyLiquid`, `BoxBlockMatcher` and `BlockScanUtils` with
native ProtoChunk sections. The
[liquid occupancy report](../benchmarks/rust-fluid-occupancy-2026-10-07/scoped-parity.json)
records **116,664 matching lines per JAR**, including fluid presence for all
**35,723 states**, public chunk/section query order, height boundaries, palette
changes/compaction and failures. Rust-only checks verify typed budget, nonfinite
and registry errors. Targeted Clippy passes. Chunk snapshots repeat controlled
resident data; collision composition for `Entity.isFree` and live world binding
remain pending. No performance gain is claimed without measurement.


`free_space.offset_is_free` composes native offset bounds, registry-bound
block/entity/border collision clearance and resident liquid occupancy. The
liquid scan runs only after collision clearance succeeds. Both phases share
resident TerrainChunk storage through `RegistryCollisionWorld`; their typed
errors and separate query budgets remain visible to the caller.

`python3 tools/test_free_space_parity.py` invokes actual `Entity.isFree`, native
collision iteration and liquid scanning on the corresponding resident ProtoChunk
storage. The [free-space report](../benchmarks/rust-free-space-2026-10-07/scoped-parity.json)
records **5,764,046 matching lines per JAR** across all **35,723 states**, offsets,
fluid/context variants, height boundaries, required missing chunks and failures.
Targeted Clippy passes. Entity lists, border applicability and collision contexts
are controlled; chunk generation, live entity indexing and full playable
integration remain pending.
The [collision-only regression](../benchmarks/rust-free-space-2026-10-07/collision-regression.json)
also passes **4,793,095 matching lines per JAR** after extending the shared
fixture and registry binding.


`entity_fluid` tracks body depth, eye immersion and current accumulation across
updates. It preserves one-tick tracker retention, missing-chunk preflight,
ordered cell scans, the first matching current tag, camera support checks and
partial writes on query failure. Current application distinguishes player
averaging from other entities' normalization and dispatches the final velocity
addition with its finite-input filter and fresh velocity read.

`python3 tools/test_entity_fluid_parity.py` invokes actual
`EntityFluidInteraction` and `Entity.addDeltaMovement`. The
[entity fluid report](../benchmarks/rust-entity-fluid-2026-10-07/scoped-parity.json)
records **607,338 matching lines per JAR** with real ProtoChunk fluid-presence
sections, ordered/reloaded overlapping tags, wet/dry sequences, eye boundaries,
nonfinite heights/current arithmetic, mutations and injected failures. Rust-only
checks cover explicit query-budget/nonfinite errors and reset-before-error
behavior. Targeted Clippy passes. Fluid heights/flows and entity fields are
controlled here; neighboring-cell flow calculation, live world/entity binding
and full playable integration remain pending. Extreme-coordinate Java scans are
bounded by injected callback failures rather than allowed to traverse billions
of cells.


`fluid_flow` computes water/lava current vectors from ordered horizontal and
lower neighbors, preserving f32 height subtraction before f64 accumulation.
Falling fluids apply the native wall deflection, including the same-family,
upward-face and IceBlock guards, then normalize the resulting vector.

`python3 tools/test_fluid_flow_parity.py` invokes actual registered
`FluidState.getFlow`, `FlowingFluid.getFlow` and its solid-face helper. The
[fluid flow report](../benchmarks/rust-fluid-flow-2026-10-07/scoped-parity.json)
records **1,051,071 matching lines per JAR** from **51,846 fixtures** covering all
registered fluid states, neighboring families/heights, per-face wall flags,
falling deflection, wrapping coordinates and query failures. Targeted Clippy
passes. Block tag/class flags and face support are controlled in this comparison;
binding those to registered block storage and support shapes, fluid spreading
and complete gameplay remain pending.


`registry_fluid` now binds contained fluid states, resolved flow-blocking tags and
actual IceBlock class membership to the full block registry. Its world adapter
composes current/height/camera queries with cached and dynamic support shapes.
Foreign registry identities, unknown tag members and invalid state IDs are
rejected explicitly; tag reloads use a new immutable binding.

`python3 tools/test_registry_fluid_parity.py` compares actual registered height,
camera immersion, current vectors and solid-face queries. The
[registered fluid report](../benchmarks/rust-registry-fluid-2026-10-07/scoped-parity.json)
records **2,446,969 matching lines per JAR** from **409,389 fixtures**, covering all
**35,723 block states**, waterlogged contents, cached/dynamic support, natural and
reloaded tags, neighboring fluid families, coordinate wrapping and failures.
Targeted Clippy passes, including checks for typed invalid-ID and foreign-registry
errors. World cells are controlled and block entities are absent in this suite;
active block-entity lifecycle, live entity binding, fluid spreading and complete
playable integration remain pending.


`entity_fluid_tick` composes immersion updates with entity water-entry behavior
and current application. It captures water/lava membership before effects,
resets fall distance in water, dispatches a splash after the first tick when
newly entering water, updates the touching-water flag and queries the push
predicate again before applying forces. Lava uses the dimension's FAST_LAVA
attribute to select its current coefficient.

`python3 tools/test_entity_fluid_tick_parity.py` invokes actual
`Entity.updateFluidInteraction` with native immersion and current application.
The [entity fluid tick report](../benchmarks/rust-entity-fluid-tick-2026-10-07/scoped-parity.json)
records **953,830 matching lines per JAR**, including changing push predicates,
first-tick/entry flags, simultaneous water/lava, attribute selection, nonfinite
arithmetic, callback mutations and failures around partial writes. The
[immersion regression](../benchmarks/rust-entity-fluid-tick-2026-10-07/immersion-regression.json)
also passes **607,338 lines per JAR** after extending the shared fixtures.
Targeted Clippy passes. Splash dispatch is controlled; audio/particle generation,
full live entity/world binding and playable integration remain pending.


`RegistryFluidWorld` now implements entity immersion queries through the bound
fluid contents, height calculations, camera support and neighbor-derived current
vectors. Entity fields and loaded chunk/section views remain supplied by the
world provider, preserving captured positions and partial-write order.

`python3 tools/test_registered_entity_fluid_parity.py` invokes actual
`EntityFluidInteraction` with registered fluid and support behavior. The
[registered immersion report](../benchmarks/rust-registered-entity-fluid-2026-10-07/scoped-parity.json)
records **6,550,126 matching lines per JAR**, including all **35,723 states** in
immersed and above-eye cells, mixed water/lava neighborhoods, waterlogged blocks,
ordered/reloaded tags, current velocities, tracker retention and query failures.
Targeted Clippy passes. Shared-fixture regressions also pass:
[tick composition](../benchmarks/rust-registered-entity-fluid-2026-10-07/tick-regression.json)
(**953,830 lines per JAR**) and
[controlled immersion](../benchmarks/rust-registered-entity-fluid-2026-10-07/immersion-regression.json)
(**607,338 lines per JAR**). Chunk presence and section wet flags are controlled,
and block entities are absent here; full live chunk/entity lifecycle, fluid
propagation and playable gameplay remain pending.


`swimming` implements entity/player swimming transitions, underwater/shallow-water
predicates and the fluid jump threshold. The player flying ability disables
swimming directly. Other transitions preserve short-circuit query order, prior
eye-in-water state, passenger guards, captured block position and final state
writes only after all required queries succeed.

`python3 tools/test_swimming_parity.py` compares actual Entity/Player methods.
The [swimming report](../benchmarks/rust-swimming-2026-10-07/scoped-parity.json)
records **191,142 matching lines per JAR**, including flag combinations, native
fluid tag membership, failures/mutations, and f32 eye-height boundaries/nonfinite
values. Targeted Clippy passes. Synchronized entity storage, full input/tick
integration and playable gameplay remain pending.


`fluid_box` implements default entity immersion bounds and boat passenger
waterline clipping. Entity bounds are deflated and captured before vehicle
lookup. Submerged boats retain those bounds; other boats restrict immersion to
the portion strictly above their top, returning no box when none remains.

`python3 tools/test_fluid_box_parity.py` compares actual entity/boat methods. The
[fluid box report](../benchmarks/rust-fluid-box-2026-10-07/scoped-parity.json)
records **92,822 matching lines per JAR**, including actual boat status values,
virtual modifier dispatch, thin/inverted bounds, waterline boundaries, signed
zero/nonfinite values and mutations/failures. Targeted Clippy passes. Vehicle
snapshots are controlled; live passenger lifecycle and full gameplay remain
pending.


`water_splash` generates native water-entry sound, bubble/splash particle and game
event requests. It preserves controlling-passenger identity, captured velocity,
f32 volume/pitch arithmetic, float-rounded surface height, changing entity width,
and random draws before each emitted effect. A particle budget returns an
explicit error after preserving preceding effects and RNG consumption.

`python3 tools/test_water_splash_parity.py` invokes actual
`Entity.doWaterSplashEffect` with native legacy and xoroshiro random generators.
The [water splash report](../benchmarks/rust-water-splash-2026-10-07/scoped-parity.json)
records **192,506 matching lines per JAR** across **7,840 fixtures**, including
sound thresholds, self/other passengers, particle positions/velocities, callback
mutations/failures, nonfinite arithmetic and the next RNG value after the effect.
A Rust-only budget check verifies that exhaustion preserves the two sound-pitch
random draws. Targeted Clippy passes. Sound identities and effect sinks are
controlled; audio playback, particle simulation/rendering and full gameplay
integration remain pending.


`attributes` implements basic/ranged attribute instances, ordered modifiers,
three-stage value calculation, cache invalidation and permanent-state packing.
Modifier clones retain reference identity for transient-update notifications.
Per-operation maps preserve native hash-table iteration, growth and deletion
order because modifier arithmetic is sensitive to floating-point association.
The implementation also retains native partial writes when a dirty observer
fails, stale operation entries after operation-changing updates, merge-on-apply
semantics and self-replacement behavior.

`python3 tools/test_attribute_parity.py` compares actual `AttributeInstance`
methods using every registered attribute definition. The
[attribute report](../benchmarks/rust-attributes-2026-10-07/scoped-parity.json)
records **1,404,798 matching lines per JAR** across **2,391 fixtures**, including
collision clusters, resize/shrink boundaries, seeded operation sequences,
reference identity, observer cache reads/failures, nonfinite values and signed
zero. The adapter observes iterator order, as native calculation does; Java
stream traversal can differ. Targeted Clippy passes. Registry/entity/equipment
binding, packed data/network codecs and full gameplay integration remain pending.


`attributes::entity` imports version-checked attribute definitions and entity
supplier bases, and composes them with the attribute engine. Reads fall back to
the supplier without creating instances; first instance access schedules an
update and a sync for client-syncable attributes. Mutations retain those set
memberships until individually acknowledged. Base resets use the entity's
supplier value and do not instantiate absent entries. Missing attributes and
missing modifiers remain distinguishable, including lookup after instantiation.

`python3 tools/test_entity_attribute_parity.py` compares actual `AttributeMap`
operations with **40 registered attributes** and **93 entity suppliers**. The
[entity attribute report](../benchmarks/rust-entity-attributes-2026-10-07/scoped-parity.json)
records **1,771,555 matching lines per JAR** across **4,720 fixtures**. Targeted
Clippy passes. The exporter asserts that registered default suppliers contain no
modifiers. Set membership is observed in registry order: this does not establish
JVM identity-hash collection order, cross-attribute bulk assignment/serialization,
custom suppliers, equipment integration or a live entity tick.


`food` implements eating, saturation, exhaustion, hunger predicates and the
server hunger tick. It retains the captured level/difficulty, short-circuit
player queries, fast/slow regeneration timers, starvation health/difficulty
guards, wrapping integer arithmetic, Java float edge behavior and partial state
writes when callbacks fail. Heal and starvation requests use explicit world
hooks, which can mutate food state before subsequent tick work.

`python3 tools/test_food_parity.py` compares actual `FoodData` methods. The
[food report](../benchmarks/rust-food-2026-10-07/scoped-parity.json) records
**180,878 matching lines per JAR** across **15,211 fixtures**, including 240-tick
sequences, threshold boundaries, all difficulties, callback mutation/failure,
signed zero and nonfinite arithmetic. Targeted Clippy passes. World rules,
player health/healing and damage delivery are controlled; consumables, saved
hunger data and live survival gameplay integration remain pending.


`food_storage` reads/writes hunger state in player NBT compounds. Missing and
nonnumeric fields use native defaults; numeric tags retain vanilla coercions,
including floor-to-int for floats/doubles, truncation of long integer bits and
no post-load clamping. Writes replace the four hunger fields with native int/
float types, normalize float signed zero and preserve unrelated player fields.

`python3 tools/test_food_storage_parity.py` compares actual `FoodData` persistence
through `TagValueInput`/`TagValueOutput`. The
[food storage report](../benchmarks/rust-food-storage-2026-10-07/scoped-parity.json)
records **45,955 matching lines per JAR**, covering every NBT numeric type,
missing/wrong-type fields, conversion boundaries, nonfinite values and save/load
round trips. Targeted Clippy passes. ProblemReporter diagnostics and the complete
player save lifecycle remain pending; these results establish the four hunger
fields, not a playable save/load implementation.


`living_health` implements health/absorption clamping, healing dispatch, player
hurt and living dead-or-dying predicates. Health writes capture synchronized
storage before attribute lookup. `SyncedFloat` preserves Java boxed-float
equality, writes the value before the update callback, and marks dirty only
after that callback succeeds; it also supports default/dirty extraction for a
single float item.

`python3 tools/test_living_health_parity.py` compares actual LivingEntity/Player
methods and actual `SynchedEntityData` float items. The
[living health report](../benchmarks/rust-living-health-2026-10-07/scoped-parity.json)
records **442,007 matching lines per JAR**, covering nonfinite values, signed
zeros/NaN equality, forced updates, dirty extraction, health-data replacement
during callbacks and partial effects after exceptions. Targeted Clippy passes.
Attribute values and the internal absorption sink are controlled. Player-specific
absorption synchronization, multiple synchronized items, registered attribute
binding, full damage processing and playable entities remain pending.


`attribute_health` binds the health engine to registered entity attributes and
mutable synchronized health. `AttributeHealth` borrows existing state, queries
actual modified MAX_HEALTH/MAX_ABSORPTION values and propagates observer errors
without losing partial health writes. Base living absorption uses field storage.

`python3 tools/test_attribute_health_parity.py` composes actual registered
`AttributeMap`, LivingEntity/Player health methods, `SynchedEntityData` and
`FoodData`. The [composition report](../benchmarks/rust-attribute-health-2026-10-07/scoped-parity.json)
records **666,082 matching lines per JAR**, including all **93 entity suppliers**,
attribute base/modifier changes, real regeneration health writes, 100-tick hunger
sequences and failed synchronization callbacks. Targeted Clippy passes. The
internal absorption sink and starvation damage delivery are controlled;
player absorption synchronization, attribute-change refresh, full damage and
live gameplay integration remain pending.


Entity attribute storage now shares immutable supplier bases and allocates each
mutable instance only on first access. This retains dense lookup indices while
avoiding space for unused instance objects in every entity.

`python3 tools/measure_attribute_memory.py --phase before|after` measures a Linux
release-process workload of 20,000 held-live zombie attribute maps, using five
fresh processes per case. Median process RSS changed from **175,820 to 12,848
KiB** with defaults only, and **175,800 to 29,076 KiB** with four active attributes
per entity. Both cases retain the same health-read checksum. The
[before](../benchmarks/rust-attribute-memory-2026-10-07/before.json) and
[after](../benchmarks/rust-attribute-memory-2026-10-07/after.json) reports record
samples and source hashes; the baseline entity implementation is `fd185a6`.
RSS includes runtime, catalog and allocator overhead. This is not whole-game
memory, a timing benchmark or evidence of an FPS improvement.

The optimized implementation passes both pinned JAR comparisons again:
[entity attributes](../benchmarks/rust-attribute-memory-2026-10-07/entity-regression.json)
(**1,771,555 lines per JAR**) and
[health/hunger composition](../benchmarks/rust-attribute-memory-2026-10-07/health-regression.json)
(**666,082 lines per JAR**). Targeted Clippy passes.


`combat_rules` implements armor/toughness and magic-protection damage arithmetic.
It preserves the nonnull-weapon/server-level guards and clamps modified armor
effectiveness only after the server enchantment hook.

`python3 tools/test_combat_rules_parity.py` compares actual `CombatRules` methods.
The [combat rules report](../benchmarks/rust-combat-rules-2026-10-07/scoped-parity.json)
records **184,248 matching lines per JAR**, including armor/toughness boundaries,
random float bit patterns, nonfinite/signed-zero values, query mutations/failures,
null/empty/unenchanted weapons and server/nonserver branches. Native unenchanted
stacks have explicitly empty components. Targeted Clippy passes. Enchanted item
components, damage tags, armor durability and the complete damage pipeline remain
pending; the exposed enchantment hook is not an enchantment implementation.


`living_damage` implements the inner `actuallyHurt` sequence for base living
entities and players. It preserves invulnerability/armor/magic dispatch, both
absorption reads before the first write, guarded absorbed statistics, Java float
rounding, player exhaustion/taken statistics, base-living second absorption
writes, and combat/health/game-event ordering. Callback failures retain preceding
writes and effects.

`python3 tools/test_living_damage_parity.py` compares actual LivingEntity/Player
methods with real final absorption clamping and controlled phase/storage sinks.
The [living damage report](../benchmarks/rust-living-damage-2026-10-07/scoped-parity.json)
records **161,198 matching lines per JAR**, including entity/player paths, attacker
types, stat limits/rounding, nonfinite values, mutations and failures at query
boundaries. Targeted Clippy passes. Armor/magic results, health storage, combat/
stat sinks and exhaustion delivery are controlled. Outer hurt gates, resistance/
equipment binding, combat tracking and playable damage integration remain pending.


`player_food` implements player exhaustion, eating eligibility and the hunger
threshold for exhaustive manoeuvres. Invulnerability reads the abilities field
directly. Exhaustion additionally checks the client/server side before updating
direct food storage; manoeuvre eligibility uses virtual food and ability lookups.
`StoredPlayerFood` binds these operations to ordinary mutable `FoodData` state.

`python3 tools/test_player_food_parity.py` compares actual Player methods and
native FoodData. The [player food report](../benchmarks/rust-player-food-2026-10-07/scoped-parity.json)
records **32,723 matching lines per JAR**, covering direct/virtual distinctions,
captured food identity during mutations, guard short-circuiting, injected failures,
nonfinite exhaustion and integer hunger boundaries. The stored-state binding is
also compared. Targeted Clippy passes. Consumable components, full ability storage
and live player tick integration remain pending.


`damage_reduction` composes native armor/toughness arithmetic with living damage
bypass checks and armor-durability dispatch. Its magic-reduction path preserves
resistance amplifier overflow, reduced-damage clamping, resisted statistics,
repeated attacker lookup and the server-only enchantment protection query.
Armor attributes floor to Java's saturated integer range before float conversion.

`python3 tools/test_damage_reduction_parity.py` compares actual living methods
for base entities and ServerPlayer receivers. The
[damage reduction report](../benchmarks/rust-damage-reduction-2026-10-07/scoped-parity.json)
records **78,069 matching lines per JAR**, covering bypass combinations,
resistance/stat boundaries, numeric edges, query failures/mutations and actual
empty-equipment protection iteration. Targeted Clippy passes. Equipment is empty;
enchanted components, actual armor durability, registered damage/effect bindings
and the full hurt sequence remain pending.


`synced_floats` stores multiple synchronized float fields. It preserves boxed
float equality, per-field/global dirty state, ascending-ID dirty/default lists,
received assignment without newly marking dirty, per-entry and batch callbacks,
and prefix effects when an observer or field lookup fails. Callbacks may reenter
the table to set/assign fields or drain existing dirty values.

`python3 tools/test_synced_floats_parity.py` compares actual `SynchedEntityData`.
The [synchronized float report](../benchmarks/rust-synced-floats-2026-10-07/scoped-parity.json)
records **554,909 matching lines per JAR**, including forced/equal updates,
received duplicates, reentrant operations, missing IDs, callback failures and
NaN/signed-zero equality. Native gaps are untouched default float items; observed
keys are explicit configured IDs. Targeted Clippy passes. Other serializers,
serializer-mismatch errors, wire codecs and player absorption binding remain
pending.


`player_health` binds ordinary registered player attributes to health and
absorption in one synchronized float table. The player absorption getter and
internal setter use virtual data access; health retains its direct data access.
The final absorption setter still applies the registered maximum before writing.

`python3 tools/test_player_health_parity.py` compares actual Player methods,
registered player attributes and `SynchedEntityData`, exporting the actual pinned
health/absorption field IDs. The
[player health report](../benchmarks/rust-player-health-2026-10-07/scoped-parity.json)
records **243,146 matching lines per JAR**, covering modified limits, healing,
absorption reads/writes, combined dirty/default lists, received assignments,
callback packet drains and failures. Targeted Clippy passes. Observer effects
are controlled; custom virtual data replacement, other synchronized serializers,
wire codecs and full player damage/tick integration remain pending.


`server_player_damage` composes ordinary server-player inner damage with
registered armor/toughness and health/absorption limits, shared synchronized
float storage and stored hunger. Armor break callbacks may change attributes
before the hit queries armor; received damage uses the actual reduction and
health modules, and player exhaustion retains its ability/client guards.

`python3 tools/test_server_player_damage_parity.py` compares the native Player
inner damage method on ServerPlayer receivers, using actual native reduction,
registered modified attributes, synchronized data and FoodData. The
[server-player damage report](../benchmarks/rust-server-player-damage-2026-10-07/scoped-parity.json)
records **373,455 matching lines per JAR**, covering repeated hits, bypass flags,
resistance, numeric edges, armor attribute mutation, dirty packet drains and
callback failures. Targeted Clippy passes. Empty equipment and controlled
source/effect, durability, combat/stat/event services bound this scope; outer
hurt gates, enchanted equipment and the complete player lifecycle remain pending.


`living_hurt` implements the outer living hurt sequence: early rejection,
blocking/freezing/helmet dispatch, non-finite damage sanitization, repeated-hit
cooldowns, hurt-animation writes, attribution, impact/knockback, death/sound
branches, last-source timestamps, effect notifications and criterion dispatch.
Services receive live hurt state, including the distinct field writes before and
after the inner hit in fresh-hit and cooldown-difference branches.

`python3 tools/test_living_hurt_parity.py` invokes the native LivingEntity method
directly on base and ServerPlayer receivers. The
[living hurt report](../benchmarks/rust-living-hurt-2026-10-07/scoped-parity.json)
records **441,413 matching lines per JAR**, including numeric edges, repeated
hits, callback mutations/failures, empty-hand death-protection lookup, native
soundless blocking components and victim criteria with no listeners. Targeted
Clippy passes. Inner damage, blocking amounts, sounds, knockback, death and
attribution/effect services are controlled. Successful totems, attacker-player
criteria, actual blocking reduction, full service bindings and player-specific
outer gates remain pending.


`player_hurt` adds the player ability/invulnerability gates, no-action reset,
shoulder removal, damage-source difficulty scaling and exact-zero rejection
before living hurt. The server-player wrapper first checks PvP permission for
player attackers and player owners of arrow attackers. Difficulty is queried
separately for the peaceful, easy and hard branches as in vanilla.

`python3 tools/test_player_hurt_parity.py` compares actual Player and ServerPlayer
methods through the native living hurt sequence. The
[player hurt report](../benchmarks/rust-player-hurt-2026-10-07/scoped-parity.json)
records **410,880 matching lines per JAR**, including all three-query difficulty
combinations, player/arrow-owner PvP gates, repeated hits and callback failures.
Targeted Clippy passes. Native downstream living services retain the controlled
fixture scope above; invulnerability internals, rule/source registries, actual
team permissions, successful totems, attacker server-player criteria and complete
player lifecycle integration remain pending.


`damage_immunity` implements entity permanent/temporary invulnerability and base
source immunity, living enchantment dispatch, ordered player damage gamerules,
and server-player dimension-change/client-loading checks. It preserves early
returns and repeated source-tag queries across the inheritance layers.

`python3 tools/test_damage_immunity_parity.py` compares each native layer on
ServerPlayer receivers, retaining native empty-equipment enchantment iteration.
The [damage immunity report](../benchmarks/rust-damage-immunity-2026-10-07/scoped-parity.json)
records **170,314 matching lines per JAR**, including mixed damage tags, rule
precedence, virtual flag mutations and callback failures. Targeted Clippy passes.
Source/rule queries and virtual flags are controlled; registered source/rule
bindings, nonempty enchantment immunity and full pipeline integration remain
pending.


`server_player_combat` binds server/player/living outer hurt to the checked
registered-attribute, synchronized-health/absorption and hunger inner damage
path. Dead-or-dying reads live health and the direct death flag; the outer
cooldown/animation state is shared across hits. PvP, blocking, effects,
attribution, knockback, death and other world services remain explicit.

`python3 tools/test_server_player_combat_parity.py` compares the actual native
ServerPlayer hurt chain through actual inner reduction and player storage. The
[server-player combat report](../benchmarks/rust-server-player-combat-2026-10-07/scoped-parity.json)
records **561,528 matching lines per JAR**, including repeated hits, death
rejection, modified attributes, cooldown differences, packet drains, numeric
edges and failures after partial writes. Some fixtures explicitly reset cooldown
between hits; they do not claim entity-tick coverage. Targeted Clippy passes.
Invulnerability answers, blocking amounts, armor-durability callbacks, effects,
PvP permission, combat/event/knockback/death services and criterion listeners are
controlled. Enchanted items, registered sources, actual lifecycle services and
playable integration remain pending.


The stored-player combat binding now executes the verified server-player
invulnerability predicates at every outer and inner query. Its inner damage body
is shared with standalone living/player damage after the required predicate has
run. Native integration retains inherited Entity/LivingEntity/Player/ServerPlayer
immunity methods, including empty-equipment enchantment iteration, gamerule
precedence, dimension changes and client-loading guards.

The [combat immunity integration report](../benchmarks/rust-server-player-combat-immunity-2026-10-07/scoped-parity.json)
records **1,176,994 matching lines per JAR**. The standalone inner-damage
[regression report](../benchmarks/rust-server-player-combat-immunity-2026-10-07/inner-damage-regression.json)
retains **161,198 matching lines per JAR** after extracting the shared body.
Targeted Clippy passes. Virtual flags, source tags/rule values and the other
world services listed above remain controlled; the fixture now executes the
immunity decisions themselves instead of supplying their final result.


`damage_source` imports version-checked native decoded damage definitions and
resolves their raw tag resources with the shared tag implementation. Sources
retain distinct direct/causing entities and optional explicit positions. Weapon
and fallback position queries use the direct entity; difficulty scaling uses
the stored causing entity, while creative-source detection dispatches virtual
causing-entity access before reading player abilities.

`python3 tools/test_damage_source_parity.py` covers all **51 bundled damage
types and 36 damage-type tags**, using the actual native DamageType codec and
TagLoader. The [damage-source report](../benchmarks/rust-damage-source-2026-10-07/scoped-parity.json)
records **152,175 matching lines per JAR**, including constructor identities,
scaling, exhaustion, tag/type membership, position/weapon queries, virtual
causing-entity replacement, mutations and failures. Targeted Clippy passes.
Catalogs are immutable snapshots with local indices, not network IDs. Rust
DamageType codecs, runtime reload, death-message generation, full entity query
bindings and combat source integration remain pending.


Registered damage sources now drive a composed player-combat fixture. Source
helpers map the immunity, outer-hurt and reduction tag enums to the imported
vanilla tags. Native source methods supply exhaustion, difficulty scaling,
creative-source abilities and direct-entity weapon selection through the real
ServerPlayer hurt chain; thorns also follows its native secondary-sound branch.

`python3 tools/test_registered_player_combat_parity.py` records **1,431,711
matching lines per JAR** in the
[registered player-combat report](../benchmarks/rust-registered-player-combat-2026-10-07/scoped-parity.json),
covering all 51 damage types and 36 tags, direct/causing entity combinations,
repeated hits, health/absorption/hunger, cooldown state and callback failures.
Targeted Clippy passes. Equipment is unenchanted; virtual entity flags/rule
values, blocking amounts, durability, PvP permission, attribution, effects,
knockback/death services and empty criterion listeners remain controlled.
Complete entity lifecycle and playable game integration remain pending.


`blocking_rules` implements damage-reduction angle/type conditions, ordered
reduction sums and clamping, block delay/disable tick rounding and item-damage
threshold/floor arithmetic. Source type-set membership is queried only when its
entry's angle condition reaches that check.

`python3 tools/test_blocking_rules_parity.py` compares native BlocksAttacks
records and direct holder-set membership. The
[blocking rules report](../benchmarks/rust-blocking-rules-2026-10-07/scoped-parity.json)
records **105,068 matching lines per JAR**, including exact angle boundaries,
NaN/infinity/signed zero, randomized float bits, source-type mutations and query
failures. Targeted Clippy passes. Angles are supplied in radians; direction/acos
calculation, component codecs, item durability, sounds and full blocking
integration remain pending.


`view_direction` implements native float-table pitch/yaw vectors and horizontal
blocking angles, using the pinned pure-Rust `libm` 0.2.16 inverse cosine. A local
203,072-input probe found 7,908 differences with the platform `acos`, versus none
with this pinned implementation, so platform math is not used for this path.

`python3 tools/test_view_direction_parity.py` compares actual native view vectors
and Java Math.acos, plus the native view/subtract/normalize/dot/acos composition
used by blocking. The [view-direction report](../benchmarks/rust-view-direction-2026-10-07/scoped-parity.json)
records **226,459 matching lines per JAR**, including random doubles, inverse-
cosine boundaries, arbitrary float rotations, degenerate horizontal vectors and
the widened float-PI fallback. Targeted Clippy passes. Full item-blocking dispatch
and validation against JVMs on other platforms remain pending. This does not
resolve the separately tracked Math.cos/Gaussian portability differences.


`item_blocking` implements living blocking dispatch: item/component capture,
bypass and pierced-arrow rejection, source direction, component reduction,
blocking-item damage dispatch and the later non-projectile melee response.
The virtual level/used hand are queried after reduction; melee response retains
the original server-level argument and performs a fresh direct-entity lookup.

`python3 tools/test_item_blocking_parity.py` compares actual LivingEntity
applyItemBlocking with native components on base and ServerPlayer receivers.
The [item-blocking report](../benchmarks/rust-item-blocking-2026-10-07/scoped-parity.json)
records **87,930 matching lines per JAR**, including callback mutations/failures,
captured items/components, piercing/bypass combinations, native direction/math,
client/stat guards and melee responses. Targeted Clippy passes. Native stacks
are explicitly undamageable; actual damageable-item mutation, blocking eligibility,
component codecs and complete combat/world integration remain pending.


`item_use` implements blocking-item eligibility and both elapsed-use timer
overloads. It retains the original blocking delay while rereading active-item
storage and remaining ticks after the virtual duration query; partial-tick
queries preserve the second virtual use-state check.

`python3 tools/test_item_use_parity.py` compares the actual inherited native
methods with real blocking components and controlled virtual item durations.
The [item-use report](../benchmarks/rust-item-use-2026-10-07/scoped-parity.json)
records **108,690 matching lines per JAR**, covering wrapping tick arithmetic,
delay boundaries, absent components, empty stacks, shared item types, partial
float values and callback mutations/failures. Targeted Clippy passes. Full
item-use lifecycle, damageable-item storage and complete combat/world integration
remain pending.


`item_durability` stores the durability projection of an item stack, including
raw versus visible counts/components, clamped damage access, damageability/break
predicates and wrapping count changes. Server-level damage preserves creative
immunity, the positive float enchantment pipeline, advancement-before-write and
copy-before-shrink ordering. The non-breaking variant retains its native empty
break consumer, including unusual invalid component values.

`python3 tools/test_item_durability_parity.py` compares actual unenchanted native
ItemStack operations. The [durability report](../benchmarks/rust-item-durability-2026-10-07/scoped-parity.json)
records **81,762 matching lines per JAR**, including absent components, empty/air
stacks, unbreakable flags, integer boundaries, callback mutations and failures.
Targeted Clippy passes. This is a durability projection, not complete item-stack
component storage. Enchantment effects, component codecs, equipment break
handling and full gameplay integration remain pending.


`blocking_item_damage` composes blocking-component item-damage arithmetic with
stored durability. It preserves player-only handling, the passed level's client
guard for item-use statistics, the later virtual entity-level check and the
server-player-only creative/advancement queries. Break callbacks receive the
pre-shrink stack copy and the caller's original equipment slot.

`python3 tools/test_blocking_item_damage_parity.py` compares actual native
BlocksAttacks calls into ItemStack storage on non-player, Player and ServerPlayer
receivers. The [blocking durability report](../benchmarks/rust-blocking-item-damage-2026-10-07/scoped-parity.json)
records **71,800 matching lines per JAR**, including scalar float boundaries,
invalid/absent durability components, statistic item capture, callback mutations
and failures. Targeted Clippy passes. Stacks are unenchanted; full component
storage, equipment lifecycle and complete combat/world integration remain pending.


`item_use_lifecycle` implements living use ticks, per-item updates, completion,
release and stop ordering. Stable stack handles preserve type versus identity
checks, rereads after virtual hand queries, captured level/item arguments,
wrapping countdowns, returned-item placement and failure-visible state changes.

`python3 tools/test_item_use_lifecycle_parity.py` compares actual inherited native
methods and ItemStack dispatch with controlled custom item handlers. The
[lifecycle report](../benchmarks/rust-item-use-lifecycle-2026-10-07/scoped-parity.json)
records **144,736 matching lines per JAR**, including empty stacks, changed
held/active items, shared item types, client/server queries, finish vibrations,
kinetic-map presence and callback mutation/failure ordering. Targeted Clippy
passes. Starting use, actual synchronized flags, consumable/kinetic effects,
remainder/cooldown components and full inventory/game-tick integration remain
pending.


`item_use_start` starts use from a captured held stack, writes the returned
duration after its callback, synchronizes equipment before changing using/offhand
flags, emits the component's start vibration and resets recent kinetic enemies
when the final active stack has a kinetic-weapon component.

`python3 tools/test_item_use_start_parity.py` compares inherited native
startUsingItem and its private equipment packet construction. The
[start-use report](../benchmarks/rust-item-use-start-2026-10-07/scoped-parity.json)
records **84,873 matching lines per JAR**, including captured stack/level/hand
arguments, empty stacks, callback mutation/failure ordering, flag writes, native
UseEffects/KineticWeapon presence and clearing a populated kinetic-enemy map.
Targeted Clippy passes; all 20 existing horizon-world library tests pass with
these changes. Synchronized flag storage, packet encoding/delivery, kinetic
combat and complete inventory/game-tick integration remain pending.


`item_use_catalog` imports all 1,658 pinned item identities and a typed projection
of their default components for stack size, durability and use-duration/release
queries. Component-driven use preserves consumable precedence over blocking and
kinetic weapons. Registered duration overrides remain separate; consumables
truncate float ticks while instruments floor them. Empty stacks query air
behavior and hide their stored components.

`python3 tools/test_item_use_catalog_parity.py` binds vanilla's actual component
initializers and confirms both JARs export equivalent defaults. The
[registered item-use report](../benchmarks/rust-item-use-catalog-2026-10-07/scoped-parity.json)
records **246,715 matching lines per JAR**, covering every item default, count
changes and component mutation sequences, override behavior, stackability,
durability and integer/float extremes. Targeted Clippy passes. The imported
projection does not implement all item components or their codecs; full inventory,
enchantment effects and stored gameplay integration remain pending.


`component_patch` stores shared immutable prototypes and ordered copy-on-write
patches. Set/remove return values, redundant-value sanitation, explicit removals,
unsanitized restoration, independent copies and saved patch snapshots retain
native behavior. Entry iteration emits patched values before untouched defaults;
key-set order instead applies the patch to the prototype's key sequence.

`python3 tools/test_component_patch_parity.py` compares actual native patched
maps. The [component-patch report](../benchmarks/rust-component-patch-2026-10-07/scoped-parity.json)
records **622,680 matching lines per JAR**, including integer/boxed-float equality,
NaN/signed zero, copy isolation and mutation-before-error when a restored removal
has no prototype default. Targeted Clippy passes. Prototypes have at most seven
keys in this suite; callers supply prototype order and native value equality.
Native large-prototype identity-hash ordering, codecs, mutable payload aliases
and full item/inventory integration remain pending.


`component_durability` binds damage/count queries and mutations directly to
shared component prototypes and patches. The durability algorithm now accepts
`DurabilityStorage`, so writes and storage errors occur at their native points,
including before break callbacks. The earlier plain durability projection
remains supported through the same interface.

`python3 tools/test_component_durability_parity.py` compares native ItemStack
operations with nonempty prototypes and observable patches at every callback.
The [component durability report](../benchmarks/rust-component-durability-2026-10-07/scoped-parity.json)
records **83,727 matching lines per JAR**, including original/copy independence
and mutation-before-error for unsanitized restored removals. Targeted Clippy
passes. The refactor also preserves both native regression suites:
[scalar durability](../benchmarks/rust-component-durability-2026-10-07/scalar-regression.json)
(**81,762 lines per JAR**) and
[blocking durability](../benchmarks/rust-component-durability-2026-10-07/blocking-regression.json)
(**71,800 lines per JAR**). Full component codecs, enchanted durability, equipment
and complete gameplay integration remain pending.


`item_stack_storage` implements stored item identity, count/pop-time storage,
component visibility, copying, splitting, transmutation and consumption. Copies
share component data until a write; canonical empty results are explicit so a
containing item store can preserve reference identity. Negative counts and
wrapping arithmetic follow the pinned methods, including mutations to an empty
stack and consumption callbacks that change or fail after a copy is captured.

`python3 tools/test_item_stack_storage_parity.py` compares 7,856 fixtures with
actual native ItemStack methods and small bound integer-component prototypes.
The [storage report](../benchmarks/rust-item-stack-storage-2026-10-07/scoped-parity.json)
records **568,580 matching lines per JAR**. Comparisons include raw and visible
state, component patch order, stack identity and item/component/count equality.
Targeted Clippy passes. The fixture arena preserves canonical EMPTY aliases;
a production inventory, full component values/codecs and complete item-use,
equipment and game-tick integration remain pending.


`item_stack_refs` adds shared stack identity with automatic reference-counted
lifetime management on the simulation thread. `simple_container` uses these
references for get/set, merging, removals, reverse removal by type, clearing,
capacity checks and item counts. Copies retain independent component patches;
container slots and callers can deliberately share the same stack.

`python3 tools/test_simple_container_parity.py` compares actual native container
and stack methods, including changes and failures in virtual size/change hooks.
The [container report](../benchmarks/rust-simple-container-2026-10-07/scoped-parity.json)
records **590,402 matching lines per JAR**. It checks alias identity, raw/visible
counts, empty-stack references, component-sensitive merging, overflow, invalid
indices and mutation prefixes on failure. In particular, native insertion into
the first empty slot clears the remainder before limiting the inserted copy.
Targeted Clippy passes. Player inventory, menus, crafting accounting, persistence,
arbitrary getItem/setItem overrides and complete gameplay integration remain
pending. Reference counting here has not been benchmarked as a performance gain.


`player_inventory` implements the 36 ordinary slots, seven mapped equipment
slots, shared entity equipment, selected hotbar slot, picking, matching/crafting
eligibility, removals, replacement, clearing and the change counter. Equipment
retains populated entries when cleared. Stack and equipment references can be
shared across inventories; replacing an inventory retains stack aliases.

`python3 tools/test_player_inventory_parity.py` compares actual native Inventory,
EntityEquipment and ItemStack methods. The
[inventory report](../benchmarks/rust-player-inventory-2026-10-07/scoped-parity.json)
records **790,440 matching lines per JAR**, including invalid-index failure
prefixes, selected-slot changes, negative equipment removal amounts, ordinary
versus equipment identity, self-replacement and shared equipment clearing.
Components cover damage, unbreakable presence, a fixed enchantment and literal
custom names. Native Map.of iteration order varies by JVM; fixtures explicitly
supply equipment-map order to both sides when removing an aliased reference.
Targeted Clippy passes. Pickup, inventory/equipment ticks, menus, recipe
accounting, persistence, full component values and complete gameplay integration
remain pending.


`inventory_pickup` composes player slot search and insertion with shared stack
storage. Search checks the selected slot, offhand, then ordinary slots. Insertion
consumes its source, preserves component-sensitive merging and pop time, and
handles damaged stacks and creative fallback at their native callback points.

`python3 tools/test_inventory_pickup_parity.py` compares actual native
Inventory.add/getSlotWithRemainingSpace, ItemStack operations and shared
equipment. The [pickup report](../benchmarks/rust-inventory-pickup-2026-10-07/scoped-parity.json)
records **424,282 matching lines per JAR**, including source/destination aliases,
callback mutations, failures, count overflow, invalid slots and the damaged
copy-and-clear that happens before slot validation. Targeted Clippy passes.
Native crash-report wrapping is represented as an error; diagnostic text is not
compared. Item-entity pickup events, networking, inventory ticks, menus, full
components and complete gameplay integration remain pending.


`inventory_tick` composes inventory/equipment iteration with ItemStack's pop-time
update and ServerLevel-only item dispatch. It retains stack arguments across
level callbacks, reads hotbar selection at the native point and observes changes
to later equipment entries during the same tick. Aliased stacks can be visited
more than once, with each visit updating their shared pop-time counter.

`python3 tools/test_inventory_tick_parity.py` compares actual Inventory.tick,
EntityEquipment.tick and ItemStack.inventoryTick with custom item handlers. The
[tick report](../benchmarks/rust-inventory-tick-2026-10-07/scoped-parity.json)
records **195,957 matching lines per JAR**, including empty stacks, callbacks
that mutate later slots, weakly consistent equipment iteration and failure
prefixes. The client comparison uses an actual allocated ClientLevel for the
non-server branch; the dedicated JAR uses null because ClientLevel is absent.
Targeted Clippy passes. Registered items' individual tick effects, full entity
and world ticking and complete gameplay integration remain pending.


`player_equipment` binds the main hand to the selected inventory slot and other
equipment to the shared entity map. Hand/slot writes capture the previous item,
store the new reference and then invoke the equip callback. Clearing the base
equipment map does not clear the selected stack; clearing the inventory does.

`python3 tools/test_player_equipment_parity.py` compares actual PlayerEquipment
and inherited LivingEntity hand/slot access over a real native Inventory. The
[equipment report](../benchmarks/rust-player-equipment-2026-10-07/scoped-parity.json)
records **408,852 matching lines per JAR**, including selection changes, callback
mutations/failures, old/new aliases, raw enum-map main-hand entries and the
selected main-hand override. Targeted Clippy passes. Full onEquipItem effects,
synchronized flags, active-use integration and complete gameplay integration
remain pending.


The item-use interfaces now accept cloned shared references as well as small
copyable handles. `StackRef` equality compares object identity; component/count
value comparisons remain explicit. Captured references stay alive through
callbacks that replace the active or held stack. The lifecycle fixture now uses
the production reference-counted stack implementation.

The native regressions retain **144,736 lifecycle**, **84,873 start-use** and
**108,690 eligibility/timing** matching lines per JAR. Reports:
[lifecycle](../benchmarks/rust-shared-item-use-2026-10-07/item-use-lifecycle.json),
[start](../benchmarks/rust-shared-item-use-2026-10-07/item-use-start.json),
[eligibility](../benchmarks/rust-shared-item-use-2026-10-07/item-use.json).
Targeted Clippy and all 20 horizon-world library tests pass. Hand queries and
flags remain controlled in these fixtures; composing them with actual player
inventory, synchronized flags and full item effects remains pending.


`synced_values` shares the synchronized scalar algorithm between bytes and
floats, retaining Java boxed-float equality. `living_use_flags` binds the pinned
byte field to using-item/offhand queries and signed-mask flag writes. Flag writes
use the same write-before-callback, callback-before-dirty ordering as other
synchronized values.

`python3 tools/test_synced_bytes_parity.py` compares actual byte SynchedEntityData
and inherited LivingEntity flag methods. The
[byte/flag report](../benchmarks/rust-synced-bytes-2026-10-07/scoped-parity.json)
records **572,100 matching lines per JAR**, covering all initial bytes, signed
masks, forced updates, assignments, reentrant callbacks, dirty packing and
failures. The [float regression](../benchmarks/rust-synced-bytes-2026-10-07/float-regression.json)
retains **554,909 matching lines per JAR**. Targeted Clippy passes. Mixed-serializer
storage, entity schemas, wire codecs and full item-use/gameplay integration remain
pending.


`inventory_use` binds living item-use ticks, completion, release and stop to
actual player inventory/hands, shared stack references and synchronized byte
flags. Equipment storage changes precede equip callbacks; synchronized flag
writes retain their value/callback/dirty ordering. Effects receive the same
mutable inventory, flag storage and active-use state.

`python3 tools/test_inventory_use_parity.py` compares inherited native lifecycle
methods composed with real Inventory, PlayerEquipment and SynchedEntityData. The
[combined report](../benchmarks/rust-inventory-use-2026-10-07/scoped-parity.json)
records **349,310 matching lines per JAR**, including live count mutations,
selection/offhand changes, equipment callbacks, flag dirty prefixes, completion
replacement and failed callbacks. Targeted Clippy passes. Item/level effects and
the equip/synchronized observers are controlled; starting use, default observer
reactions, consumables, kinetic combat, remainders/cooldowns and complete gameplay
integration remain pending.


`inventory_use::flags_updated` implements LivingEntity's synchronized use-flag
reaction. On clients, a using flag can select the held stack and query its
duration; clearing use can clear the active stack and remaining time. Server
notifications leave active-use state unchanged. Received assignments notify even
when the byte value is unchanged, while ordinary unchanged writes do not.

`python3 tools/test_use_flag_reaction_parity.py` invokes the inherited native
observer through actual synchronized writes/assignments, player inventory and
equipment. The [reaction report](../benchmarks/rust-use-flag-reaction-2026-10-07/scoped-parity.json)
records **366,331 matching lines per JAR**, including duration callbacks,
empty-but-distinct stack references, signed masks, repeated received values,
selection/count mutations and failures before dirty marking. Ordinary lifecycle
operations are also exercised with the real observer reaction. Targeted Clippy
passes. Other synchronized keys, start-use packet flow, full item/equip effects
and complete gameplay integration remain pending.


`inventory_use` now binds start-use to stored inventory, shared active stacks
and synchronized flag reactions. Server equipment synchronization precedes flag
writes, followed by start vibration and the kinetic-enemy reset service. Captured
stack/hand arguments and live state reads retain native callback ordering.

`python3 tools/test_inventory_use_start_parity.py` exercises inherited native
start-use with actual Inventory, PlayerEquipment and synchronized storage. The
[start report](../benchmarks/rust-inventory-use-start-2026-10-07/scoped-parity.json)
records **9,506 fixtures and 223,004 matching lines per JAR**, including native
equipment packet construction, observer reactions, callback mutations/failures
and clearing a populated kinetic-enemy map. Targeted Clippy passes. Packet
encoding/delivery, the full kinetic-enemy map and combat, complete item/equip
effects and full gameplay integration remain pending.


`item_template` implements immutable template construction, conversion from
stacks, count replacement with native reference identity, current-prototype
component lookup, creation and patch precedence. Strict validation checks
damageable/stackable incompatibility before contained components and top-level
size. A required component-schema service handles contained validation. Rejected
creations return canonical EMPTY; rejected applied patches restore the exact
previous raw patch, including redundant entries.

`python3 tools/test_item_template_parity.py` compares actual ItemStackTemplate
and ItemStack validation with scalar max-stack-size/max-damage/damage components.
The [template report](../benchmarks/rust-item-template-2026-10-07/scoped-parity.json)
records **518,556 matching lines per JAR**, covering zero/negative/overflow-edge
counts, changed prototypes, component removals, aliasing and validation rollback.
Targeted Clippy passes. Contained components and bundle-weight validation are
not exercised by this corpus; their schema implementation, codecs, use-remainder
integration and complete gameplay integration remain pending.


`use_remainder` converts a consumed stack using the validated item-template
factory. Infinite materials and unchanged visible counts retain the original
reference. An exhausted stack returns the created remainder; otherwise an extra
item callback runs and the captured original reference is returned. Callback
mutations and failures retain their native prefix.

`python3 tools/test_use_remainder_parity.py` compares actual UseRemainder with
native template creation. The
[remainder report](../benchmarks/rust-use-remainder-2026-10-07/scoped-parity.json)
records **353,264 matching lines per JAR**, including canonical EMPTY creation,
empty-but-distinct inputs, reference replacement and callback failures. Targeted
Clippy passes. The concrete actor inventory/drop handler, contained-component
validation, finish-use side effects and complete gameplay integration remain
pending.


`bundle_weight` implements checked native fraction construction, addition and
multiplication, nested-bundle overhead, occupied-beehive weight and lazy caching
of successful and failed weight calculations. Contained validation checks
container sizes, then bundle sizes and weight, then charged projectile sizes.
It does not recursively apply full strict validation to each contained item.

`python3 tools/test_bundle_weight_parity.py` compares pinned Fraction arithmetic,
actual BundleContents and ItemStack strict validation. The
[bundle report](../benchmarks/rust-bundle-weight-2026-10-07/scoped-parity.json)
records **137,332 matching lines per JAR**, including overflow, negative template
counts, changed prototypes, cached errors, nested bundles, bee presence, empty
component visibility and validation order. Targeted Clippy passes. Typed schema
adapters remain fixture-local; complete bundle inventory operations, codecs,
item-use integration and full gameplay integration remain pending.


`java_hash_map` supplies default-capacity storage for non-null keys with native
hash codes and a total comparable ordering consistent with equality. It retains
linked bucket traversal, tree-bin root movement, red/black insertion/deletion,
resize splits, iterator removal and structural-modification checks. Tree lookup
uses the ordered tree; removed arena slots are reused.

`python3 tools/test_java_hash_map_parity.py` compares this API with the actual
JDK 25 HashMap used by vanilla cooldown storage. The
[map report](../benchmarks/rust-java-hash-map-2026-10-07/scoped-parity.json) records
**601,084 matching lines** for each pinned-game classpath run, including all-key
collisions, tree/list transitions, varied removal orders, iterator mutations
and clear-on-empty behavior. Both runs use the same installed JDK; this is a
collection comparison, not a gameplay parity claim. Targeted Clippy passes.
Null/non-comparable keys, alternate map constructors and unexposed Map methods
are outside this storage API. Concrete cooldown and gameplay integration remain
pending.


`item_cooldowns` composes cooldown entries with native hash-map ordering. Adds
store before start notifications; removals notify even for missing groups. Tick
expiry removes before end notifications, rereads the live clock and preserves
iterator invalidation after callback mutations. Percentages use native wrapping
integer differences and float arithmetic. `identifier` supplies validated names,
Java hashes and path-first ordering. UseCooldown converts seconds and retrieves
the player cooldown manager before adding through the stack's current group.

`python3 tools/test_item_cooldowns_parity.py` compares actual ItemCooldowns and
UseCooldown over real stack components, with controlled player/start/end
callbacks. The
[cooldown report](../benchmarks/rust-item-cooldowns-2026-10-07/scoped-parity.json)
records **167,906 matching lines per JAR**, including colliding identifiers,
wrapping clocks, zero/negative durations, float edge cases, empty visibility,
reentrant ticks and mutation/failure prefixes. Targeted Clippy passes. Complete
item-finish integration, server cooldown packets and full gameplay remain
pending.


`item_after_use` composes shared stack copying, item completion, validated
remainder creation and cooldown application. The original stack is copied before
the item handler; remainder, cooldown and count are captured from that copy
before actor callbacks. The original copy reaches cooldown grouping even when
the item handler returns another stack. `use_remainder::convert_with` permits
creation and delivery to share the same mutable world without overlapping
borrows. Templates and component patches support structural value equality.

`python3 tools/test_item_after_use_parity.py` invokes actual
ItemStack.finishUsingItem and native remainder/template/cooldown implementations.
The [completion report](../benchmarks/rust-item-after-use-2026-10-07/scoped-parity.json)
records **211,804 matching lines per JAR**, including prototype changes, original
copies, empty aliases and mutations/failures at each effect boundary. The
[remainder regression](../benchmarks/rust-item-after-use-2026-10-07/remainder-regression.json)
retains **353,264 matching lines per JAR**. Targeted Clippy passes. Item handlers
and actor extra-item delivery remain controlled; concrete consumables, full
inventory-use binding, contained schemas and complete gameplay remain pending.


`extra_items::deliver` connects server-player remainder delivery to actual
inventory pickup. Failed insertion reaches the predicted drop service with the
same remaining stack; successful insertion retains native count consumption and
slot aliases. The drop service represents throwRandomly=false and
Prediction::PREDICTED.

`python3 tools/test_extra_items_parity.py` composes native item completion,
remainders and cooldowns with inherited ServerPlayer extra-item handling and
real Inventory/PlayerEquipment. The
[delivery report](../benchmarks/rust-extra-items-2026-10-07/scoped-parity.json)
records **272,124 matching lines per JAR**, including full/partially filled
inventories, offhand merging, aliased slots, creative material queries, partial
insertion and callback failures. Targeted Clippy passes. Drop arguments are
verified, but entity spawning remains controlled. Concrete item handlers, full
contained schemas, complete use lifecycle and full gameplay remain pending.


`food_properties` implements the food consumption listener with stored FoodData
and native random streams. It captures the actor's random source before the
first sound, updates hunger and saturation for players, then emits the burp
sound using the captured source and current position.

`python3 tools/test_food_properties_parity.py` compares actual
FoodProperties.onConsume with real FoodData and both native random generators.
The [food listener report](../benchmarks/rust-food-properties-2026-10-07/scoped-parity.json)
records **70,632 matching lines per JAR**, including player/non-player actors,
sound arguments, subsequent random state, changed food stores and positions,
and callback mutation/failure prefixes. Targeted Clippy passes. Sound playback,
complete consumable dispatch, particles/effects and full gameplay integration
remain pending.


`item_particles` emits eating particles from one retained ItemStackTemplate.
Each iteration reads the actor's direct random field and current rotations;
eye-height and level callbacks preserve native evaluation order. Component
mutations after capture do not alter the particle template.

`python3 tools/test_item_particles_parity.py` compares inherited
LivingEntity.spawnItemParticles with real templates and both native random
generators. The
[particle report](../benchmarks/rust-item-particles-2026-10-07/scoped-parity.json)
records **143,167 matching lines per JAR**, including exact position/velocity
bits, empty stacks, negative particle counts, snapshot identity, float edge
cases, random-source replacement and callback mutation/failure prefixes.
Targeted Clippy passes. The fixture uses scalar damage component patches and
controlled particle delivery; renderer enqueue, full consumable dispatch and
complete gameplay remain pending.


`consumable` supplies consumption timing and particle/sound dispatch. Both
eating and drinking pitch draws occur for every animation before particle
callbacks. Sound overrides run afterward; volume and pitch retain their
previously drawn values. Particle emission may use a different actor random
source from the one supplied for sound selection.

`python3 tools/test_consumable_emission_parity.py` composes actual Consumable
emission with inherited LivingEntity particles and real item templates. The
[consumable emission report](../benchmarks/rust-consumable-emission-2026-10-07/scoped-parity.json)
records **124,732 matching lines per JAR**, covering all animations, timing
overflow and float edges, random-source aliases, optional particles, sound
overrides and callback mutation/failure prefixes. Targeted Clippy passes. Sound
playback, rendering, consumable completion/effects and full gameplay remain
pending.


`consumable` now also runs eligibility, immediate/timed start and completion.
Completion captures its random source before emission, selects the current item
for server statistics, dispatches live component listeners, then runs server
effects, the game event and finite-material stack shrink in native order. The
same input stack is returned even when callbacks empty it. Timed starts return
CONSUME without a transformed stack; immediate starts return that stack.

`python3 tools/test_consumable_parity.py` compares actual Consumable methods
with concrete FoodProperties/FoodData, inherited particles, shared stacks and
both native random generators. The
[consumption report](../benchmarks/rust-consumable-2026-10-07/scoped-parity.json)
records **246,033 matching lines per JAR**, covering player/non-player/server
branches, food component changes, stat item selection, empty advancement trigger
storage, client/server effects, game events, creative material queries and
mutation/failure prefixes. Targeted Clippy passes. Eligibility and start-use
callbacks, statistics, effect bodies and event delivery remain controlled;
listener enumeration here binds the ordinary food schema. Full advancement
evaluation, potion effects, entity/world lifecycle integration and playable
gameplay remain pending.


`status_effect` stores shared MobEffectInstance identity, scalar details, hidden
chains and blend state. Native copies retain scalar details while omitting hidden
effects and resetting blend state. Stronger/longer effect updates preserve
hidden-chain aliases and native change flags. Duration mapping skips zero and
infinite durations; scaling uses the pinned version's floor and narrowing rules.

`python3 tools/test_status_effect_parity.py` compares actual constructors,
copy/update operations, duration mapping/scaling, equality and blend copying.
The [status storage report](../benchmarks/rust-status-effect-2026-10-07/scoped-parity.json)
records **655,643 matching lines per JAR**, covering constructor clamping, raw
amplifiers, shared hidden chains, negative/overflow/infinite durations, float
edges and mapper mutation/failure behavior. Equality excludes hidden and blend
state as in vanilla. Mismatched-holder warning logs are outside the comparison.
Server/client ticking, concrete effect behavior, registries and full entity
integration remain pending.


`consume_effect` implements apply-status, remove-status, clear-status and sound
consumption effects. Applying status effects draws probability even for an empty
list, creates fresh native-style instance copies and visits every entry without
short-circuiting on success. Removal preserves holder order and duplicates;
sound playback captures block position before querying the actor's category.

`python3 tools/test_consume_effect_parity.py` compares the four actual native
consume-effect classes with real effect instances and random generators. The
[consume-effect report](../benchmarks/rust-consume-effect-2026-10-07/scoped-parity.json)
records **184,437 matching lines per JAR**, including NaN/probability edges,
copy identity/details, hidden/blend resets, shared input mutation, return-value
aggregation, sound arguments and callback failure prefixes. Targeted Clippy
passes. Actor status add/remove/clear and sound delivery remain controlled;
named holder-set rebinding, teleport consumption and full gameplay integration
remain pending.


`teleport_consume` implements random-teleport consumption, including the
16-attempt limit, coordinate/random evaluation order, logical-height bounds,
dismounting and post-success effects. Successful teleports emit the game event,
class-specific sound and optional packed directional-particle event before
resetting fall distance and impulse context.

`python3 tools/test_teleport_consume_parity.py` compares actual
TeleportRandomlyConsumeEffect with both native random generators, ordinary and
fox actor classes, native offset packing and fall/impulse reset bodies. The
[teleport consumption report](../benchmarks/rust-teleport-consume-2026-10-07/scoped-parity.json)
records **282,010 matching lines per JAR**, including exact candidates and
subsequent random state, height overflow, float edges, attempt exhaustion,
captured/live positions and callback mutation/failure prefixes. Targeted Clippy
passes. Destination validation and teleporting, passenger links and world event
delivery remain controlled services; full gameplay integration remains pending.


`status_effect` also implements server/client ticking and client blend updates.
Hidden instances count down with their parent. When the active duration reaches
zero, the hidden details are restored before the server downgrade callback.
Infinite effects use the actor's tick count for scheduling. Blend updates retain
native callback/read ordering, duration-dependent steps and interpolation.

`python3 tools/test_status_tick_parity.py` compares actual MobEffectInstance
ticks, skipBlending and getBlendFactor with controlled MobEffect callbacks. The
[status tick report](../benchmarks/rust-status-tick-2026-10-07/scoped-parity.json)
records **412,552 matching lines per JAR**, including shared hidden chains,
zero/negative/infinite durations, raw amplifiers, early application failures,
downgrade notifications, blend float edge cases, removed actors and callback
mutation/failure prefixes. Targeted Clippy passes. Registered effect behaviors,
entity effect maps, UI rendering and full gameplay remain pending.


`java_hash_map` now supports non-comparable keys, unequal keys whose comparable
ordering returns zero, and the native map-copy constructor. Lookup searches
ambiguous tree branches; tree insertion uses class/identity tie-breaking. Copies
size their table from the source count before insertion, retaining shared values
while allowing the resulting bucket order to differ.

`python3 tools/test_java_identity_map_parity.py` compares actual JDK 25 HashMap
operations and copies across mixed key classes. Each Java process exports its
actual identity hashes for Rust to use in that same scenario. The
[identity/copy report](../benchmarks/rust-java-identity-map-2026-10-07/scoped-parity.json)
records **577,200 matching lines per classpath run**. The
[ordered-key regression](../benchmarks/rust-java-identity-map-2026-10-07/ordered-map-regression.json)
retains **601,084 matching lines**, and the
[cooldown regression](../benchmarks/rust-java-identity-map-2026-10-07/cooldown-regression.json)
retains **167,906 matching lines per JAR**. Targeted Clippy passes. Map-only runs
use the same installed JDK; these comparisons do not assert gameplay parity or
equal identity hashes across processes. Keys must have pure operations; null
keys and other unexposed Map APIs remain outside this storage subset.

`PlayerAttributeHealth`, `ServerPlayerDamage` and `ServerPlayerCombat` can now
borrow `LiveAttributes`, the same storage used by the resident moving player,
as well as `EntityAttributes`. Health limits and armor queries use the borrowed
store; equipment damage callbacks can update it before damage reduction reads
armor and toughness. Player absorption retains synchronized float storage.

The [live player combat comparison](../benchmarks/rust-live-player-combat-2026-10-07/scoped-parity.json)
runs the identical corpus through both stores and matches 1,176,994 output lines
against each pinned Java reference. It covers repeated hits, cooldowns, health
and absorption writes, exhaustion, numeric edges, armor mutation during a hit
and callback failures. Equipment, attribution, death services and other world
consumers retain the controlled boundaries documented in the report. Connecting
this binding to the resident client landing consumer remains the next step.

## Related docs

- [World generation](world-generation.md)
- [Terrain filling](terrain-filling.md)
- [Heightmaps](heightmaps.md)
