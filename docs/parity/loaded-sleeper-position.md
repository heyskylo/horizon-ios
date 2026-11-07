# Loaded sleeper first-tick position

Pinned Java26.3 `LivingEntity.baseTick` checks `firstTick`, calls `setPosToBed` for its loaded sleeping position, then runs `Entity.baseTick`. `setPosToBed` uses the actual bed's sleep height plus0.125; ordinary red beds therefore place the sleeper at blockY+0.6875. `LivingEntity.readAdditionalSaveData` defers this for a freshly loaded entity. It is a first-tick operation, not repeated snapping or zero-tick placement.

Native already has `Mob.first_tick` and the actual MobWorld/bed context. The final repair shares the existing Game pure part-selection, shape-top and position functions through `bed_geometry`, using public visibility only because gameplay and the reusable Mob layer are separate crates. Game keeps its original helper math and straw head-to-foot rule. A read-only MobWorld adapter uses actual registered bed state/properties and outline facts in world coordinates; it never uses collision height or a fixed-height fallback. Only the loaded first-tick refresh calls the new adapter. The normal start helper delegates mechanically to the shared position math with its existing ordinary-bed height, preserving output and all side effects. Loading, normal start/stop side effects, motion, brain/schedule, bed occupancy events and stand-up search are unchanged. No model/world field was added; the adapter reads existing facts only.

## Frozen proof

Base: `507e0ae6f48d25732398e87785d74345433dc269`. Issue: [#11616](https://github.com/horizon-rust/horizon/issues/11616).

The predeclared172-record actual Game/fresh Java fixture uses two independent NoAI/NoGravity villagers, bound to real distinct UUIDs and tags, loaded on intact north/south red beds. Both head/foot states, facing, occupied flags, loaded floor, type/Health20, zero motion, sleeping_pos and actual clocks are positively admitted. Position is read as full typed doubles through the actual command getter, with separate exact position predicates and fresh result/success sentinels. Body admission does not conflate missing actors with successful position changes.

| Subject | Relative0 | Relative1 | Relative2 |
| --- | --- | --- | --- |
| Loaded NoAI villager | Y200.5625 | Y200.6875 | Y200.6875 |
| Awake NoAI control | Y200 | Y200 | Y200 |
| Normal-AI sleep control | Y200.5625 | Y200.5625 | Y200.5625 |

The normal control starts without sleeping_pos, remembers the actual bed and independently enters sleep during a bounded20-tick setup at midnight. This is a positively admitted fixture window, not an exact goal-timing guarantee. Normal gravity produces MotionY `-0.0784000015258789`; those raw values stay strict rather than assuming NoAI/NoGravity physics. Native already matches that control. The native Mob regression separately calls real normal `start_sleeping` and asserts its immediate positionY+0.6875 and normal occupancy event.

Fresh Java passes all172 records. Untouched actual Game is **164/172**: exactly the eight loaded-sleeper position reads/predicates at relative1/2 differ. Every identity/bed/motion/control/clock/callback record passes. The initial fixed-height draft passed172/172 but was held after identifying the existing authoritative shape-helper boundary. Its binaries, source patch, unit logs and complete diagnostics remain protected. The final shape-based source repeated the unchanged unit and script/reference pair and passes172/172, using the exact same script and fresh reference bytes.

Script SHA256: `6619903d27c315230a9de15277c5836bb5edd122ee4dbe1783e97961667b42b1`.
Fresh Java reference SHA256: `1bf61d165b3d47a189d1e604e249216f81321c91755b7e40b1f4250dedde0005`.

```sh
python3 tools/test_loaded_sleeper_position_parity.py --reference-only --work-dir .local/sleeper-java
python3 tools/test_loaded_sleeper_position_parity.py --binary /path/to/frozen-game \
  --reference .local/sleeper-java/vanilla.jsonl --work-dir .local/sleeper-game
```

The resource-backed `loaded_sleeping_position_refreshes_in_actual_mob_tick` regression uses actual registered bed states and the real entity NBT decoder. It first proves that load alone leavesY64.5625 and sets the live sleeping/NoAI/first_tick state. Actual `Mob.tick` then fails old at64.5625 versus64.6875. The identical test also checks next tick, health, motion, unchanged sleep reference, awake/normal-start counterparts and no repeated startSleeping occupancy event. Missing resources fail admission rather than silently skipping this explicitly invoked test.

## Full-body and wake diagnostics

Both additional modes were predeclared with the final script. `--full-diagnostic` compares all188 records including complete bodies and absolute clocks; `--wake-diagnostic` keeps the172 core records and adds an independently UUID-bound loaded sleeper whose bed is removed, for183 total. It reads actual wake position, sleep-field absence and full before/after bodies. PR9737 stand-up and existing bed-loss behavior remain untouched.

Old full output is164/188; old wake output173/183. All complete expected/actual residuals are retained in [loaded-sleeper-position-residuals.jsonl](loaded-sleeper-position-residuals.jsonl), with draft and final outputs retained separately. Both the fixed-height draft and final geometry source produce172/188 and181/183; none is all-green. No raw-tag, raw absolute-clock or all-wake parity is claimed. Separate diagnostic captures preserve each actual Java session's raw clock rather than demanding cross-session absolute equality. The172 core uses identical reference bytes and strict relative0/1/2.

```sh
python3 tools/test_loaded_sleeper_position_parity.py --full-diagnostic --binary /path/to/game --work-dir .local/sleeper-full
python3 tools/test_loaded_sleeper_position_parity.py --wake-diagnostic --binary /path/to/game --work-dir .local/sleeper-wake
```

All scripts/declarations, pack/world inputs, references, full bodies, actual binaries, source/bytecode review and build/unit logs remain in `.local/h233/`. Rejected v1 had invalid UUID-suffix selector syntax and no tick-step requests; v2 corrected selector binding but still had no actual stepped clocks; v3 had real ticks but incorrectly expected normal gravity sleep to match NoAI positioning/motion. None are qualified native defect counts. Final v4 was predeclared, independently Java-admitted and rerun untouched.

## Source reach and limits

The existing sleeping-state helper supports Villager/WanderingTrader; the first-tick call uses that existing family without adding sleep state for other species. Runtime/unit evidence here is Villager on ordinary red beds, with two facings. The moved existing pure controls cover straw foot selection, replaced/mismatched/missing parts and empty shapes; extra missing-foot/part cases are retained in the shared module. These are geometry/unit controls, not straw runtime parity. Other species, special/modded bed runtime, loaded-unavailable beds, rendering, movement/stand-up policy and broad AI behavior are untested. Missing properties/parts or an empty shape produce no authoritative position. Offset-dependent outlines without an available offset context also produce None, not an invented shape/height; current bed outlines are static. None does not introduce a new wake/removal policy. Existing normal coordinate arithmetic and missing-bed paths remain intact. Actual neighboring PR9737/7964/8093/4020 diffs and current open/closed issues were reviewed; no exact open first-tick repair or pending production import was found.

## Local checks and base review

Final `tools/pr_ready.py --quick --base 507e0ae6f48d25732398e87785d74345433dc269` ran from 2026-11-07T05:15:30Z to05:30:21Z: public tree, rustfmt, Clippy,3,266 Rust tests and doc tests pass. The resource-backed sleeper test was also invoked explicitly and passes on final source. Three Python optional-release subcases fail in the existing Bash scripts (`musl[@]`/`arm64[@]`: unbound variable);718 Python tests ran with one skip. The overall command exits1, not green. The409 affected parity fixtures listed by quick mode were not run; the focused actual Game/Java fixture above was run separately. The scope warning counts503 code lines including the moved helpers, actual Mob regression and executable fixture; these support the same repair.

Later fetched main `eb2d90fb106922f391f6b9883f46b9d60de1d976` has no relevant bed/Mob/helper/instruction/fixture-driver delta. Proof remains frozen on507e; no unrelated main changes were imported. Setup/proof start times not recorded precisely remain null in the private manifest.
