# Command enchantment-map admission

Own frozen main `41f822b659789780f58d32c63ddfb6887ea1b8bf`, actual Game/fresh pinned Java 26.3: **286/430 → 430/430** complete records. Identical protected builtin script/program/reference bytes are used across the pair. All actor/target/load/accepted-input controls pass. No masks, tolerance, error normalization or individual failed-row selection.

Run `python3 tools/test_enchantment_map_validation_parity.py --binary /path/to/game --work-dir /fresh/path`; `--reference-only` captures Java. The optional `--loader-diagnostic` replays the separate original 457 program. Both public replay modes' complete op lists were checked equal to their preserved originals. The builtin pair used the unchanged private v2 script; public mode factoring/header changes do not alter its command program.

## Runtime contract

One real loaded chest, stone floor/air, actual player UUID/type/Health/mode/Pos and strict relative-clock0. Each command independently restores emerald 4 with custom_data 1 in slot0, reads that complete initial stack, resets result/success to-99, attempts replacement, then reads complete saved stack/id/count/typed enchantment component and the original-stack condition. Slot 1 stone 7/custom_data is independently retained. Both components are tested separately on diamond_sword and enchanted_book.

Seventeen cases per component: negative/zero/1/255/256/largeInt, Float1.5, Byte1, String, legacy levels, empty, builtin unknown, fully-qualified alias, Long4294967297, Long9007199254740993, Double-0.5 and unknown custom-namespace key. Accepted aliases and boxed-number narrowing are independently established by Java; no scalar NumericTag floor assumption. Empty-map full stack reads and final parent body are predeclared raw diagnostics, while their target id/count/component-presence/callback controls remain strict.

Java accepts1/255/Float1.5/Byte1/empty/qualified/Long-wrap/Long-beyond2^53. Both Longs narrow exactly to1, including beyond floating precision. Every invalid map is rejected before execution: no action callback, both sentinels stay-99, and exact original emerald stack remains. Native old executes all 18 invalid attempts. No equipment/applicability/max-enchantment-level rule is added.

## Every qualified old residual

All 144 old residuals are exactly the Cartesian product of both component names × the nine rejected cases below × these eight labels: `replace`, `result`, `success`, `saved-stack`, `saved-id`, `saved-count`, `saved-enchantments`, `initial-retained`. No other canonical record differs. Full styled expected/actual JSONL and strict reports remain protected in `.local/h024/`.

| Rejected case | Java codec error reason before `missed input` | Old behavior |
|---|---|---|
| negative | Value -1 outside of range [1:255] | executes, replaces target |
| zero | Value 0 outside of range [1:255] | executes, replaces target |
| over | Value 256 outside of range [1:255] | executes, replaces target |
| large | Value 100000 outside of range [1:255] | executes, replaces target |
| string | Not a number | executes, replaces target |
| legacy | Not a number; Failed to get element minecraft:levels from registry minecraft:enchantment | executes, replaces target |
| unknown | Failed to get element minecraft:h024_missing from registry minecraft:enchantment | executes, replaces target |
| negative_half | Value 0 outside of range [1:255] (original tag-0.5d retained in error) | executes, replaces target |
| custom_unknown | Failed to get element h024:missing from registry minecraft:enchantment | executes, replaces target |

`ItemEnchantments.CODEC` is a registry-keyed unbounded map of `Codec.intRange(1,255)`. Existing `int_of` provides Java boxed Number conversion; it is unchanged. The command-only validator checks all entries, aggregates failed entry reasons and their `missed input`, and rejects partial maps. Persistent `read_levels` and its legacy/partial filtering remain untouched.

## Registry carry and limits

Both existing component-decoder callers receive their existing `Registries`. `Game::enchantments()` supplies active IDs at existing build/function-sync points. Both the priority `by_registry` view and legacy fallback are replaced consistently (not extended); tag data and other registry views remain intact. The shared snapshot also feeds enchantment completion and registry arguments such as `/enchant`, plus component equality parsing in item predicates. Runtime parity claims here remain the measured item replacement components.

The resource-free real parser unit fails old when negative input crosses parsing. The separate prepared-resource Game unit uses actual `Enchantments::with_entries` with the Java-admitted definition, first asserts adoption by Game's authority, then calls actual registry synchronization/build. Old fails at the missing custom snapshot ID. Fixed passes builtin/custom/unknown, both views, tags/unrelated IDs and refresh/removal. No expected enchantment ID is inserted directly into Registries in this carry unit. Pure parser fixtures are builtin-scoped.

The original 457 program/Java reference/native output remain byte-identical and executable as a **rejected loader prerequisite**. Java admits the custom definition, but old native's actual custom-definition witness fails even though the pack/function are enabled. Mixed 309/457 is not a qualified baseline. `Enchantments::load_with` does not merge integrated-world pack definitions; this source-backed omission is separate and unassigned. No loader, network-delivery, world-pack lifecycle or custom-world admission claim/fix is made. Custom carry evidence is explicitly controlled model/sync-path only.

Raw builtin diagnostics: old2/5, fixed3/5. All three saved-stack/body traces match fixed; only two absolute clocks differ. Old Java 281/Game 0 and fixed Java 301/Game 0 are retained verbatim, while both relative deltas 0 are strict. No default-elision or formatter repair was made.

## Final checks

`pr_ready.py --quick --verbose --base 41f822b659789780f58d32c63ddfb6887ea1b8bf` passed 1,122 Rust tests, rustfmt, public-tree and lock checks. It exited nonzero: 708 Python tests include three existing optional-release Bash-array failures (`musl[@]`/`arm64[@]: unbound variable`, one skipped), and the first Clippy run caught a new test-only `field_reassign_with_default` warning. That fixture was corrected without changing values/assertions or production; the affected workspace and server Clippy commands, rustfmt and exact parser regression then all passed. The prepared-resource Game sync regression ran explicitly: one test, passed. Both parser and Game sync regressions fail on the untouched implementation at their intended assertions.

The 382 affected broader harnesses were listed for CI, not claimed as local passes. A final read-only main review remains on the same frozen base. Ledger checking used the actual pinned server jar and `--no-compare`; its remaining generic command/build citation suggestions are not new gameplay proof or a broad closure claim. No comparison worktree was created. Known phase times are recorded; unknown times remain null. The checkout/perf checkpoint, original loader diagnostic and all invalid drafts remain protected.
