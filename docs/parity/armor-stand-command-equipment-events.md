# Armor-stand command equipment events

H130 covers the equipment game event from the existing `/item` stand slot
writer. It preserves player/mob, dispenser/interaction and native equipment
routes, audio, RNG and equipment policy. No broad equipment or sculk rewrite.

## Authority and repair boundary

Pinned26.3 `LivingEntity.setItemSlot` calls `onEquipItem` with the old and new
stacks. The latter rejects identical item/components and `Entity.firstTick`;
`doesEmitEquipEvent` is true for all stand slots. EQUIPPABLE on the new stack
chooses `equip`; otherwise `unequip`, including empty armor and a held stick.
The separate audio portion of that vanilla method is outside this repair.

The command `Slot::Stand` branch writes the equipment then uses the existing
`Game::post_game_event`, with the stand's actual ID and position. It reuses
`ItemStack::same_item_same_components`, so count alone does not create an event.
Item prototypes and the stack's already-modeled component patch determine
EQUIPPABLE presence. No new component decoder or command admission is added.

A transient `ArmorStand.first_tick` starts true and clears only inside an actual
entity-ticking-area stand tick. A saved movement-speed instance cannot stand in
for that flag: NBT can load the attribute before the first tick. The existing
live-NBT adoption path preserves only this new flag instead of resetting it
when rebuilding the model. The flag is not saved or sent. No other live-NBT
fields are copied or changed by this patch.

## Own live fixture

`tools/test_armor_stand_equip_events_parity.py` uses the existing integrated Game
command harness and official Java command oracle. It needs no new observer or
pack. Actual UUID/type/Health20/Pos/NoGravity/Silent guards bind the stand at
[6.5,200,10.5], with a supported, loaded sensor at[9,200,10]. The real owner and
loaded floor are independently admitted. The stand's first real entity tick is
observed before the five mature cases; a second fresh stand covers suppression
before its own first tick. Silent stands isolate vibrations from equipment audio.

Each case removes the old sensor, drains45 actual ticks of pending block work,
places a fresh sensor and gives it2 readiness ticks. Its phase/power, frequency0
and empty pending/candidate listener state are positive controls. Actual item
commands and fresh result/success sentinels admit all changes, identical writes
and empty writes. Full equipment slot values remain strict. Arrival observations
are at exact action-relative0/1/2/3 ticks; the full program advances301 actual
world ticks. Absolute startup clocks remain raw and separate from that exact
relative invariant.

The declared strict248 program SHA256 is
`a1b5e81fb47c81da595c2e13309feabab83fcd7d8dfcb4ddc8dcb17bd40cda84`.
The executable full276 diagnostic SHA256 is
`e2c53d0c5ec60ed99ae5f5ea37c9261acfc855d9b2cfa9c5d0a22caf71cdfca2`.
Full actor/sensor bodies and the two absolute-clock replies are predeclared
raw diagnostics; their exact labels are in the local declaration files.
Every other complete record, including styles/callbacks/stores, compares
strictly without masks, normalization or numeric tolerance. Both program
variants retain their complete raw output, scripts and comparison references.

Frozen current base is679f8c6e3f1f9f5d5c26a1444e8eec4b514b96e4. The own untouched
Game baseline matches239/248: exactly nine sensor-state/frequency/result records
fail for helmet equip, removal and hand-stick change. Slots, feedback, actor,
first-tick, reset, identical/empty and clock admissions all pass. All nine rows arise from this one command-dispatch gap.

## Reachable regression

The prepared-resource Game unit parses the actual summon/item commands and
observes the real sculk listener candidate. It verifies first-tick suppression
with an independently loaded speed attribute, then an actual stand tick and
helmet equip/removal/hand changes, identical/empty suppression, actual source ID
and event position. The original unit fails untouched code at the intended
post-tick helmet assertion (no candidate versus equip). An additive live-NBT
write control checks retention of the new transient flag; its old-writer
counterfactual and fixed result are retained separately from the untouched
baseline unit.

```sh
python3 tools/test_armor_stand_equip_events_parity.py --reference-only --work-dir /tmp/h130-java
python3 tools/test_armor_stand_equip_events_parity.py --binary /path/to/game --reference /tmp/h130-java/vanilla.jsonl --work-dir /tmp/h130-game
python3 tools/test_armor_stand_equip_events_parity.py --full-diagnostic --binary /path/to/game --work-dir /tmp/h130-full
HORIZON_TEST_RESOURCES=/path/to/prepared cargo test -p horizon-client --lib stand_equip_event_tests -- --ignored
```

Local immutable binaries/archives, command traces, raw bodies, references,
bytecode, declarations, stage times and hashes stay under `.local/h130`.
Unknown times stay null. The initial wrong Cargo test target ran0 tests and is
retained as a rejected prerequisite, never a passing regression. A copied
header's clock description was corrected without changing either declared
program; the final baseline rerun uses the corrected driver too.

## Final pair and retained diagnostics

The strict same-program/reference pair is **239/248 old →248/248 fixed**. Helmet
is inactive through action ticks0/1/2 and active at3, frequency5; removal and
hand-stick changes have the same arrival and frequency4. Identical helmet,
identical empty and pre-first-tick changes keep frequency0/inactive. Every
slot/action/actor/reset/ticking/clock admission passes. The strict reference
SHA256 is`b6ff2a2e71846b64213f39cff5672635f73b79f8c8156c8a618279888320f6ee`.

The complete diagnostic is **250/276 old →262/276 fixed** against the same full
comparison reference. All14 remaining labels and their differences follow;
complete bodies, styles and callbacks remain in the raw files. Non-clock
callbacks match. `source` below is[I;130,0,0,1]. The two missing attribute
instances are`minecraft:armor` and`minecraft:armor_toughness`, both base0.0d;
`movement_speed`0.7d remains present on both sides. Ordering is retained raw.

| Label | Java reference | Fixed Game |
| --- | --- | --- |
| clock-start | time309; callback`[true,309]` | time0;`[true,0]` |
| helmet-sensor-body0 | selector tick357L; source present | tick48L; source absent |
| helmet-sensor-body1 | event source present | source absent |
| helmet-body-after | both zero armor instances present | both absent |
| same_helmet-body-after | both zero armor instances present | both absent |
| remove-sensor-body0 | selector tick457L; source present | tick148L; source absent |
| remove-sensor-body1 | event source present | source absent |
| remove-body-after | both zero armor instances present | both absent |
| same_empty-body-after | both zero armor instances present | both absent |
| hand-sensor-body0 | selector tick557L; source present | tick248L; source absent |
| hand-sensor-body1 | event source present | source absent |
| hand-body-after | both zero armor instances present | both absent |
| first_tick-body-after | both zero armor instances present | both absent |
| clock-end | time610; callback`[true,610]` | time301;`[true,301]` |

The live candidate retains the actual stand source ID, verified by the Game
unit. Saved listener UUID representation is a separate existing limitation;
this repair does not change its serializer. Whole-body attribute materialization
and absolute clock equality are also unclaimed. The full fixture stays strict
and executable; there is no276/276 claim or hidden replacement of captured
clock values. The actual relative delta301 remains strict in the core gate.

The final reviewed main checkpoint is`a589a3dd9be04fa7f17667c0175711aaa92e1147`:
only launcher native-source timestamp staging changed since the frozen base.
Relevant stand, item-command and sculk paths are unchanged. This is source
review, not a combined-latest runtime claim. No H075 production is imported.

## Local checks

Required checks ran once on the final production patch: rustfmt, Clippy with
warnings denied and doctests pass;2997 Rust tests pass with one existing
drop-motion bit mismatch. The same37.5/-21.25 case reproduces on the owned
a171 untouched-production baseline executable: Z bits4597448471913462162
versus4597448471913462163. Its function/core are unchanged through frozen679.
The explicitly run prepared-resource H130 regression passes. Python696 reports
four existing macOS tooling failures and one skip (ARM installer-fixture asset;
Bash3.2 optional-archive arrays). No unrelated repair is included. Final
public-tree/whitespace checks are recorded separately.

Resource planning included both Game and reusable-library links, jobs2/inc0
and a5GiB hard floor. Only verified inactive owned artifacts were archived,
with original paths, hashes and restore metadata retained. The final check's
measured low buffer was8.5GiB; shared dependencies and other tasks were preserved.
