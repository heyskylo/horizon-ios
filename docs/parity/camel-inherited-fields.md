# Camel inherited saved fields

Pinned Java26.3 `Camel.addAdditionalSaveData` invokes `AbstractHorse.addAdditionalSaveData`, which always writes actual EatingHaystack/Bred booleans, signed Temper and Tame. `Camel.isTamed` always returns true. Native's Camel arm omitted those fields, while raw passthrough could hide missing model state. Its seven-species horse writer is a separate path.

Frozen base: `7127fb8ed17018270228e42dc443e88fe62f3124`. Issue: [#11650](https://github.com/horizon-rust/horizon/issues/11650).

The repair adds a dedicated `Extra.camel_temper:i32` for Camel/CamelHusk, loads it with existing `int_value`, binds their existing eating flag, and writes the actual four fields. The existing Camel-only debug projection reads the signed field. Legacy `variant3` and all seven other equine paths stay unchanged. Camel always-tame force remains. No feeding, taming, breeding eligibility, timer, dash, pose, mount/equipment or numeric-helper policy changes.

## Frozen command proof

Fresh Java independently admits Camel and CamelHusk, absent/default/nondefault inputs and horse/llama positive controls. Actual UUID/type/Health10/Pos/Motion/NoAI/NoGravity, loaded air/floor, name/tags, a typed past Long pose timestamp, exact callbacks and fresh result/success stores are guarded. Each of the four fields has strict scalar reads, saved presence and exact Byte/Int predicates; complete bodies remain raw diagnostics.

Preparation reaches actual common clock1024 through bounded conditional real tick-step commands before any subjects are summoned. Initial clocks are admitted in0..1023 and preserved exactly in raw traces. Conditional powers1024..1 reach the guarded absolute1024; no direct clock setter or fabricated counter is used. All subjects are then observed at exact relative clock0. LastPoseTick1 is past and remains typed Long1, avoiding the independent H094 boundary.

The predeclared core program has357 records. Untouched actual Game is **314/357 → fixed357/357**, with43 old target-field differences and every setup/pose/control/clock record passing. The separately predeclared full program has615 records and adds negative/large/max Int, negative Float fraction, Long narrowing and nonnumeric fallback cases. Fresh Java admits all615; old native is **486/615 → fixed615/615**, with129 old target-field differences and all other controls passing. Numeric rows remain strict, not masked or normalized. Full raw bodies, absolute clocks and all preparation receipts remain diagnostic traces; no full-body equality is claimed.

All qualified and rejected expected/actual residuals are retained in [camel-inherited-fields-residuals.jsonl](camel-inherited-fields-residuals.jsonl), with qualification explicit. The unmodified scripts/references/binaries/raws and source/bytecode audit are protected in `.local/h092/`.

```sh
python3 tools/test_camel_inherited_fields_parity.py --reference-only --work-dir .local/camel-fields-java
python3 tools/test_camel_inherited_fields_parity.py --binary /path/to/game \
  --reference .local/camel-fields-java/vanilla.jsonl --work-dir .local/camel-fields-game
python3 tools/test_camel_inherited_fields_parity.py --full-diagnostic --reference-only --work-dir .local/camel-fields-full-java
python3 tools/test_camel_inherited_fields_parity.py --full-diagnostic --binary /path/to/game \
  --reference .local/camel-fields-full-java/vanilla.jsonl --work-dir .local/camel-fields-full-game
```

## Actual model and source reach

The real decode/model/no-raw-save regression fails old at absent saved Tame despite actual always-tame state. The unchanged assertions pass fixed for both classes, absence, zero, positive and signed/large values, with no raw source, original raw, and contradictory stale raw. Supported unrelated flags, UUID and unrelated raw fields remain intact. Additional units inspect the actual signed field after existing numeric decoding and mutate the modeled Temper/Bred/eating values against contradictory raw input. Runtime ID, position, motion, health, legacy variant3, pose clock/sitting and dash state are unchanged by save. A seven-species unit preserves their existing output and raw/default behavior without claiming their entire eating codec.

After field addition, its only production consumers are Camel/CamelHusk decode, their save projection and existing Camel-only debug output. The complete variant3 audit is retained. Camel's actual always-tame invariant prevents the taming goal from reading/modifying legacy temper; cactus/rabbit-foot feeding supplies temper gain0. Those consumers and policies are untouched. Other variant3 users are species-guarded for villagers, fish, shulkers, copper golems, llamas and other existing routes. The eating flag is existing state; dash stays flag2. No new observer, registry, schema or generic state engine is added.

## Independence and limits

[H093 #11483](https://github.com/horizon-rust/horizon/pull/11483) adds a distinct signed equine counter for seven other species and deliberately retains Camel legacy adapters. Keep both fields and both scopes: H092 Camel load/save/debug uses camel_temper; H093's seven-species reader/writer and gameplay clamps remain its own. When combined, its Camel getter/debug adapter must yield the actual camel_temper instead of stale variant3; retain existing modification order/limits, not an old Camel storage fallback. No pending H093 implementation is imported here.

[H094 #11508](https://github.com/horizon-rust/horizon/pull/11508) changes pose timestamp boundaries. Keep its complete pose policy and H092's independent inherited-field carry if adjacent Camel load context conflicts. This patch does not edit camel.rs, pose functions, the existing LastPoseTick writer, or equipment branches. The common-clock fixture observes only the admitted past pose control, not zero/future pose correctness.

Runtime coverage is zero-tick Camel/CamelHusk plus horse/llama saved controls; all-seven coverage is a resource-free source/model control, not a runtime census. Owner, other inherited fields, other numeric/nonfinite/malformed shapes, live setters, AI, feeding/taming behavior, packets, restart and whole Camel/horse codecs remain unclaimed. Other species' existing raw/default EatingHaystack behavior remains untouched.

Rejected prerequisites remain executable/raw: v1 placed pose1 in native's actual future clock0; v2 incorrectly assumed Java startup0 (actual305); v3 omitted the top1024 step when native started0 and stopped at1023, failing the strict clock admission. Their mixed counts are not H092 baselines. V4 was predeclared and freshly Java-admitted before the untouched pair. The first model-test build used the wrong float constructor and is retained as a rejected build, not runtime evidence; the corrected real test ran and failed at actual missing projection before production edits.

## Frozen hashes and times

Script SHA256: `40315278dd6734338e1fb2b842ce8af398113d902fa629ff78b7ee7149da9f63`. Core Java reference: `57e6a7706eadeefb915d0e1c3060e906d21ccf9d04b2fb58fc78b9c698fb7199`. Full Java reference: `f7b6fb9db8d31280d40cdeac7ab13b2bd6a35787449c6d5f2d1fa34ffb2bf64e`. Fresh Java bytes are identical between old/fixed in each mode. Fixed executable SHA256: `59ab51fc4c3dcaca971070474c3c2d1a149df655e64b2dc014f427f711a0579e`.

Fixed core ran2026-11-07T06:55:02Z–06:55:14Z; full06:55:14Z–06:55:25Z. Setup/initial-proof/exact implementation start times not recorded precisely remain null; the source-edit observation is retained separately. Final repository-check times/results are recorded after completion.

Later main review: `ebcfa2263aa3d8e399aa993b1e05daf42eb4cfa0` has no relevant-path changes from the frozen base. Its only change is unrelated lightning handling; no production was imported and the frozen pair remains valid.

## Required checks

Required local checks ran once on final production, 2026-11-07T06:56:32Z–07:11:24Z: 3274 Rust tests, Clippy, rustfmt, doc tests and public-tree check passed. Python ran718 with three existing optional-release Bash failures (masks0/1 `musl[@]`, mask2 `arm64[@]` unbound arrays), one skip; `pr_ready` exited1. These release-script paths are untouched. Content-root/launcher checks were not applicable; 410 affected parity harnesses were listed for CI, not run locally. The independent357/615 command proofs and all three model regressions pass. No all-checks-green claim.
