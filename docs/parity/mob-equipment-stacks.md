# Standard Mob equipment stacks in commands

Own frozen main `41f822b659789780f58d32c63ddfb6887ea1b8bf`, actual Game/fresh pinned Java26.3: **178/191 → 191/191** complete strict records. The original script/reference bytes stay identical across the pair. No masks, tolerance, normalization or failed-row selection.

Five independent UUID-bound Husks, NoAI/NoGravity, Health20, exact position/type/UUID/load/floor and strict relative-clock0: custom offhand stick3, custom damaged helmet1, plain offhand/head counterparts, custom mainhand diamond3. Numeric custom_data is independently admitted into a real chest before each Mob write. Initial empty-source copying, unrelated equipment, saved equipment and copied destination stacks are read separately, with fresh result/success sentinels and complete feedback/styles/callbacks.

## Every old residual

Thirteen records differ; all other canonical records and every admission match:

| Case | Labels | Java expected | Untouched Game actual |
|---|---|---|---|
| offhand_custom | saved-stack | stick3/custom_data7 compound, callback true/3 | plain stick1, true/2 |
| offhand_custom | saved-count |3|1|
| offhand_custom | saved-components | custom_data7, true/1 | missing, false/0 |
| offhand_custom | copied-stack | stick3/custom_data7/Slot0b, true/4 | plain stick1/Slot0b, true/3 |
| offhand_custom | copied-count |3|1|
| offhand_custom | copied-components | custom_data7, true/1 | missing, false/0 |
| head_custom | saved-stack | helmet damage3/custom_data8, true/3 | plain helmet, true/2 |
| head_custom | saved-components | damage3/custom_data8, true/2 | missing, false/0 |
| head_custom | copied-stack | helmet damage3/custom_data8/Slot0b, true/4 | plain helmet/Slot0b, true/3 |
| head_custom | copied-components | damage3/custom_data8, true/2 | missing, false/0 |
| mainhand_custom | copied-stack | diamond3/custom_data9/Slot0b, true/4 | plain diamond1/Slot0b, true/3 |
| mainhand_custom | copied-count |3|1|
| mainhand_custom | copied-components | custom_data9, true/1 | missing, false/0 |

All writes/copies are admitted; these failures occur after valid component input. Mainhand save already matches old, distinguishing a getter/copy defect. The actual Game regressions independently establish that armor adoption already keeps damage/components in Extra.armor, while offhand loses count at adoption. They fail old at count1 versus3 and copied damage0 versus3. One UUID-inconsistent unit draft was caught before execution and preserved as a rejected setup; it supplies no runtime failure count.

## Existing state and source reach

Reuse `finalize::item_stack/set_item_stack` and the existing Gear stacks for the six standard hand/armor slots; read the authoritative stored stack first, then Extra.armor with its identity check, then the existing plain fallback. Preserve Extra.armor on writes. Synchronize existing command writes with the store already read by the common saver, and invalidate old state for empty/plain replacement. The shared existing store is also read by the common Mob saver and other existing equipment consumers; they now receive retained command input through their existing machinery. Only command reads/writes change. No model field, loader, saved writer, numeric helper, codec or armor-effect formula change, and no natural-drop/interaction parity claim.

| Slot or route | Source treatment | Own runtime coverage |
|---|---|---|
| mainhand | existing full-stack accessor/helper | custom count3, correct save and command copy |
| offhand | existing Gear stack/helper | custom count3, plain, empty, save/copy; actual Game adoption regression |
| head | existing armor stack plus Gear helper | damage3/custom_data, plain, empty, save/copy; actual Game copy regression |
| chest/legs/feet | same existing standard-slot mapping | source reviewed; no separate runtime census |
| body | existing representation/fallback | full-stack/count expansion unresolved under broader #7985 |
| saddle | existing route retained on the source base | H025 admission/access/common Some saver remains separate |
| equine species savers/natural/drop/interaction paths | unchanged | no claim; retained H025 horse offhand-save lead remains separate |

Actual runtime species coverage is Husk. The shared standard-slot command path reaches other modeled Mobs; this is not an all-species save/lifecycle claim. #7985 is an unassigned broader umbrella, with released/refused prior claims and a fork-local rewrite; none is imported or claimed wholesale. #7563's armor state is preserved.

## Keep both with H025

H025 PR11284's exact diff was inspected; it is not imported. Preserve its body/saddle eligibility rule, early saddle getter, saddle setter and common modeled-Some saddle save insertion when combining. The standard-slot mapping here deliberately excludes body/saddle. Preserve this full-stack read/write synchronization alongside those separate arms. No H025 branch edits or general slot admission changes belong here.

Public replay `tools/test_mob_equipment_stacks_parity.py` is byte-identical to the frozen private script. Raw full bodies, absolute clocks, strict reports, artifact/input hashes and setup evidence remain in `.local/h023/`; known phase times are recorded and unknowns remain null. Required checks passed1,078 Game tests, rustfmt, workspace/server Clippy, lock and public-tree checks. The full run exits nonzero only for the same three existing optional-release Python Bash-array failures (`musl[@]`/`arm64[@]: unbound variable`, 708run, one skipped). The382 broader harnesses are listed for CI, not claimed as local passes. After the read-priority refinement, affected rustfmt, both Clippy configurations and all1,078 Game tests passed again; two explicit prepared-resource Game regressions pass on final source. No overall required-run green claim.

## Retained raw diagnostics

Seven predeclared raw traces (five complete bodies, two absolute clocks): old2/7, first-fixed3/7 and final3/7 match. Unrelated full-body fields/order remain verbatim diagnostics, with no full-body equality claim. Absolute clocks are old Java313/Game0 and first-fixed Java321/Game0 and final Java309/Game0 at both start/end; every actual relative delta0 remains strict. Fresh reference and script bytes are identical across the canonical pair.

Final source review retained the first fixed191/191 capture and source checkpoint, then put the existing authoritative `stored_stack` ahead of the armor cache. Other existing consumers already update that store. This read-order refinement changes no state/model/damage policy. The final binary passes191/191 with the same program/reference; only affected formatting/lint/Game checks are repeated after the original required run.
