# Baby pig passenger attachment

H008 is limited to the Pig age-specific passenger attachment. Pinned Java26.3
`Pig.getDefaultDimensions` selects `BABY_DIMENSIONS` for a baby. That definition
sets `EntityAttachment.PASSENGER` to `(0F,0.5F,0F)`. Adults retain the EntityType's
`0.86875F`, widened to `0.8687499761581421`. The existing float helpers and
`passenger_attachment` model-scale multiplication are preserved, including the
landed precision work in #8582. No physical dimensions or generic scale change.

## Frozen actual Game / fresh Java proof

Base `9f8f6842dbd0f4355284c192e02f983075bdf1ca`. Existing command-execution route,
no new observer. A positively loaded stone platform holds five distinct typed,
fixed-UUID pigs: an adult rider on a baby vehicle, an adult rider on an adult
vehicle, and an unmounted control. All have Health10, NoAI1, NoGravity1 and
PersistenceRequired1. Initial positions, ages, tags and UUIDs are independently
read. Real `/ride` commands establish both links. `/tick step 1` plus the existing
tick helper advances exactly one world tick; clock delta1 and baby age
-24000→-23999 are strict admissions. NoAI does not freeze age.

Fresh Java measures baby vehicle feet `[4.5,200,4.5]` and rider
`[4.5,200.5,4.5]`; adult vehicle feet `[10.5,200,4.5]` and rider
`[10.5,200.86874997615814,4.5]`. Both causal vehicle UUIDs and each vehicle's
saved passenger UUID are read after that tick. The unmounted actor stays at
`[7.5,200,12.5]` and both real dismount attempts confirm it is unmounted. Fresh
result/success sentinels accompany mounts, positions, relationships and the
unmounted negative. Complete Pos/Motion records remain strict, not projected or
tolerance-compared. The actual owner UUID/Health/mode/Pos and geometry/load are
positive controls.

The predeclared **193** complete raw records include feedback styles/callbacks,
all actor guards and actual relative clocks. The executable **205** diagnostic
adds exactly ten full actor bodies (initial/final for each actor), `clock-start`
and `clock-end`; every scalar/vector/relation record remains in the strict gate.
No full-body or absolute-clock equality is assumed. All raw diagnostic records
are retained, not normalized.

| Program | Execution JSONL SHA-256 |
| --- | --- |
| 193 | `f0f5158a46b6a24ff1c1d506667a69df52bdb7dc51a42f72418e009c870e6248` |
| 205 | `9355d7debfdf577e044efc8b6e685b38ff95f19ec163073c3b3d2cd185ec12cb` |

```sh
python3 tools/test_baby_pig_attachment_parity.py --reference-only --work-dir /tmp/h008-java
python3 tools/test_baby_pig_attachment_parity.py --binary /path/to/old-game --reference /tmp/h008-java/vanilla.jsonl --work-dir /tmp/h008-old
python3 tools/test_baby_pig_attachment_parity.py --binary /path/to/fixed-game --reference /tmp/h008-java/vanilla.jsonl --work-dir /tmp/h008-fixed
```

Use `--full-diagnostic` with its own fresh Java reference for205. Newly generated
Java outputs are retained independently; both Game binaries compare against the
same immutable chosen reference. Full diagnostics keep unequal absolute clocks.

The first192/204 draft omitted `/tick step` while frozen: age stayed-24000,
clock delta0 and neither rider reached its seat. This is a rejected prerequisite,
not an attachment failure. Its source/scripts/raw Java capture remain executable.
The corrected193/205 program was predeclared before fresh Java and native runs.

## Unit and scope

The reachable real-Mob attachment/seat regression fails old with
`[0,0.8687499761581421,0]` instead of `[0,0.5,0]` at age-24000/default scale. It
passes fixed and covers ages-24000/-1/0/42 with default/explicit scale1 and scale2.
Scale2 expects baby1.0/adult1.7374999523162842 from the official float arithmetic.
This is unit coverage; the live Game pair uses scale1. No scale2 absolute-position
parity is claimed.

No growth, collision, dismount, generic scale/dimensions, other species, body
writer, Guest ride/H007, numeric load/H055, packet, rendering or reopen change.
Raw bodies and clocks, immutable binaries, bytecode/ledger/duplicate review,
script/reference hashes and logs are retained locally under `.local/h008/`.

## Results and retained residuals

Strict actual Game pair: **192/193 → 193/193**. The sole old
difference is `baby_rider-final-Pos-read`: Java Y200.5 versus native
Y200.86874997615814. Adult height and every other strict record pass old and fixed.

Full diagnostic: **195/205 → 198/205**. All final full bodies now
match. No205/205, all-initial-body or raw-clock equality claim. The remaining
seven rows follow; complete original bodies, colored message parts and callbacks
remain in the retained raw JSONL/reports. The body rows differ only in the shown
initial saved attributes; no body writer/attribute repair is included.

| Exact label | Java expected | Fixed Game actual | Callbacks expected / actual |
| --- | --- | --- | --- |
| `clock-start` | `The game time is 309 tick(s)` | `The game time is 0 tick(s)` | `[[True, 309]]` / `[[True, 0]]` |
| `baby_vehicle-initial-body` | `attributes: []` | `attributes: [{id:"minecraft:movement_speed",base:0.25d}]` | `[[True, 1]]` / `[[True, 1]]` |
| `baby_rider-initial-body` | `attributes: []` | `attributes: [{id:"minecraft:movement_speed",base:0.25d}]` | `[[True, 1]]` / `[[True, 1]]` |
| `adult_vehicle-initial-body` | `attributes: []` | `attributes: [{id:"minecraft:movement_speed",base:0.25d}]` | `[[True, 1]]` / `[[True, 1]]` |
| `adult_rider-initial-body` | `attributes: []` | `attributes: [{id:"minecraft:movement_speed",base:0.25d}]` | `[[True, 1]]` / `[[True, 1]]` |
| `unmounted-initial-body` | `attributes: []` | `attributes: [{id:"minecraft:movement_speed",base:0.25d}]` | `[[True, 1]]` / `[[True, 1]]` |
| `clock-end` | `The game time is 310 tick(s)` | `The game time is 1 tick(s)` | `[[True, 310]]` / `[[True, 1]]` |

| Immutable fresh reference | SHA-256 |
| --- | --- |
| 193 records | `799c808d3482cf3cb07258b8683b43a2b99071b6fd439ccae78f568c3fa3594c` |
| 205 records | `ca451d6c804b32d6c75181e7ce4920bb525e8ddae113a8138131cdf6344004f1` |

## Local checks and checkpoint

Final production/harness: formatting, workspace/server Clippy, doc tests and
public-tree checks pass. The required client-library suite reports1959 passed,
63 ignored and one failure in the unrelated drop-motion test. The own pre-fix
unit executable reproduces the same yaw37.5/pitch-21.25 Z-bit mismatch:
actual4597448471913462162 versus expected4597448471913462163. The new attachment
regression passes fixed. Python runs696 tests with four existing macOS tooling
failures (ARM installer-fixture assets and three Bash3.2 optional-archive cases),
one skip. No unrelated repair is included.

Later main69fab597a adds optional native-mod dependencies/docs, with no relevant
rider, age, passenger, codec, harness or instruction change. This is source
review only; the runtime proof stays on9f8f6842d. No source update was imported.
Available build/check elapsed times and artifact hashes remain retained; unknown
earlier start times stay unknown.
