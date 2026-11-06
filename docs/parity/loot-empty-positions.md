# Empty loot results retain command replacement positions

Frozen base `41f822b659789780f58d32c63ddfb6887ea1b8bf`: actual Game improves **353/362 → 362/362** complete command records against own fresh pinned Java 26.3, using identical script and canonical reference bytes. All positive setup controls pass. No historical71/72 denominator is reused; the coordinator's invalid71 and corrected72 captures remain separate background evidence and were not imported.

Run `python3 tools/test_loot_empty_positions_parity.py --binary /path/to/horizon-client --work-dir /fresh/path`. `--reference-only` captures Java, and `--reference /path/to/vanilla.jsonl` enforces byte-identical fresh canonical reference bytes.

## Contract and source

Fourteen independent real chests have diamond3 in slot0, emerald4 in slot1, gold_ingot5 in slot2. Every case admits actual block/BE id/coordinates, loaded chunk, stone support, exact typed initial inventory, real player identity/Health/mode/position and strict relative-clock 0. Each loot action and complete post-inventory read has fresh result/success sentinels. Slot2 remains a strict unaffected control. All command feedback, styles, callbacks and stores compare as complete records without masks/tolerances.

| Table | Implicit replacement | Explicit count2 | Insert |
|---|---|---|---|
| Emitted iron count0 | clear0, retain emerald1 | clear0/1 | unchanged |
| Emitted zero then iron1 | clear0, iron1 at1 | clear0, iron1 at1 | retain initial slots, iron1 at3 |
| No pool results | unchanged | clear0/1 | unchanged |
| Iron1 | iron1 at0, retain emerald1 | iron1 at0, clear1 | iron1 at3 |

An unmodified `minecraft:empty` entry separately emits no position; implicit replacement stays unchanged. A named built-in diamond-block table is a positive route control. These controls distinguish actual table admission, accepted zero results and actual emitted empty stacks.

Pinned `LootTable.createStackSplitter` forwards a zero-count stack rather than dropping it. `LootCommand.blockReplace` derives implicit count from the list and uses each index; a missing explicit-count entry becomes EMPTY. `EmptyLootItem.createItemStack` emits nothing. The new private emission bit distinguishes no-emission/deferred placeholders from real zero-count items without changing existing selection/modifier calls or RNG draws.

The output policy is enabled only at the actual command replacement destination, through additive inline/named/entity/block evaluation helpers. Existing APIs default to filtered output. Insert/give/spawn and natural consumers retain filtering; source TOOL, block properties, killed-by-player flag, random sequence routing and nested evaluation are reused. Replacement normalizes a zero payload to the existing empty slot representation and reports its actual zero count (`Dropped 0 [Air]`). No generic tracker/formatter change.

The actual Game regression first proves independent diamond/emerald initial slots and a positive loot action, then fails old at slot0 still occupied after zero+iron. The identical test passes fixed (exactly one executed test); an explicit count2/no-pools control verifies clearing. Resource-free evaluator controls retain zero/nested positions, reject true non-emissions, keep filtered outputs identical and compare exact RNG continuation across 32 seeds; existing TOOL controls also cover the command output path.

## Scope and diagnostics

Live proof covers inline loot-table replacement/count/insert and a named-positive control. Entity/mine source adapters are wired through existing context machinery, with evaluator controls; this is no new H036 entity-context or H060 executor proof. H035 pot-decoration component copying is owned separately by Fixer3: keep that source/component work and this output-position policy, without importing the pot patch. Dynamic-drop behavior and native hooks stay on their existing paths; no all-nesting/all-provider claim.

Sixteen raw traces preserve14 full block bodies plus two absolute-clock records separately. Fixed bodies all match (14/16 raw records); the absolute startup clocks remain exact raw diagnostics, while relative delta0 is strict. Old raw diagnostic11/16 contains the three changed chest bodies plus the clocks. Full styled JSONL, Java/native logs, program/reference hashes, bytecode, binaries and rejected attempts remain protected in `.local/h039/`. No full raw-clock equality claim.

## Every old canonical residual

| Label | Fresh Java expected | Old Game actual |
|---|---|---|
| `zero_implicit-loot` | Dropped 0 [Air]; callbacks=[[True, 1]] | Dropped 0 items; callbacks=[[True, 0]] |
| `zero_implicit-loot-result` | result has 1 [h039]; callbacks=[[True, 1]] | result has 0 [h039]; callbacks=[[True, 0]] |
| `zero_implicit-post-inventory` | 1, 200, 1 has the following block data: [{count: 4, Slot: 1b, id: "minecraft:emerald"}, {count: 5, Slot: 2b, id: "minecraft:gold_ingot"}]; callbacks=[[True, 2]] | 1, 200, 1 has the following block data: [{count: 3, Slot: 0b, id: "minecraft:diamond"}, {count: 4, Slot: 1b, id: "minecraft:emerald"}, {count: 5, Slot: 2b, id: "minecraft:gold_ingot"}]; callbacks=[[True, 3]] |
| `zero_implicit-post-inventory-result` | result has 2 [h039]; callbacks=[[True, 2]] | result has 3 [h039]; callbacks=[[True, 3]] |
| `zero_next_implicit-loot` | Dropped 2 items; callbacks=[[True, 2]] | Dropped 1 [Iron Ingot]; callbacks=[[True, 1]] |
| `zero_next_implicit-loot-result` | result has 2 [h039]; callbacks=[[True, 2]] | result has 1 [h039]; callbacks=[[True, 1]] |
| `zero_next_implicit-post-inventory` | 10, 200, 1 has the following block data: [{count: 1, Slot: 1b, id: "minecraft:iron_ingot"}, {count: 5, Slot: 2b, id: "minecraft:gold_ingot"}]; callbacks=[[True, 2]] | 10, 200, 1 has the following block data: [{count: 1, Slot: 0b, id: "minecraft:iron_ingot"}, {count: 4, Slot: 1b, id: "minecraft:emerald"}, {count: 5, Slot: 2b, id: "minecraft:gold_ingot"}]; callbacks=[[True, 3]] |
| `zero_next_implicit-post-inventory-result` | result has 2 [h039]; callbacks=[[True, 2]] | result has 3 [h039]; callbacks=[[True, 3]] |
| `zero_next_explicit2-post-inventory` | 1, 200, 4 has the following block data: [{count: 1, Slot: 1b, id: "minecraft:iron_ingot"}, {count: 5, Slot: 2b, id: "minecraft:gold_ingot"}]; callbacks=[[True, 2]] | 1, 200, 4 has the following block data: [{count: 1, Slot: 0b, id: "minecraft:iron_ingot"}, {count: 5, Slot: 2b, id: "minecraft:gold_ingot"}]; callbacks=[[True, 2]] |

Program SHA256: `65388143725bdf19f225e7675c022ca66e8a8ff27daa032423d0880eb9196830`. Canonical fresh Java reference SHA256: `8efe7d742f47033a7682c7806832964dee25b9a301b47493b5321513906623ad`.

## Final validation

Final production checks ran once (`pr_ready.py --quick --verbose --base 41f822b659789780f58d32c63ddfb6887ea1b8bf`), UTC 16:48:22–16:56:08 on 2026-11-06. Reported 1154 Game/items tests and 26 content-root tests passed; docs, rustfmt, clippy, public tree and unchanged Cargo.lock passed. The separate prepared-resource Game regression ran exactly one test and passed, after failing old at slot0. An initial extraction selected the main executable's zero-test suite; that invocation was rejected and its raw log/binary retained, not counted as proof.

Python ran 708 tests with the same three pre-existing `test_optional_release_assets` failures (masks0/1 `musl[@]`, mask2 `arm64[@]` Bash unbound arrays), plus one skip. Overall `pr_ready` exits1; no all-green claim. The 383 broad affected harnesses are listed for CI by `--quick`; the focused Game/Java pair ran locally.

Exact raw clock traces: old fresh Java309/Game 0, fixed fresh Java301/Game 0; both start/end readings retained without normalization and both strict relative deltas 0. Current-main read-only diff has no relevant change from frozen41f. Source comments and test-only additions after the runtime build do not alter its gameplay code.

Ledger refresh uses the explicit pinned jar and`check --no-compare`; no analysis worktree is created. Its source/helper callee-coverage and heuristic citation limits remain documented in local logs, not expanded into new context policy. Only the persistent checkout is used, and the perf checkpoint/resources remain protected.

The final preservation review also excludes a zero-count internal dynamic placeholder from the opt-in emitted-item list. It leaves the prior filtered/natural behavior unchanged and adds no dynamic-context or H035 pot-copy claim. After this one-bit guard, affected checks passed: all76 Items tests (including the deferred-placeholder/RNG control), Items Clippy, formatting and the rebuilt exactly-one Game regression. The unchanged362-record live program was repeated against the final binary with the same program/reference bytes and passed362/362. The full required run above was not repeated; its unaffected coverage remains qualified to that run.

Final raw diagnostic: all14 block bodies match, while fresh Java305/Game0 absolute-clock traces remain different, with strict relative0 in both. Prior intermediate fixed capture301/Game0 and old309/Game0 remain preserved separately. A failed literal patch needle and its obsolete canceled build are retained as rejected prerequisites, not parity observations.
