# Parallel mob tick: reach and acceptance requirements

Design: [#14457](https://github.com/horizon-rust/horizon/issues/14457).
Implementation: `crates/horizon-client/src/game/creatures/mob_islands.rs`.
Parallel ticking is off by default. Design approval and normal implementation review are separate gates.

## Reach is a correctness contract

Two mobs may run in different groups only if neither turn can read or write state whose serial
ordering depends on the other. Bounds must cover the entire turn, including positions reached
after movement or teleportation, nearby queries at those positions, pushes, and brain events.
An initial resting, unloaded, sitting or leashed state is not proof of independence: earlier
turns can change another mob before its turn.

When changing a mob's movement, locomotion attributes or effects, teleportation, query range,
remembered entity references, or cross-mob writes:

1. Audit `reaches`, `jump`, `reads_villages`, ID links and the production fallback checks.
2. Extend the conservative bounds, or retain a serial fallback for every affected tick until
   those bounds are established. Never use an unsupported local bound to keep a tick parallel.
3. Add or extend a behavioral regression with independently placed interacting mobs. Check
   that the planner joins them or that the production entry point actually falls back.
4. For a supported parallel interaction, compare grouped and serial outcomes and assert that
   multiple groups really ran. For a fallback, check production and verify, including that
   verify does not execute a cloned reference tick. A serial-only test cannot prove grouping.

Verify logs discrepancies and retains the grouped result. It is diagnostic coverage, not a
production recovery mechanism or a substitute for the reach audit.

## Acceleration and locomotion settings

Initial velocity does not bound a mob's movement during its turn. `MoveControl.tick` sets
speed from movement or flying attributes before `LivingEntity.travel` adds acceleration.
A stationary cow with movement speed 96 can move into a herd 96 blocks away in one turn,
then query and push its neighbors outside the old initial-velocity envelope.

The current local envelope is therefore restricted to default locomotion. Any mob whose
cached or effective movement speed, jump strength, or effective flying speed differs from
its type's default keeps the entire phase serial in production and verify, without a cloned
reference tick. Speed, Slowness, Jump Boost and Dolphin's Grace also force this fallback:
an effect can expire during the turn, so a currently slowed speed is not a sufficient bound.
The checks apply to every initial role and NoAI state. Non-finite velocity also forces serial.

The `2 * initial_velocity_magnitude + 2` term and local teleport margins are not a bound for
arbitrary locomotion settings. Narrowing this fallback requires conservative bounds for
within-turn acceleration, impulses and every subsequent query, with behavioral regressions.

The resource-backed regression first proves ordinary separated herds actually group on
available hardware. A stationary fast cow then moves into a destination herd through the
real mob-phase entry point. Production and verify must both stay serial, execute no cloned
reference, and match the serial mob and complete level-RNG outcomes. Reach conversion also
checks changed movement, flying and jump attributes, movement effects and non-finite velocity.

## Owner teleports

`TamableAnimal.tryToTeleportToOwner`, called by `FollowOwnerGoal` and
`TamableAnimalPanicGoal`, places a pet around its owner's position, potentially far beyond
its initial velocity and local read range. Enderman and shulker teleport margins do not cover
this destination.

Any mob with `extra.owner: Some` therefore keeps the whole mob phase serial, in production
and verify. This deliberately includes owned species that do not currently teleport and pets
that are sitting, leashed or resting. No species list or current goal state narrows this rule.
Supporting parallel owner teleports later requires accounting for all possible owner views,
destination boxes and subsequent interactions, with destination-herd regressions first.

The resource-backed islands tests include actual wolf, cat and parrot teleports between
separated herds. They first check that the unowned world can group on available hardware,
then assert serial fallback, no diagnostic clone, successful teleports and matching serial
mob/RNG outcomes in production and verify. On one CPU the baseline also stays serial.

```sh
cargo test -p horizon-client --lib game::creatures::mob_islands::tests:: -- --include-ignored --test-threads=1
```

Set `HORIZON_TEST_RESOURCES` to prepared resources and isolate game homes as `AGENTS.md`
requires. Do not set `HORIZON_TEST_SEEDS` when proving grouped execution: it forces fallback.

## Teleport consumables

A mob that finishes an item whose consumable has a `ConsumeEffect::TeleportRandomly` effect
(chorus fruit by default; a fox eats it in `Fox.aiStep`) moves up to half the effect's
`diameter` on each axis, and `Entity.randomTeleport` then drops it to the first ground below,
which can be far down. The diameter is item data and the fall is unbounded, so no margin like
the enderman's or shulker's covers it, and a mob landing beside a herd in another group would be
seen and pushed by that herd's later mobs in the serial loop but not in a grouped one.

`Islands::plan` therefore returns `None`, in production and verify, while any mob holds such an
item in either hand or is using one (a used item is always one of the two hands). The check reads
the item's consumable, never the species, so a future mob that eats is covered as well, and it runs
before the reach conversion: no cloned reference tick runs. Once the item is eaten or dropped,
the next tick can group again.

The resource-backed islands tests include a fox 85 blocks above one herd that finishes chorus
fruit and lands in it. They first check that the same world groups without the fox, then assert
serial fallback, no diagnostic clone, a successful teleport and matching serial mob/RNG outcomes
in production and verify. Another test asserts `plan` is `None` for chorus fruit in either hand.

Other ways one turn can move a mob that were audited when this fallback was added (grep of
position writes, `teleport`, `set_pos`, `move_to` under `mobs/` and `game/creatures/`):

- Enderman and shulker teleports (`jump`) and the hurt-time teleports in `ai.rs`: margins.
- `tame::teleport_to_owner`: the owner fallback above.
- Fox chorus teleport: the fallback in this section.
- Motion: `travel`, vex `noPhysics` travel, turtle and bat adjustments: the velocity reach and
  the locomotion fallback.
- Villager `start_sleeping`/`stop_sleeping` and the first-tick bed position: the villager claim box
  (its bed and points of interest).
- `riders::stop_riding` and `conversion::single`: placed at the vehicle or at the converted mob,
  both ids linked in the reach.
- Block landing (`landing.rs`), resizing (`living.rs`): sub-block adjustments.
- `MobEvent::Push` and `SetVelocity` (ravager roar, hoglin/piglin knockback, entity pushing,
  frog tongue): they change velocity, not position, on a target in the actor's own reads, so the
  target is in its island. Open: a ravager's fling is `4 / d` and unbounded for centres closer than
  about 0.03 blocks, so a mob pressed against a ravager could move farther than its reach in the
  same tick. It is not covered by a fallback; see the PR discussion.

## Before merge

- Pass `pr_ready.py` on the exact proposed head and review the port-ledger warnings. Resolve
  failing tests or establish their cause; missing directories are not a passing result.
- Refresh the affected parity and real server-thread/TCP guest proof on that head. Record
  actual grouped execution and verify counts; fallback-only parity is separate evidence.
- Supply serial-versus-parallel measurements of tick time, including median and tail latency,
  CPU time and planning overhead. Include separated herds, RNG-heavy mobs, crowded worlds
  and owner/raid fallbacks, on representative worker counts. Record hardware, configuration,
  mob counts, warmup, duration and commit. Show that gains justify thread creation and waits
  and that fallback overhead is acceptable before accepting this as a performance change.

These are acceptance requirements, not a claim that the draft has already satisfied them.
