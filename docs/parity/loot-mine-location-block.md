# Loot-mine block-state location checks

Issue [#11677](https://github.com/horizon-rust/horizon/issues/11677). Frozen current base `1f205c93dfbebbce32627480b113613473124c24`, independently fresh pinned Java26.3. The Items evaluator passed every location_check. This repair binds an actual read-only location callback only on the loot-mine command, while retaining that evaluator, its table/sequence selection, actor/tool behavior and output conversion.

## Own command proof

A small actual world pack overrides stone's table with unconditional emerald2 and conditional diamond1. The condition requires an oak log with axis=x at offsetX1. Independent cases have correct state, wrong axis, loaded air and a negative-coordinate correct state. The last case executes from an unrelated fractional source position, proving origin is the mined block, not the executor. All four source/counterpart blocks and loaded floor are independently read before and after; source blocks remain unchanged. Actual player UUID/type/Health/mode/position, held stick, pack function/score42, actual empty chest block entities, exact saved stacks/types/components, callbacks/fresh result/success stores and relative clock0 remain strict. All inventory output is deterministic and compared whole; absolute clocks alone remain raw diagnostic traces.

Fresh Java admits the predeclared162 records. Pristine Game matches142/162: all20 differences are extra conditional diamonds and their feedback/read/store effects for wrong-state and absent counterparts. Correct and negative-center cases and every setup/pack/unconditional/clock control pass. [All old residuals](loot-mine-location-block-residuals.jsonl) are public. Fixed Game matches162/162 with unchanged script/reference bytes; scripts/references/executables, all raw outputs and manifests remain protected in `.local/h079/`.

```sh
python3 tools/test_loot_mine_location_block_parity.py --reference-only --work-dir .local/location-block-java
python3 tools/test_loot_mine_location_block_parity.py --binary /path/to/game \
  --reference .local/location-block-java/vanilla.jsonl --work-dir .local/location-block-game
```

Pinned `LootCommand.dropBlockLoot` supplies `Vec3.atCenterOf(BlockPos)`. `LocationCheck.test` adds integer offsets; `LocationPredicate.matches` uses `BlockPos.containing`, which floors. Thus the measured integer block origin plus half plus integer offset floors to origin+offset, also at negative coordinates. Fresh Java and the actual negative case independently admit this rule. No arbitrary fractional offset or numeric-overflow policy is claimed.

## Source and regression

The additive `Items::block_drops_with_location` borrows `Fn(&Value) -> Option<bool>`; private block context carries it to location_check only. Old public APIs/no-context callers stay unbound with existing behavior. The dynamic-stack expansion, table, selected sequence/world RNG and output path are shared unchanged. The existing pure match_block/block_state_property body moves mechanically to `Items::matches_block_state`; existing callers delegate to that exact body. An extraction equivalence record is retained.

The Game adapter captures the actual mine origin, loaded ChunkMap and registry as field borrows independent from the mutable RNG. It reads the same local block coordinates as World.block and uses actual loaded names/properties. No global World trait, alternate predicate engine, substituted world view, chunk loading or output exception is added. Matcher extraction reaches existing Items block/state conditions; the new callback path reaches only loot-mine. Other item consumers retain their prior no-context behavior, including natural breaks.

An actual registered Game regression creates a real pack/function/table, admits actual world catalog loading and commands, then checks the four source/counterpart/output cases. It fails old only at wrong/absent conditional diamonds and passes fixed with unchanged assertions. Two evaluator controls protect legacy/unbound/unsupported behavior and seeded unaffected output/RNG continuation. A loaded-chunk/registry shape test protects negative coordinates, loaded-air rejection, whole unsupported conjunctions and no chunk-load side effects.

## Supported shape and explicit limits

Some(result) is returned only for location conditions with the known type/predicate/integer-offset fields, a predicate containing only block, a block containing a string blocks selector and optional exact string state properties. Any other location field, unsupported predicate/block field, noninteger/out-of-range offset or missing loaded chunk makes the whole callback return None. Unbound/unsupported None deliberately retains legacy true behavior; it is not labeled Java-correct. Loaded absent counterpart means actual air and is measured. Unloaded targets, array selectors, state ranges, other location fields, overflow/extreme coordinates and arbitrary fractional offsets are outside runtime coverage.

No full LocationPredicate, biome/light/weather/dimension/catalog, all natural breaks, actor attribution H038, BE-copy H080, packet or general loot-engine closure is claimed. PR9497 is a separate world-engine biome fix and remains untouched. H036's pending entity-target callback is independent; keep both borrowed contexts and their absent initializers if they meet. H039 empty-result positions, H041 fishing tools, H035 block-component copying and H060 executor presence remain independent. No pending production was imported; future integration must preserve both source-presence and location contexts, plus destination-specific empty-result behavior.

Original v1 script/raws remain executable: it incorrectly expected function callback42 while Java returns an empty callback list. The revised v2 keeps the actual function record strict and independently reads its score42 effect. V1 is a rejected guard, not a native baseline; v2 was freshly Java-admitted before pristine native. Known phase observations are retained, unknown start times stay null. Required checks are recorded after completion.

Script SHA256 `903c8c29e30dfe554665a62ef186af5eee57154a93285365660163137c2bc7db`; Java reference `5c0a36e3c1dc14e7b21ede14731c71ee963bc8480b72ef2959c0ab625602666d`; old executable `1fa5a6ba32c335aed8cfda459cec7dcf9de868959e517d1e88e7fad0d34fc8a5`; fixed executable `1339eeab660292ef841bcf3ad91297c7fe8d0942fa36a7de24af343f3fe16ca2`.

## Required local checks

Required checks ran once on final production (2026-11-07T08:09:50Z–08:16:36Z):1212 Rust tests,27 content-root tests, rustfmt, Clippy, doc tests and public-tree check passed. Python718 retains the same3 optional-release Bash unbound-array failures (masks0/1 musl[@],mask2 arm64[@]),1skip; untouched release code, overall exit1.412 affected harnesses were listed for CI, not run locally. The independent frozen command pair is162/162; no all-checks-green or unsupported-context parity claim.

Later main `62a480890` had no relevant-path changes in the reviewed source/fixture/instruction set. The frozen base and paired artifacts remain unchanged; no later production was imported.
