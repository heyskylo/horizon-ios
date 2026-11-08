# Mob glide travel after modeled eligibility

H169 fixes dry-air travel for an eligible gliding Mob. It leaves merged H168's modeled flag, eligibility, gear wear and random draws intact. The registered Game regression proves actual UUID/runtime identity, loaded elytra eligibility and the modeled flag before checking motion.

## Runtime proof

The frozen command program has **147 strict records**, including complete feedback styles and callbacks. Fresh pinned Java 26.3 and pristine native source `2d22e6a7e3007c28045c6384f1b9ecd9fae819eb` match **141/147**: six rows differ, all the gliding Husk's Pos/Motion and separately stored Pos at ticks 1/2. Every setup/control admission passes. No records are removed or normalized.

Subjects are three independent UUID-bound airborne Husks with real elytra: eligible AI glide, stationary NoAI glide and ordinary-air control. The scene guards UUID/type/Health, loaded floor/chunk, actual gear, relative clocks 0/1/2 and fresh result/success stores. It records full entity bodies and absolute clocks as diagnostics, preserving exact values and ordering. There are no wall impacts in this runtime program.

First glide tick Java position is `[4.5d,299.9823599996567d,8.501782000017165d]`, velocity `[0d,-0.017640000343322755d,0.0017820000171661376d]`. Old native position remains `[4.5d,300d,8.5d]`. The original real registered Game test fails there after all model/gear/identity controls pass. Fixed source matches **147/147**, using byte-identical script and fresh Java reference. All six target differences close. [Original six strict differences](mob-glide-travel-old-differences.jsonl) retain their full feedback/styles/callbacks.

Run the existing actual Game/Java route:

```sh
python3 tools/test_mob_glide_travel_parity.py --reference-only --work-dir PATH
python3 tools/test_mob_glide_travel_parity.py --binary PATH_TO_GAME --work-dir PATH
```

The current common command driver reads an undeclared `seed_sync_gaps_diagnostic` option. This task's wrapper supplies its false default locally; the unrelated diagnostic remains off, with no driver/oracle or observation changes. The original pre-command failure is retained. Program v1 erroneously marked a repeated target Pos projection as setup admission; v2 changes that metadata only. All 147 outputs, expectations and target comparisons remain strict; both fresh Java and the pristine native ELF reran v2.

## Source reach and preservation

`motion::fall_flying_movement` is a mechanical extraction of existing Player arithmetic. The existing Player/Guest/client/server route delegates to it without changing collision, damage, flag or RNG behavior. Existing JVM-derived bit-exact climb/dive tests and Player wall/exit controls cover preservation. The extraction's arithmetic body and the original Game regression are checked against their frozen originals.

Only `Mob::travel` gains a glide branch, after unchanged custom-species, flying and fluid branches. NoAI continues to skip travel. The Mob uses its own existing `do_move` collision/restitution and current velocity, never a Player wrapper or replacement scene. Climbable fallback uses existing air travel and ends modeled gliding. The new math draws no random values.

`DamageKind::FlyIntoWall` maps to `minecraft:fly_into_wall` and follows existing builtin armor-bypass/no-knockback handling. Other damage categories, invulnerability/resistance/enchantment/native hook rules retain their existing treatment. The wall path emits the existing fall `MobEvent::Sound`, then calls existing `Mob::damage`; it does not call `cause_fall_damage`. Exhaustive consumers compile with the new variant. Added actual Game wall collision/armor/source controls are source/unit evidence only, not an additional Java packet/runtime claim.

Runtime coverage is this dry-air Husk scene. Other Mob species, custom travel, controlled/ridden subjects, fluids, climbable behavior, wall-health parity, movement attributes, sounds, RNG schedules, packets and animation have no new runtime parity claim. Full body/epoch diagnostics remain separate. No pending changes are imported. Later main changes to living-vehicle body state, TraderLlama NoAI countdown and controlled-horse air speed were reviewed and are independent.

## Local results

The original Game regression and added wall-source control pass, alongside the existing guest climbable test (3 Game tests). Existing client-library glide controls pass (21 tests), including JVM-derived bit-exact arithmetic and Player wall/exit behavior. The final silent guard is included in the fixed runtime ELF; the final-source rerun passes both Game controls. Required checks run once after the final freeze.

The ledger uses the same pinned client/server-game jars with `--no-compare` and an actual changed-file basis. The map checker reports two pre-existing unrelated `Mob.attemptToShearEquipment` / `Mob.shearItem` targets (`Game::shear_pig_saddle`) missing from mounts.rs; those rows remain unchanged. Heuristic client/callee notes and the moved existing Math.cos citation warning are retained, not treated as extra parity claims.

Final required run: **3563 Rust tests, rustfmt, Clippy, docs and public tree pass**. Python runs 741 tests with the known 3 optional-release Bash3 empty-array failures (4 skipped); overall exit1 remains explicit. The 504 affected parity harnesses are listed, not run, in quick mode; this task's own runtime pair ran separately. Source/tests/inputs were hash-verified unchanged throughout. After completion only documentation and the public wrapper's default work-directory handling changed; Python CLI/compile, input-equivalence, diff and public-tree checks cover that follow-up.

Raw traces: old7/13 exact, fixed9/13 exact. All nine fixed full-body records match; four absolute startup-clock rows remain unequal/raw. The predeclared relative clocks and all147 target/control records stay strict. This is not a claim of whole-epoch equality or all-Mob simulation closure.

Later main `20111ddf7b` was reviewed without imports: supporting-block/fall ordering, water-climb and controlled-horse changes, inside-block damage/teleport handling and unrelated Mob events remain independent. Preserve their existing bodies alongside this additive travel branch and damage-kind mapping. The existing post-travel teleport handling remains in the Mob AI caller.
