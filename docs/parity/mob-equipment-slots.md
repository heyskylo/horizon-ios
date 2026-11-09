# Mob command body and saddle slots

Frozen source `41f822b659789780f58d32c63ddfb6887ea1b8bf`. Own actual Game/fresh pinned Java26.3 command program has 170 complete strict records: untouched **152/170 → 170/170**. The same script/reference bytes are used across the pair. No masks, tolerance or error normalization.

## Admission and scope

Seven independent UUID-bound subjects: Husk/Pig body rejects for diamond_chestplate; Horse iron_horse_armor and Wolf wolf_armor body positives; Husk iron_helmet head positive; Husk/Pig saddle positives. Every actor has actual type/Health/Pos/NoAI/NoGravity/UUID guards, loaded floor/chunk, zero observation ticks and exact relative-clock0. Slots start empty. An independently equipped head remains intact for body/saddle cases; the head case retains an offhand stick. Each action resets result/success sentinels, reads saved equipment separately, and copies the actual command slot into a real chest to exercise its getter. Full entity bodies and absolute clocks are retained as raw diagnostics.

Official `LivingEntity.getSlot` creates equipment SlotAccess. `createEquipmentSlotAccess` accepts head/hands directly; other slots accept an empty stack or `getEquipmentSlotForItem(stack)==slot`. That helper uses `Equippable.slot` and `canUseSlot`; base LivingEntity permits slots. Preserve the existing slot listing, rather than inventing a species allowlist. This task adds body/saddle to the existing command equipment-item admission check and reuses `Mob::saddle_item/set_saddle`. Merged #7965's independent head/body state stays intact.

The approved common saved equipment insertion writes only actual `Some(m.saddle_item())` through `item_to_nbt`. Its source reach is all non-equine Mobs passing the common writer; runtime saddle proof is Husk/Pig, not all species. Existing species writers and absent-key behavior remain unchanged. No loader, model field, schema, numeric conversion or component rewrite.

## Every old residual

Eighteen records differ; all other canonical records match, including every setup/control:

| Subjects | Labels | Java expected | Untouched Game actual |
|---|---|---|---|
| Husk/Pig body | replace | No targets accepted item; callback false/0 | Replaced one slot; callback true/1 |
| Husk/Pig body | result, success | stored0 | stored1 |
| Husk/Pig body | copied-stack | no slot0 stack; callback false/0 | diamond_chestplate1; callback true/3 |
| Husk/Pig saddle | replace | Replaced one slot; callback true/1 | No targets accepted item; callback false/0 |
| Husk/Pig saddle | result, success | stored1 | stored0 |
| Husk/Pig saddle | saved-saddle | exact saddle1 compound; callback true/2 | missing; callback false/0 |
| Husk/Pig saddle | copied-stack | exact saddle1/Slot0b compound; callback true/3 | missing; callback false/0 |

Complete styled expected/actual JSONL, strict reports, reference/script/artifact hashes and stage times remain protected in `.local/h025/`. The invalid body model can be hidden by its species save projection, so the independent slot copy and actual Game model assertion matter.

## Retained prerequisites and controls

The original v1 170-record script/reference/native/raw capture is executable and retained as a rejected prerequisite: Horse offhand command admission succeeds but its saved offhand control is absent. It is not this task's baseline or a repaired route. The newly predeclared v2 uses a supported head control; its own fresh Java reference and untouched capture pass every admission. No individual failed target observation was removed.

Actual Game regressions register real subjects, run commands and inspect both existing model and command-save output. Body positives precede the wrong-item rejection; the saddle regression includes Pig's existing saver, Husk's common projection, unchanged head/raw fields/flags/tags, a rejected stick and an empty saddle write. Two early unit attempts retain a Peaceful-difficulty prerequisite failure; a real difficulty command corrects the test setup. No production difficulty/identity policy changes.

Component retention H023, Survival interaction eligibility, horse offhand serialization, body physics, other species and all-equipment lifecycle are outside this proof. No unpublished neighboring production is imported. Related #9956 owns equip sound/game-event behavior; this patch does not change it. #7985 covers general equipment stack count/components.

Run `python3 tools/test_mob_equipment_slots_parity.py --binary /path/to/game --work-dir /fresh/path`; `--reference-only` captures fresh Java. The public script is byte-identical to the frozen v2 pair script.

## Raw diagnostics and checks

The nine predeclared raw traces remain separate: seven full bodies and two absolute clocks. Old2/9 and fixed3/9 match; remaining unrelated full-body differences are retained verbatim without an all-body equality claim. Old Java293/Game0 and fixed Java257/Game0 absolute start/end values remain exact raw evidence; both relative-clock deltas are strict0. Fixed canonical references are byte-identical to the independently captured fresh Java reference.

Both explicit fixed Game regressions pass (two tests), against the identical tests that fail untouched at their intended body/saddle assertions. `pr_ready.py --quick --verbose --base 41f822b659789780f58d32c63ddfb6887ea1b8bf` passed3,158 Rust tests, rustfmt, workspace/server Clippy, lock and public-tree checks. It exited nonzero only for the three existing optional-release Python Bash-array failures (`musl[@]`/`arm64[@]: unbound variable`, 708run, one skipped). The382 affected broader harnesses were listed for CI, not claimed as local passes. No full-check green claim. Final main review retains the same frozen base; no relevant source change required a repeated pair. Broader source reach is not a runtime all-species or lifecycle claim. Known phase times are recorded; unknown times remain null. The perf checkpoint, rejected scripts/references/raw captures and all artifacts remain protected in the one persistent checkout.
