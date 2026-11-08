# Bounded entity movement predicates

Named entity predicates now evaluate `minecraft:movement` for a supported Mob query
snapshot. This changes neither motion nor ticking. The live claim is one actual unmounted
NoAI/NoGravity pig with zero movement and zero observation ticks. No moving Java control
was admitted, so moving behavior, velocity units and update phase are not established by
this runtime scene.

## Authority and supported context

Pinned26.3 `MovementPredicate.matches` reads `Entity.getKnownMovement()`, scales each
component by20, then tests signed x/y/z, total speed squared, horizontal speed squared,
absolute vertical speed and the actual double fall distance, in that order. A living
player-controlled mount may use its controlling player's movement; ServerPlayer has
last-known-client movement. Saved Motion alone is not this general measurement.

The new optional `MovementView` is a query snapshot, not a persistent or previous-tick
store. Only `Game::mob_entity` populates it, from current `Mob.velocity` and
`Mob.fall_distance`, and only when all these conditions hold:

- `m.extra.mount.is_none()`;
- `m.rider.is_none()`;
- local `self.riding != Some(m.id)`;
- no actual Hub guest has `vehicle == Some(m.id)`.

Those guards conservatively leave other contexts unprojected. An unprojected `None`
currently fails this family and is explicitly unsupported; that fallback is not claimed
Java-correct. Existing Player, Guest, vehicle and ownership APIs are unchanged.

| Source path | Reach and proof limit |
|---|---|
| `advancements::entity` | Shared condition dispatch; adds only the movement arm. |
| `Game::mob_entity` | Fresh optional snapshot for supported Mobs; runtime measured pig only. |
| Named entity predicates, criteria, loot, nested Mob views | Shared source reach, not independent runtime coverage. |
| Player/Guest/rider, boat/item/projectile/dragon/lightning and other constructors | Existing construction remains unchanged, default/unprojected unless an existing Mob view is copied. No all-context claim. |

The constructor audit includes default and struct-update paths. All existing id, position,
flags, equipment, NBT, name, scores and team construction remains unchanged.
H269/H270/H271/H268 and named-reference PR11588 are separate; no pending patch is imported.

## Range semantics and added model proof

Movement uses a private exact range matcher. It preserves inclusive min/max comparisons,
exact numeric equality and the official independently squared bounds for speed tests.
There is no EPSILON, square-root comparison, clamp or invented negative-bound rule.
Existing global `range_f` behavior remains unchanged.

Added source/model tests use per-tick inputs giving signed axes3,-4,0 after scaling,
speed5, horizontal3, absolute vertical4 and fall distance7.25. They exercise boundaries,
negative bounds mapped through squaring, exact comparisons and `None` versus a projected
stationary snapshot. These are labeled added/model proof, not moving runtime evidence.
Broader numeric, nonfinite and codec-admission forms remain unclaimed.

## Frozen runtime and actual Game proof

Own pristine base `33facd006aecbc40bdee9a25c179f57cb923d2d3`.
The existing command harness loads seven real predicate files and a fixed-UUID pig.
Independent pack/type predicates and direct identity, Health, Pos, typed Motion,
NoAI/NoGravity, no-passengers, floor/load and relative-zero-clock guards admit the subject.
High x minimum100 rejection, zero-range positive, absent ranges and inversions use fresh
result/success stores and terminal condition callbacks.

The SAME66 program and one frozen official reference produce:

- strict50/62 ->62/62, exactly12 assigned callbacks/stores/unless differences closed;
- all42 independent admissions pass;
- full52/66 ->64/66, retaining all four raw rows and two absolute epoch differences;
- all four native raw JSONL records are byte-identical before/after, including complete
  actor bodies. No output normalization or masks.

The original actual Game test loads real pack files, uses actual summon/model/view
adoption, then registered commands and direct evaluator calls. Its function body is
byte-identical: intended old1failure/0ignored8.41s ->fixed1pass/0ignored8.73s. Only an
unused test-module import outside that body was removed. Added model tests:2pass/0ignored.
Full raw outputs, pack bytes, source/reference hashes, ELFs and known phase timings are
retained locally under `data/entity-predicate-movement/evidence`; unknown times are null.

```sh
python3 tools/test_entity_movement_predicate_parity.py --reference-only --work-dir /tmp/movement-java
python3 tools/test_entity_movement_predicate_parity.py --binary /path/to/horizon-client --reference-dir /tmp/movement-java --work-dir /tmp/movement-pair
```

Use fresh output paths and the pinned resources. Required final checks are reported in
the PR. The original capacity refusal and the explicitly authorized single accepted retry
on issue12406 are preserved; no production edit preceded accepted assignment.
