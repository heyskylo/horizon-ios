# Signed saved equine Temper

Issue #11462. Frozen own current main `13f70ad482df0a548e1954df8da4b967be3183cb`.

Fresh Java26.3 and actual Game run the identical predeclared503-record command program:21 independent UUID-bound NoAI/no-gravity llama, trader llama and horse actors with absent/0/30/50/100/−7/1000 Temper. All UUID/type/Health/Pos/Motion/NoAI/load/floor, default/coat/strength/chest/tame/bred/tag and zero relative-clock controls remain strict. Each Temper query has fresh result/success sentinels and actual typed saved Int feedback. No failed actor updates serve as resets.

Untouched483/503 → fixed503/503. Exactly20 old scalar read/result residuals follow; all other records already match. The same fresh Java reference bytes are used: SHA256 `db85f0934fc0a7da522fbb0a34ef0a957af37bc6c9771c530adf1b2dc0f54d10`. No selected denominator, masks or tolerances.

| Record | Java expected | Untouched actual |
| --- | --- | --- |
| `llama-fifty-temper` | `[[True, 50]]` | `[[True, 30]]` |
| `llama-fifty-result` | `[[True, 50]]` | `[[True, 30]]` |
| `llama-hundred-temper` | `[[True, 100]]` | `[[True, 30]]` |
| `llama-hundred-result` | `[[True, 100]]` | `[[True, 30]]` |
| `llama-negative-temper` | `[[True, -7]]` | `[[True, 0]]` |
| `llama-negative-result` | `[[True, -7]]` | `[[True, 0]]` |
| `llama-large-temper` | `[[True, 1000]]` | `[[True, 30]]` |
| `llama-large-result` | `[[True, 1000]]` | `[[True, 30]]` |
| `trader_llama-fifty-temper` | `[[True, 50]]` | `[[True, 30]]` |
| `trader_llama-fifty-result` | `[[True, 50]]` | `[[True, 30]]` |
| `trader_llama-hundred-temper` | `[[True, 100]]` | `[[True, 30]]` |
| `trader_llama-hundred-result` | `[[True, 100]]` | `[[True, 30]]` |
| `trader_llama-negative-temper` | `[[True, -7]]` | `[[True, 0]]` |
| `trader_llama-negative-result` | `[[True, -7]]` | `[[True, 0]]` |
| `trader_llama-large-temper` | `[[True, 1000]]` | `[[True, 30]]` |
| `trader_llama-large-result` | `[[True, 1000]]` | `[[True, 30]]` |
| `horse-negative-temper` | `[[True, -7]]` | `[[True, 0]]` |
| `horse-negative-result` | `[[True, -7]]` | `[[True, 0]]` |
| `horse-large-temper` | `[[True, 1000]]` | `[[True, 100]]` |
| `horse-large-result` | `[[True, 1000]]` | `[[True, 100]]` |

## Model and source boundary

A dedicated `Extra::equine_temper:i32` replaces the unrelated byte variant slot for the existing seven saved writers: horse, donkey, mule, skeleton horse, zombie horse, llama and trader llama. Their load path uses the existing caller-specific NBT Int reader without a gameplay clamp; their writer projects the actual signed field. The llama-only load cap is removed. Unrelated `variant3:u8` state remains unchanged.

The broad existing reader also includes Camel/CamelHusk. Those two retain their original byte-load/clamp and legacy getter/modification path; this task does not adopt their saved Temper schema or claim camel parity. The existing debug projection reads the actual equine value while retaining unrelated species' previous byte values. Physical field allocation is shared by Extra, but saved semantics change only for the seven named writers; runtime coverage is the three measured species.

Gameplay is separate: existing taming/feeding consumers use that authoritative counter and `modify_temper` retains0..species-max30/100. Addition explicitly wraps as Java int addition before clamping, including debug builds. Eligibility, call/draw order, event emission and species maxima are retained. No new taming harness, shared numeric helper, coat/strength policy, observer schema, physics, breeding or timer change. Int inputs are measured; no all-numeric-type conversion claim.

H181 issue11447 covers the independent NoAI trader countdown. Its caravan/AI/ints[0] paths and all timer code are untouched; no pending worker production is imported.

## Unit proof

The corrected common decode/save and actual `RunAroundLikeCrazy::tick` test bodies are byte-identical in before/fixed sources, with identical Cargo features/profile. Both fail untouched at intended assertions (loaded50 saves30; loaded1000 survives a successful taming opportunity but saves30). Fixed passes both plus a bounds/overflow/Camel-preservation unit (3/3).

Resource-free decode/save covers signed Int bounds, raw-present/no-raw/stale-raw output, fresh-model/default0 and unrelated raw fields; it cannot pass from raw preservation. The existing prepared mob-world consumer unit requires the fixture to actually run, reads loaded1000/−7 on the three measured species, checks actual tame/buck events and counter projection, and verifies RNG continuation. It exercises the modified goal tick, not navigation, scheduler, player attribution or a full gameplay lifecycle. Bounds controls check wrappingMAX+5 andMIN−1 and unchanged species maxima/other byte state. Camel controls preserve the existing source behavior without a Java parity claim.

Two early test-only string-construction compile attempts and missing-position controls remain rejected prerequisites, with logs/source/artifacts retained. The late absent-field setup correction was applied to both before/fixed unit sources. A protected task checkpoint and temporary detached base in this one checkout supplied the corrected old build; no worktree or second implementation branch was created. No rejected attempt is a runtime failure count.

## Raw diagnostics and ownership

All23 separately predeclared full-body/absolute-clock records remain non-equal both before and after (0/23 exact). Complete expected/actual records, bodies, raw clocks and original outputs are preserved in `.local/h093/{old-v2,fixed-v2}/raw-diagnostic-differences.json` and trace logs; no full-NBT or absolute-clock equality is claimed. The target field and all declared controls remain strict in503/503. These residuals are retained independently without an unrelated serializer/formatter repair.

The original paired `program-v2.py`, references and declarations remain immutable. The public existing-route wrapper `tools/test_equine_temper_parity.py` changes only the corpus report caption; command operations, labels and comparison boundaries are identical. Private `.local/h093/` holds scripts/raws/unit sources/artifacts/hashes, official queries, ownership searches and known phase times (unknown null).

Our first exact claim is https://github.com/horizon-rust/horizon/issues/11462#issuecomment-6697709943. A later competing claim is recorded, and public coordination is https://github.com/horizon-rust/horizon/issues/11462#issuecomment-6697853400. The coordinator verified our earlier claim and authorized continuation; no acknowledgement/unclaim by the other claimant is asserted. Ownership is rechecked before publication. No second issue or competing reservation is created.

## Final local checks

Required checks passed3,239 Rust tests, formatting, workspace/server Clippy and public-tree checks. Python ran712 with three existing optional-release archive test failures (`musl[@]`/`arm64[@]`: Bash unbound variables), one skipped; overall readiness exits1. The409 affected parity harnesses were listed for CI by `--quick`, not run locally. The explicit prepared consumer/model/bounds run passed3/3 separately.

Final source review found main still at13f, identical to the frozen proof base. Final ownership check showed no foreign assignment, in-progress label, new substantive reply or competing PR. The other claimant's acknowledgement remains pending.
