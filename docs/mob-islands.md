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

When changing a mob's movement, teleportation, query range, remembered entity references, or
cross-mob writes:

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
