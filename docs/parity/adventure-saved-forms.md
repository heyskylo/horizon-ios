# Two saved adventure-predicate forms (H285, isolated stack path)

The actual ItemStack NBT writers now canonicalize two independently measured forms under both `minecraft:can_break` and `minecraft:can_place_on`: a singleton list of a valid named String in `blocks` becomes that String, and a Compound at `predicates.minecraft:custom_data` becomes text through the existing typed `to_snbt`. Identifier text is not rewritten and no registry validation is added. Tag/reference forms, multiple names, empty/absent/state rules, already textual partial predicates, root custom_data and exact components custom_data retain their prior types.

A single private helper runs only on newly encoded components in `item_nbt::encode_item` and `item_to_nbt_with_items`. StackData.other stays authoritative and saving does not mutate it. The shared `canonical_other_component`, `encode_components`, `components_tag`, decoder and command bridge are unchanged. No new field/schema/registry, raw cache/wire image, predicate engine or formatter is introduced.

## Frozen actual command proof

From the repository root, run `python3 tools/adventure_saved_forms_fixture.py --reference-only --work-dir /fresh/java-path`, then the same script with `--binary /absolute/frozen-game --reference /fresh/java-path/vanilla.jsonl --work-dir /fresh/native-path`. Every directory must be new; use the existing pinned Java26.3/prepared resources. The final full program deliberately still exits1 for the residual below; do not treat it as a fully green258-record run.

The258 strict records use13 actual loaded chest slots, real Player UUID/type/Health/position/mode, actual stone/count3, unrelated Compound custom_data and Int repair_cost, chest id/Intcoordinates, floor/load and zero relative clocks. Complete saved-component replies, complete typed predicates, callbacks/styles and fresh result/success stores remain strict. Canonical strings, multiple names, two outer predicates, state, empty/default, absent and exact custom_data controls remain in the program. No adventure action is performed.

Pristine current `845132a56bbc5aad439608ae4adca7ad4d801bb1` matches241/258. Fixed matches257/258, repairing all16 targeted form/read/type/store rows. The sole remaining record is `final-inventory-count-call`: the aggregate inventory reply differs in component-key ordering, with equal result13 and matching stored result/success. The predeclared full-stack canonical-string control at Slot1 is byte-for-byte the same structured raw record old/fixed and differs from Java, establishing retained layout behavior. The [complete unmodified remaining record](adventure-saved-forms-full-residual.jsonl) is public. No formatter repair, output normalization, row removal or post-failure gate is applied. All258 expected outputs and all raw layouts remain retained.

Original v1 was rejected because its full inventory feedback was incorrectly marked as an independent setup admission even though it contains target values. Before production, v2 changes only that admission flag, preserves all258 strict records and independently reruns fresh Java and pristine native. Fresh reference bytes are identical. The qualified pair uses v2; v1 and its invalid capture remain executable evidence.

## Real model and writer controls

The original seven tests are preserved after formatting: six decoded/no-source/mutated-source type assertions fail old, while canonical/unrelated controls pass; all seven pass fixed. Three added tests bring the focused total to10. They exercise ordinary and withItems writers, actual set_other mutation against retained contradictory input, no model mutation, repeated-save and decoded-save idempotence, unchanged root/exact custom_data, markers and prior unmeasured shapes. The withItems unit uses the existing resource-free Items test constructor, not a fake resolver or a claim about prototype policy. There is no separate raw-save parameter in this ItemStack API.

Nested ItemStackTemplate/direct-encoder paths remain explicitly unclosed. A source-preservation test demonstrates their prior representation and the unchanged shared encoder; it is not a Java parity assertion for that route. No command-removal loss repair is included.

## Caller reach and limits

encode_item feeds ordinary item_to_nbt, encode_items and Player inventory serialization. Existing callers also serialize block-entity inventories, entity equipment/inventories/projectiles/frames/displays/trades, hotbar/vault state and live serialized views including LAN chunk block-entity NBT. The withItems wrapper feeds the ItemEntity writer used by actual Game data reads, persistence and End/Nether transfer serialization. These are source reach, not new runtime delivery/transfer/gameplay claims. Direct component views used by predicates, crafting, spill, recipes/agent and CIT stay on the unchanged shared encoder.

Runtime coverage is the admitted saved chest-stack route and these two forms. Adventure permission/action H197, delivery H283, actual break/place, predicate execution, packets, renderer, restart, all component schemas, identifier aliases and general SNBT/layout policy remain outside the claim. Existing codecs/prototype/removal policies are preserved. Related action/hash/Mob-equipment/pot changes are reviewed, not imported.

All complete v1/v2 scripts/reference/raws, source/bytecode/caller audits, old/fixed binaries and hashes, rejected lookup/setup attempts and known/null phase times remain protected in `.local/h285`. Of17 predeclared raw traces, old2 and fixed4 match exactly; all13 fixed full-stack/body/layout/absolute-clock residuals remain unmodified diagnostics. Disk guards remain10GiB entry/5GiB hard; only verified inactive own executable caches were archived with hashes/extracted verification/restore manifests. Proofs/resources/shared or foreign artifacts and protected refs are untouched.

## Final local checks

Frozen final source: required `pr_ready --quick` ran once from 2026-11-07T18:48:00Z to 18:53:25Z. The 91 horizon-items tests and 27 content-root tests pass, along with rustfmt, Clippy, public-tree and doc checks. The full command exits1: Python runs720 tests with the three known host Bash empty-array failures in optional-release-assets (two skipped). The425 affected parity harnesses are listed for CI, not locally run by quick mode. The separate frozen command proof remains257/258 as reported above; no full-green claim.
