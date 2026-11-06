# Mob-only flags in modeled non-Mob saves

`NoAI` and `PersistenceRequired` belong to `Mob.addAdditionalSaveData`.
Pinned Java 26.3 writes the actual persistence boolean and only writes NoAI
when true. `Entity.saveWithoutId`, `LivingEntity.addAdditionalSaveData`,
`ItemEntity`, `ArmorStand`, `AbstractBoat` and `AbstractChestBoat` do not
serialize those two fields for these non-Mob instances.

## Boundary

The raw-preserving `Base::tag` is deliberately unchanged. Its callers are not
all non-Mob: the generic `vehicle_to_nbt` also supplies `Dragon::to_nbt`.
Only `item_entity_to_nbt`, `armor_stand_to_nbt` and `boat_to_nbt` select the
bounded non-Mob save path. It removes exactly those two keys after preserving
shared Entity data. Mob reconstruction, numeric flag conversion and pickup
getter/overwrite behavior are unchanged.

| Route | Boundary and coverage |
| --- | --- |
| Oak boat / oak chest boat | `boat_to_nbt`; actual Game/Java and decode/save unit |
| Ordinary armor stand | `armor_stand_to_nbt`; actual Game/Java and decode/save unit |
| Valid stone Item entity | `item_entity_to_nbt`; actual Game/Java and decode/save unit |
| Pig control | `mob_to_nbt` unchanged; actual supported/default/false fields and live-versus-stale unit |
| TNT, fangs, ominous spawner, projectiles, painting, cushion, frames | Other Base callers unchanged; untested live |
| Orb / falling block | Base has no raw input; external raw merging remains outside this repair |
| Carts / Dragon / crystal | Generic vehicle helper unchanged; untested live |
| Marker / OtherEntity / unknown records | Separate raw/unmodeled routes unchanged; unresolved H104 limitation |

This is a staged supported-class repair, not whole-H104 closure. Before
extending another route, independently admit that actual class and inspect
its writer and any later raw merge. No generic schema or UUID policy changes.

## Frozen command proof

Base: `0b2f2fda7392a528b452af050a9c1d8e6f1b090f`.
All five earlier `mob_to_nbt` resolver repairs are already upstream; this
pair needs no common API patch or pending H045 production import.

Driver SHA-256:
`017d65120fac2fe27e00d8bd07d61fb877a19196d5ff42205d66a60a80b96b78`.
Fresh Java reference SHA-256:
`beabc536fb2db824a96a056fb884a4bbe9c25b119e15c5f7004603d15558135d`.

The predeclared 315 complete command records use 15 distinct fixed UUIDs:
five classes crossed with absent, Byte 0 and Byte 1 input. Every actor has
positive type/UUID/tag/Pos/Motion admission, Health for stand/pig or an actual
valid stone stack for Item, loaded stone/air geometry and exact relative
clock zero. Silent, NoGravity, Invulnerable, PortalCooldown and Tags stay
strict. Each target field has fresh result/success sentinels, actual read
feedback/callbacks, presence checks, and typed pig values. Byte-zero presence
is different from field absence; false NoAI omission differs from the pig's
always-written false PersistenceRequired.

The untouched Game matches **259/315**: exactly 56 differences are the two
supplied fields on four non-Mob classes (six per zero-input case, eight per
one-input case). Absent inputs, pig controls and all setup admissions pass.
The fixed Game matches **315/315**, using the unchanged script and identical
fresh Java reference bytes, with no canonical residuals. These are not 56
independent defects. The old no-raw/stale-raw decode/save
unit fails at the intended boat Byte-zero omission assertion and passes fixed
(1/1), including both source modes and opposite live Mob values. The same
test source uses an optimized fast artifact before and the repository debug
test artifact after; both actual Game executables use the matching fast profile.

Complete body and absolute game-time records were declared raw diagnostics
before captures. They remain executable and retained in Java/native logs;
raw-field retention and other class/default behavior are not whole-body
parity claims. No masks or tolerance apply to the 315
canonical records; no absolute startup-clock equality is claimed.

The separate 17 raw diagnostic records match 4/17 before and 12/17 after.
The five fixed residuals are retained with full expected/actual records:

| Diagnostic | Java | Fixed Game |
| --- | --- | --- |
| Pig bodies, UUID suffixes 13/14/15 | `attributes: []` | `attributes: [{id: "minecraft:movement_speed", base: 0.25d}]` |
| Start/end absolute clocks | 309 / 309 | 0 / 0 |

The old capture's absolute clocks are 289 / 289 versus native 0 / 0; its
same three pig attribute residuals also remain raw. These separate diagnostics
are not folded into a green aggregate or repaired here.

```sh
python3 tools/test_non_mob_saved_flags_parity.py --reference-only --work-dir .local/h104/java
python3 tools/test_non_mob_saved_flags_parity.py --binary /path/to/game --reference .local/h104/java/vanilla.jsonl --work-dir .local/h104/run
```

Use fresh work directories: the driver preserves rejected attempts and
asserts identical fresh reference bytes. Runtime commands use the existing
Game and official command harness; no observer or packet implementation.
