# Explicitly summoned leash knots

This repair uses the existing real Other identity/collection route for explicitly loaded
`minecraft:leash_knot` entities. A class-specific optional check counter is initialized
only for that class; other constructors, including temporary cloud save projections, keep
`None`. Existing clone, level transfer and removal of the Other record carry that state.
It does not put explicit knots in the natural lead-derived `knot_ids` map.

Loading uses the existing `knot_state(BlockPos::containing(input)).position`: floor each
coordinate, then center X/Z by 0.5 and Y by 0.375. Saving inserts the existing six common
Entity defaults only for knots, alongside the existing Marker case: Motion (Double list),
Air (Short300), OnGround (Byte0), Invulnerable (Byte0), PortalCooldown (Int0) and
fall_distance (Double0). Actual UUID, name, tags and unrelated raw fields remain intact.

The existing running-world attachment phase ticks the class-specific counter only inside
an actual entity-ticking area with the subject chunk loaded. The counter compares the old
value before incrementing: support is first checked on tick101, then reset. Actual support
uses the loaded block name and existing `minecraft:fences` tag, without loading chunks.
An unsupported explicit Other entry is removed; natural knots and lead ownership remain
untouched. Pinned `LeashFenceKnotEntity.dropItem` plays `LEAD_UNTIED` and drops no item, so
removal reuses the existing untied sound. Sound delivery is not a runtime parity claim.

## Authority and proof

Pinned Java26.3 `LeashFenceKnotEntity.recalculateBoundingBox`, its empty additional
load/save methods, `survives`, `dropItem`, and superclass `BlockAttachedEntity.tick` and
`setPos` were inspected through local bytecode/ledger. The existing natural holder
geometry helper already implements the same center. No new attachment engine or codec
schema is introduced.

The frozen pristine base is `fd06d1c2650b52d8d4bb3d808ca73e55971ba717`. Fresh Java admits
a predeclared 629-record fixture: three independent real UUID knots with fractional
positions, valid/absent/removed fence support, plus a real NoAI pig control. Observations
are at exact relative ticks0/1/100/101. The fence is removed after tick1; both unsupported
subjects remain through100 and are absent at101, while the valid fence subject survives.
Direct UUID and typed-selector identities are checked independently. Fresh result/success
stores, exact typed saved fields and callbacks/styles, floor/chunk/support and actual
clock guards remain strict. Full bodies/position records are preserved as diagnostics.

Pristine Game matches 463/629; all166 differences are position normalization(10), missing
six common defaults(140), or survival/cleanup(16). All admission controls pass. Four
registered Game tests fail old on normalization (including negative fractions), saved
projection, tick101 removal and paused/ticket-excluded eligibility. A fifth actual
lead-created-knot preservation control admits its real existing natural adapters, then
fails old only because the explicit unsupported knot remains.

Use the manual existing-route driver from the repository root with fresh output folders:

```sh
python3 tools/summoned_leash_knot_fixture.py --reference-only --work-dir /tmp/knot-java
python3 tools/summoned_leash_knot_fixture.py --binary /absolute/path/to/horizon-client \
  --reference /tmp/knot-java/vanilla.jsonl --work-dir /tmp/knot-native
```

The second command reruns fresh Java and checks reference bytes. The same script and
reference must be used for old and fixed captures. Prepared resources stay local. The
resource-backed `game::persist::summoned_knot_tests` exercise actual registered commands,
live collection/state, no-raw saving and actual Game ticks, rather than an invented
selector or raw-tag echo.

Coverage is explicit summoned knots in the admitted scene. Negative fractional positions,
paused/ticket-excluded turns, unrelated Marker/pig and actual natural lead-created knot
preservation are Game-unit controls. This does not establish natural-knot lifecycle,
delayed-lead issue11408, all attachments, packet delivery, generic Other serialization,
restart or cross-dimension behavior. No pending production was imported.

All five focused Game regressions pass on the final source. Their old runs fail at the
actual missing normalization/removal assertions, including a separately added natural
control whose lead creation, knot ID and holder guards already pass before that failure.
Later main `98b882712308386bbb48acbc60968f7d7a991902` has no changes in the reviewed
knot/Other/attachment source set, so the pair remains on its declared frozen base.

The final unchanged runtime pair passes **629/629**, with byte-identical program and
fresh-reference files. All declared admissions pass. Frozen binary/source hashes, complete
old/fixed bodies/styles/callbacks, clock traces and trace differences are retained under the
local task archive. These unnormalized raw diagnostics are separate from the predeclared
typed-field contract; a 629/629 result is not whole-compound or absolute-clock equality.

The full trace comparison is retained without masks: old17/17 traces differ; fixed5/17
differ, all five being absolute `time query gametime` receipts. The ten saved body records
and two absent-entity body reads match in this fixture. The independently queried relative
clocks remain strict; no post-capture clock normalization was applied.

## Local checks

The final `pr_ready.py --quick --base fd06d1c2650b52d8d4bb3d808ca73e55971ba717` run
passes full Clippy (including the server configuration), formatting, public-tree, 1,138
Rust tests and doc tests. It exits1 because three existing optional-release Python cases
hit system Bash's `musl[@]`/`arm64[@]` unbound-variable errors (719 run, two skipped).
The five resource-backed Game regressions and 629-record pair pass separately. The416
broad affected harnesses are listed by `--quick`, not run; no full-green CI claim is made.
