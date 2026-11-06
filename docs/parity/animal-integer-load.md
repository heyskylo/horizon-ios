# Animal integer NBT loads

Only `Age`, `ForcedAge` and `InLove` load conversion is changed. Pinned Java26.3
`AgeableMob.readAdditionalSaveData` and `Animal.readAdditionalSaveData` call
`ValueInput.getIntOr`. `TagValueInput` invokes `NumericTag.intValue`: Float/Double
use `Mth.floor` (Math.floor followed by JVM d2i), while Long narrows to32 bits.
Both finite overflow signs are measured; no guessed cast-then-decrement formula.

The existing `nbt_path::int_value` has precisely those semantics. Make that
unchanged helper available to the loader instead of going through an i64 view.
No global `number`/boolean helper, save clamp, NoAI, growth, breeding, ItemEntity
Health or other codec repair. Signed InLove saving is already independently fixed
by merged #7609; its saver remains untouched and negative controls stay strict.
Pending #10344 (ItemEntity Health) and #9991 (baby eligibility) are not imported.

## Frozen actual Game and fresh Java

Base: `e5875b0b192e64c2de121e30940d39bdd4c7728b`.
Existing authoritative command harness, no network observer or world steps.
27 fresh fixed-UUID NoAI/NoGravity pigs cover absent/default, Byte/Short/Int,
Long narrowing, Float/Double positive/negative fractions, adjacent int bounds and
large finite overflow. Actual UUID/type/tag/Health10/Pos/flags, owner identity,
loaded stone/air geometry and relative world-clock0 positively admit every case.
Age is allowed to be negative. Every field read resets result/success to-99 first.
Fresh Java independently verifies all expected callbacks and result/success stores.
All three fields save as Int, including defaults and negative values.

Predeclared strict **666** complete records; **695** executable full diagnostic.
Only the27 whole-body rows and clock-start/end are diagnostic; all commands remain
raw traced. Scalar messages/styles/field types/callbacks/stores and every setup
control remain strict. No masks/tolerances or after-capture selection changes.

```
python3 tools/test_ageable_integer_load_parity.py --binary /path/to/old-game --work-dir /scratch/old
python3 tools/test_ageable_integer_load_parity.py --binary /path/to/fixed-game --work-dir /scratch/fixed --reference /scratch/old/vanilla.jsonl
python3 tools/test_ageable_integer_load_parity.py --full-diagnostic --binary /path/to/game --work-dir /scratch/full
```

Emitted core script SHA256: `e5a6bcb1cae0853480fd72ce9675bdd0123d3d629c9d38af40f229d9c969d08d`.
Emitted full script SHA256: `f7fe07b37956beb8bf22143d50dd76e26961359df625c3bf8dc59ce130cbefd9`.

Old current: **606/666**. Exactly60 differences: scalar read and result-store for
these ten cases across EACH of Age/ForcedAge/InLove. Every success remains true/1;
these are numeric state failures, not parse/callback-admission failures. All other
controls pass, including signed integer InLove, Long narrowing and default types.
Full old diagnostic: **606/695**, retaining all27 bodies and raw absolute clocks.

| Exact case prefix | Java saved Int (each field) | Old native saved Int |
|---|---|---|
| `float_neg` | `-3` | `-2` |
| `double_neg` | `-3` | `-2` |
| `float_above` | `2147483647` | `-2147483648` |
| `float_below` | `-2147483648` | `2147483392` |
| `double_above` | `2147483647` | `-2147483648` |
| `double_neg_inside` | `-2147483648` | `-2147483647` |
| `double_huge` | `2147483647` | `-1` |
| `double_neg_huge` | `-2147483648` | `0` |
| `float_huge` | `2147483647` | `-1` |
| `float_neg_huge` | `-2147483648` | `0` |

## Reachable regression and retained evidence

The resource-free unit calls actual `mob_from_nbt`, checks all three modeled
fields, then `mob_to_nbt` with NO raw source and with the original compound. It
checks exact Int types/values, absent defaults, NoAI/AgeLocked and unrelated raw
field preservation. Expected constants are independently checked against Java;
no copied cast algorithm or direct model-field setter substitutes for decoding.

Owner-local `.local/h055` retains complete raw styled records, every program and
reference, source/bytecode/ownership evidence, logs and old/fixed executables.
Available timestamps/stage logs remain retained; unknown event times stay unknown.
No game files are published, and no broad whole-body/default-attribute/order or
startup-clock parity is claimed.

The predeclared input specifications (compact JSON hashes3769488f… /31f2a24d…)
include expected-result assertions for Java admission. The existing driver strips
only `expect_result` before execution and emits ordinary JSON spacing. The hashes
above identify those actual execution bytes; all commands, labels and comparison
selection are unchanged. Both specification and execution forms remain retained.

## Fixed paired result

**606/666 old →666/666 fixed**, using byte-identical emitted input scripts and
fresh Java reference bytes. All81 individual state reads and their independently
reset result/success stores pass. The resource-free decode-to-save regression also
fails old at the three wrong values and passes fixed across27 inputs. The shared
NBT-path helper body is byte-for-byte unchanged (visibility only), and ledger
mapping validation reports249 rows with no problems.

## Complete diagnostic residuals

Full diagnostic **606/695 old →666/695 fixed**. Same emitted full script bytes;
the independent fresh full Java references differ only at absolute clocks. All29
remaining complete raw mismatches are listed below; excerpts only describe the
difference and never replace whole-record comparison. Full styles/callbacks/body
values remain retained. No695-green or whole-body/raw-clock equality claim.

| Exact label | Java expected | Fixed native actual |
|---|---|---|
| `clock-start` | `old fresh: 309; fixed fresh: 293` | `0` |
| `absent-body` | `attributes: []` | `attributes: [{id: "minecraft:movement_speed", base: 0.25d}]` |
| `zero-body` | `attributes: []` | `attributes: [{id: "minecraft:movement_speed", base: 0.25d}]` |
| `byte-body` | `attributes: []` | `attributes: [{id: "minecraft:movement_speed", base: 0.25d}]` |
| `short-body` | `attributes: []` | `attributes: [{id: "minecraft:movement_speed", base: 0.25d}]` |
| `int_min-body` | `attributes: []` | `attributes: [{id: "minecraft:movement_speed", base: 0.25d}]` |
| `int_max-body` | `attributes: []` | `attributes: [{id: "minecraft:movement_speed", base: 0.25d}]` |
| `long_above-body` | `attributes: []` | `attributes: [{id: "minecraft:movement_speed", base: 0.25d}]` |
| `long_below-body` | `attributes: []` | `attributes: [{id: "minecraft:movement_speed", base: 0.25d}]` |
| `long_wrap-body` | `attributes: []` | `attributes: [{id: "minecraft:movement_speed", base: 0.25d}]` |
| `long_max-body` | `attributes: []` | `attributes: [{id: "minecraft:movement_speed", base: 0.25d}]` |
| `long_min-body` | `attributes: []` | `attributes: [{id: "minecraft:movement_speed", base: 0.25d}]` |
| `float_pos-body` | `attributes: []` | `attributes: [{id: "minecraft:movement_speed", base: 0.25d}]` |
| `float_neg-body` | `attributes: []` | `attributes: [{id: "minecraft:movement_speed", base: 0.25d}]` |
| `double_pos-body` | `attributes: []` | `attributes: [{id: "minecraft:movement_speed", base: 0.25d}]` |
| `double_neg-body` | `attributes: []` | `attributes: [{id: "minecraft:movement_speed", base: 0.25d}]` |
| `float_inside-body` | `attributes: []` | `attributes: [{id: "minecraft:movement_speed", base: 0.25d}]` |
| `float_above-body` | `attributes: []` | `attributes: [{id: "minecraft:movement_speed", base: 0.25d}]` |
| `float_min-body` | `attributes: []` | `attributes: [{id: "minecraft:movement_speed", base: 0.25d}]` |
| `float_below-body` | `attributes: []` | `attributes: [{id: "minecraft:movement_speed", base: 0.25d}]` |
| `double_inside-body` | `attributes: []` | `attributes: [{id: "minecraft:movement_speed", base: 0.25d}]` |
| `double_above-body` | `attributes: []` | `attributes: [{id: "minecraft:movement_speed", base: 0.25d}]` |
| `double_neg_inside-body` | `attributes: []` | `attributes: [{id: "minecraft:movement_speed", base: 0.25d}]` |
| `double_below-body` | `attributes: []` | `attributes: [{id: "minecraft:movement_speed", base: 0.25d}]` |
| `double_huge-body` | `attributes: []` | `attributes: [{id: "minecraft:movement_speed", base: 0.25d}]` |
| `double_neg_huge-body` | `attributes: []` | `attributes: [{id: "minecraft:movement_speed", base: 0.25d}]` |
| `float_huge-body` | `attributes: []` | `attributes: [{id: "minecraft:movement_speed", base: 0.25d}]` |
| `float_neg_huge-body` | `attributes: []` | `attributes: [{id: "minecraft:movement_speed", base: 0.25d}]` |
| `clock-end` | `old fresh: 309; fixed fresh: 293` | `0` |

Core fresh reference SHA256: `cb1532bb798fbfbc3f24794bacc0fb6e8192fbd9eb2120eb2ad3c1c72640da70`.

## Required checks and later source checkpoint

Final `pr_ready.py --quick --base e5875b0b192e64c2de121e30940d39bdd4c7728b`:
2024 Rust tests pass, one fails. The existing `drops_match_vanillas_motion_bit_for_bit`
Z last-bit difference at yaw37.5/pitch-21.25 is actual4597448471913462162 versus
expected4597448471913462163. This task's OWN archived old executable reproduces
exactly the same failure. No drop-motion repair. The resource-free H055 test passes
both explicitly and in the full run; the core suite has no failures.

Python:696 tests, four existing macOS packaging failures, one skip. The installer
signature fixture supplies x86_64 assets on this ARM64 host; three optional-release
archive combinations hit Bash3.2 unset arrays. A narrow installer rerun confirms
its prerequisite. The readiness command exits nonzero; no fully green claim.
Rustfmt, clippy with warnings denied, doc tests and public-tree checks pass. Broad
parity is skipped by `--quick`; the scoped/full actual Game pairs ran separately.
The new driver is discovered by the ordinary parity runner without extra flags.

Latest actual main pull: `2993ef0b205e7f6fb9478a1af9fd5084b31a5e5a`.
The integer loader/helpers, command harness/corpus and pinned lock have no delta
from frozen e5875b0b1. Later changes add opt-in legacy combat/rendering, unrelated
to these zero-tick reads. No combined-latest runtime claim or repeat of unaffected
proof. Evidence-only documentation completion is followed by whitespace/public
source checks; no production edits follow the required run.
