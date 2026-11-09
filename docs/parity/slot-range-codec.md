# Inline slot-range name admission (H279)

Frozen source `41f822b659789780f58d32c63ddfb6887ea1b8bf`. Own actual Game/fresh pinned Java26.3, `tools/test_slot_range_codec_parity.py`. All98 declared complete records compare exact feedback/styles/callbacks and fresh result/success stores without masks/tolerances. Eight predeclared raw traces retain full chest bodies and exact absolute clocks separately.

The loaded single chest has27 slots. Cases cover inline container.0 with a populated and an empty target, invalid99/54, wildcard, and globally valid53 (beyond this chest's size). Initial/post full typed inventories and a neighbor control chest remain strict, alongside block/BE identity, coordinates, support/load, actual write results and relative-clock0. The original exact invalid99 command runs before the stored variants.

Untouched Game: **91/98**; fixed Game: **98/98**, same program and byte-identical fresh Java reference; all admissions pass. Seven residuals below concern parse-versus-execution feedback/callbacks/stores. Invalid inputs leave target and neighbor unchanged in both builds; Java rejects without callback and retains -99 sentinels, while old Game reaches execution and stores0. Valid53 must remain accepted, reaching execution with callback(false,0) and stores0. Valid0 replaces one slot; wildcard replaces27.

## Exact untouched residuals

| Label | Java expected | Untouched Game actual | Callback expected / actual |
|---|---|---|---|
| original-inline99 | `Failed to parse structure: Failed to parse either. First: Unknown element name:container.99; Second: Failed to parse either. First: Not a string; Second: Failed to parse either. First: Not a list: {slots:"container.99",type:"minecraft:slot_range"}; Second: Unknown element name:container.99 | ...ainer.99"} with diamond<--[HERE]` | `The target does not have specified slots` | `[]` / `[[False, 0]]` |
| invalid99-fill | `Failed to parse structure: Failed to parse either. First: Unknown element name:container.99; Second: Failed to parse either. First: Not a string; Second: Failed to parse either. First: Not a list: {slots:"container.99",type:"minecraft:slot_range"}; Second: Unknown element name:container.99 | ...ainer.99"} with diamond<--[HERE]` | `The target does not have specified slots` | `[]` / `[[False, 0]]` |
| invalid99-result | `result has -99 [h279]` | `result has 0 [h279]` | `[[True, -99]]` / `[[True, 0]]` |
| invalid99-success | `success has -99 [h279]` | `success has 0 [h279]` | `[[True, -99]]` / `[[True, 0]]` |
| invalid54-fill | `Failed to parse structure: Failed to parse either. First: Unknown element name:container.54; Second: Failed to parse either. First: Not a string; Second: Failed to parse either. First: Not a list: {slots:"container.54",type:"minecraft:slot_range"}; Second: Unknown element name:container.54 | ...ainer.54"} with diamond<--[HERE]` | `The target does not have specified slots` | `[]` / `[[False, 0]]` |
| invalid54-result | `result has -99 [h279]` | `result has 0 [h279]` | `[[True, -99]]` / `[[True, 0]]` |
| invalid54-success | `success has -99 [h279]` | `success has 0 [h279]` | `[[True, -99]]` / `[[True, 0]]` |

## Source and scope

Pinned `RangeSlotSource.MAP_CODEC` reads slots with `SlotRanges.CODEC`, using global names. The inline command codec now reuses `args::slot_ranges`; it has no inventory-size context. `container.53` remains globally valid;54/99 are not names. The resource-free real `resource_or_id` argument regression admits0/53/wildcard/hotbar8, then fails old because54 crosses parsing. Its exact decoder error is independently pinned by the Java pair.

Only inline slot_range name admission changes. There is no target-size check, evaluator, global registry/parser rewrite, contextual source/provider policy or Player/Guest change. Other slot-source families and nested schema validation are not expanded or claimed. Existing contents validation stays intact.

Merged PR9117's original valid51/full52 programs and optional `--slot-codec-diagnostic` remain byte-unchanged in `command_exec_corpus.py`/`test_command_exec_parity.py`. Those counts are preserved historical evidence, not this own current denominator. No H247 production is imported.

## Reproduction and evidence

```sh
python3 tools/test_slot_range_codec_parity.py --reference-only --work-dir /tmp/slot-java/run
python3 tools/test_slot_range_codec_parity.py --binary /absolute/path/to/horizon-client --reference /tmp/slot-java/run/vanilla.jsonl --work-dir /tmp/slot-game/run
```

Output directories must be fresh. Complete scripts, declarations, reference/output JSONL, raw logs, immutable binaries and hashes remain under `.local/h279/`. No failed input is counted as a valid admission.

- Program SHA256: `d23f51bc5a1e7f507d199b1d428172bb4a0bf8e8b81f2fc6416e83f24546249c`
- Fresh Java reference SHA256: `1ef944bc6b57e3a7130d5e31f8c9660e81778d0cb8294a27d8c9a0858e155db3`
- Untouched Game SHA256: `e45a7e8023e21869d10c1b010f3c3617bb092f62e3b941a94bbd986619d1e131`

## Full raw diagnostics

6/8 separately declared full-body/absolute-clock traces match. Exact unmatched text follows. Complete styles/callbacks stay retained; no absolute-clock normalization or equality claim.

```text
/execute store result score start h279 run time query gametime
JAVA: The game time is 293 tick(s)
GAME: The game time is 0 tick(s)
/execute store result score end h279 run time query gametime
JAVA: The game time is 293 tick(s)
GAME: The game time is 0 tick(s)
```

## Local validation

The same resource-free argument-codec regression fails old (`container.54 crossed argument parsing`) and passes in the final required suite. `pr_ready --quick --verbose --base 41f822b659789780f58d32c63ddfb6887ea1b8bf` ran once: **44 command tests passed**, along with rustfmt, clippy (including server features), doc tests and public-tree checks. Overall exit1 is the same three pre-existing macOS optional-release-archive Python subtests (masks0/1 `musl[@]`, mask2 `arm64[@]` unbound arrays);708 ran, one skipped. The381 broad affected harnesses were listed for CI by `--quick`; the focused actual Game pair was run locally. Ledger map-check:158 rows pass; ledger check retains one uncited generic multi-codec dispatcher warning for decode, whose range arm cites the actual MAP_CODEC. No full multi-codec closure is claimed.
