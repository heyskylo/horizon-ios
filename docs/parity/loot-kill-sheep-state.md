# Actual sheep state in loot-kill commands

Issue [#11664](https://github.com/horizon-rust/horizon/issues/11664). Frozen base `ebcfa2263aa3d8e399aa993b1e05daf42eb4cfa0`; fresh pinned Java26.3. `LootCommand.dropKillLoot` binds the selected live entity as THIS_ENTITY without killing it. Native passed only the type to its Items evaluator, which rejected nonempty entity-properties checks. Merged7506 already supports the nested wool tables;7505 is the independent actual-death table path.

## Strict command proof

The predeclared181-record program uses independent UUID-bound white/red sheep crossed with sheared/unshorn. Actual type, Health8, Pos/Motion, NoAI/NoGravity, color/sheared flags, loaded floor and chest block entity, initially empty inventories, plain held stick, explicit real player source and exact zero relative world ticks are positive admissions. Targets are alive and unchanged after loot insertion. An independent unconditioned diamond insertion proves execution/output. Using insertion avoids empty-slot replacement semantics.

Complete deterministic command replies/styles/callbacks, fresh result/success stores, item identities, saved wool stacks/count/types and total stack counts are strict. Meat is always independently admitted as mutton in its table-defined1..2 count domain. Exact random meat amounts, complete output inventories and full actor bodies are predeclared separate raw traces; they are retained without a random equality or full-body claim.

Fresh Java admits181 records. Pristine actual Game matches161/181: exactly20 missing-wool consequences on the two unshorn subjects, with all controls and both sheared subjects passing. [Every old expected/actual residual](loot-kill-sheep-state-residuals.jsonl) is retained. Fixed Game matches181/181 with the exact same script and fresh Java reference bytes. Script/reference/binary hashes and all raw receipts are protected in `.local/h036/manifest.json` and capture directories.

```sh
python3 tools/test_loot_kill_sheep_state_parity.py --reference-only --work-dir .local/loot-kill-java
python3 tools/test_loot_kill_sheep_state_parity.py --binary /path/to/game \
  --reference .local/loot-kill-java/vanilla.jsonl --work-dir .local/loot-kill-game
```

## Implementation and checks

The actual command derives its target with existing `mobs::drops::loot_entity` from the registered Mob. An additive `Items::entity_drops_with_target` borrows that target and the actual loaded LootData matcher. Only `entity_properties` with entity=this uses it. The command keeps the Items evaluator, selected table and random sequence/world source, draw order, held tool, by-player attribution and stack/output conversion. Existing `entity_drops` and all no-context callers retain their behavior. Other target families and unavailable matcher contexts keep the prior route.

The existing pure entity matcher and its private item matcher move mechanically onto LootData; Context delegates to those same bodies. A retained before/after extraction check confirms identical bodies apart from data ownership and visibility. This shared extraction reaches existing world loot conditions/functions through delegation; the single new callback path reaches Mob loot-kill commands. Runtime coverage is the four sheep cases, not all species, predicates, custom tables or arbitrary actor identities.

The actual registered Game regression fails old at absent wool for both unshorn colors, with positive meat, actual modeled state and unchanged UUID/runtime ID/health/position/motion/clock controls. Identical assertions pass fixed. Seeded evaluator continuation controls compare old API and contextual entrypoint on unaffected tables/tools/by-player branches and subsequent random draws. A loaded-matcher unit protects color/sheared facts, unbound behavior and non-this selectors. Existing world-loot tests protect shared matcher consumers.

The first test runner selected the binary rather than the Game library and ran zero tests; the next test-only draft had an ambiguous String AsRef assertion. Both are retained as rejected prerequisites, not old-fail proof. A fixed-draft build omitted None initialization in two existing block contexts; retained as a compile prerequisite, then corrected mechanically. No invalid runtime setup or unrelated differences were relabeled as defect evidence.

## Independent work and limits

H035 pot block-component copying, H039 zero-result positions, H041 inline fishing tool and H060 mine-executor presence are separate. Actual diffs were reviewed; no pending production imported. When patches meet in cmd_loot, retain independent mine/tool/copy/destination behavior plus this selected-target callback; retain H039's command-only result-position machinery independently of this predicate binding. Its empty-result option and this target context must both reach nested evaluations; neither replaces the other. H060 block contexts keep this new target absent while carrying their separate executor-presence Boolean.

No actual death, loot attribution H038, global evaluator replacement, natural-drop policy, arbitrary custom-table/namespace closure, connected-player, packet, rendering or all-species parity is claimed. Existing unsupported selectors and unrelated conditions/functions remain unchanged. Original raw random amounts/order and full outputs remain available. Setup/proof/exact implementation start times not recorded precisely stay null; known capture/check times are recorded in the manifest. Required local checks are recorded after completion.

Later main review at `648f9354664ee5b6e423ff6d32624170c2ae4b93` found only unrelated consume-effect icon additions in the touched Items file, plus separate bee/effects/audio/save changes. Sheep loot, matcher, dispatch/sequence and fixture paths are unchanged. The frozen base and pair are retained without imports.

Frozen script SHA256: `e0124ffba57b2b55518cbc358bc9a213bd57503355aabeda4f201ba3da8c8fc4`; Java reference: `8f2ea79f91646325b28c1ecb10ade33f70045d7f11dc022660cbc00eb57856b4`; old executable: `8710e0338b5a0affad4fa85ba9e7036348a1db530db22d7558cb854cb64f9f92`; fixed executable: `94dd3daa48228664861893210a16e22d1033f2119385ed26e7f683cbdab14638`.

## Required local report

Required checks ran once on final production (2026-11-07T07:36:44Z–07:45:50Z):1688 Rust tests,27 content-root tests, Clippy, rustfmt, doc tests and public-tree check pass. Python718 has the same3 optional-release Bash unbound-array failures (masks0/1 musl[@],mask2 arm64[@]),1skip; untouched release scripts, so overall exit1.465 affected harnesses are listed for CI, not run locally; the independent sheep181/181 proof passes. The size warning counts the mechanical92-line matcher move plus tests/reproduction; this remains one bounded target-context repair, not an all-checks-green claim.
