# Hive block-state saved patch

H034 is limited to the exact whole `minecraft:block_state:{honey_level:"0"}`
prototype of `minecraft:beehive` and `minecraft:bee_nest`. `Items` registers both
with that default. `PatchedDataComponentMap.set/asPatch` compares whole values
with the item prototype. The saved encoder omits an equal value; it preserves
explicit empty maps, nonzero honey, extra properties and non-hive items.

The decoder, live `StackData`, unsaved `components_tag`, command admission and
existing saved defaults are unchanged. Resource-free tests exercise both decoded
and fresh/no-source models, explicit removal markers, round-trip save/load and
unchanged live/unsaved values. Both new tests fail on the old encoder at the
expected saved default assertion, then pass with the bounded encoder change.

## Actual Game/Java contract

Frozen source: `09450fb6f9ef40652e604b9cf8a68cee5de8ff54`. Fresh pinned Java 26.3,
prepared resources and the existing actual Game command-execution route. No new
observer. A loaded, positively identified chest has 13 independently absent slots.
Each real insertion has item/count/slot/write-ACK checks, full saved stack reads,
component/property/presence reads and fresh result/success sentinels. Real owner
UUID/type/Health/mode/Pos, chest coordinates/state/floor/load and exact relative
clock zero remain strict. There are zero observation ticks after scene admission.

Cases cover both hive IDs: explicit zero, plain default, level one, empty map,
zero plus unrelated custom data, and zero plus `facing:"south"`; stone's zero is
the counterexample. The final complete saved inventory retains all 13 stacks.
An absent component produces false/0 and stored result/success 0/0; a present
empty compound produces true/0 and stored 0/1. Fresh Java establishes these
callbacks independently. This does not test the separate command-removal seam.

The predeclared **435-record** contract compares complete raw JSON records,
including message parts/styles and callbacks, without masks or tolerances. The
**438-record** full diagnostic additionally records exactly `clock-start`,
`final-parent-block` and `clock-end`. Absolute startup clocks are retained rather
than equated; the relative delta is strict. The whole parent tag is diagnostic;
every actual saved stack and component remains strict.

| Program | Execution JSONL SHA-256 |
| --- | --- |
| 435 | `8d8f06cbc216a0034a6b9cc1552f727223e031c07b5cd46a4a7b78a04971cbbb` |
| 438 | `586510d68b52d6196c6ca6334fe83c597303014d0e7fcf18e6755edc812a1ac6` |

Run independently on Java, then reuse the exact reference bytes with each binary:

```sh
python3 tools/test_hive_default_patch_parity.py --reference-only --work-dir /tmp/h034-java
python3 tools/test_hive_default_patch_parity.py --binary /path/to/old-game --reference /tmp/h034-java/vanilla.jsonl --work-dir /tmp/h034-old
python3 tools/test_hive_default_patch_parity.py --binary /path/to/fixed-game --reference /tmp/h034-java/vanilla.jsonl --work-dir /tmp/h034-fixed
```

Use `--full-diagnostic` for the executable 438-record program, obtaining its own
fresh Java reference first. Each output directory retains the script, full Java
and Game JSONL, traces/logs and a strict report with complete expected/actual
records. Local immutable binaries, predeclared scripts/labels, Java bytecode
inspection and raw captures are retained under `.local/h034/`; no game resources
or bytecode are committed.

No bees, harvesting, silk-touch/CopyBlockState, placement, simulation, generic
prototype framework, other defaults or persistence lifecycle claim is made.

The adjacent pending lodestone-tracker saved-default work (#10306) changes another
component in `encode_components`. Keep both bounded rules when combining them;
H034 neither imports that production branch nor changes its component behavior.

## Results and retained diagnostics

Actual Game strict pair: **382/435 old → 435/435 fixed**. Full diagnostic: **382/438 → 436/438**. No full-diagnostic equality claim. Both pairs reuse byte-identical execution scripts and fresh Java reference bytes.

| Reference | SHA-256 |
| --- | --- |
| 435 raw records | `b2be897a1661deec9f00359cfbb374c6523256f0695216d61995e7f705cd138b` |
| 438 raw records | `a58c6e6406942aae1f5de83ad6666cc6dac7d72bdf688dcb75c7e05ed9e2d15c` |

The 53 old strict differences are 14 each for `hive_zero`/`nest_zero`, 12 each for `hive_keep`/`nest_keep`, and the final full saved inventory. All are removed by the saved encoder fix. No counterexample/setup row is dropped.

Every remaining full-diagnostic difference follows (complete expected/actual text and callbacks). Styled raw parts remain in the retained JSONL/report. The full parent tag now matches as well; no formatter/history change was made. Absolute startup clocks differ; both real relative clock deltas are zero.

| Label | Fresh Java expected | Fixed Game actual |
| --- | --- | --- |
| `clock-start` | `The game time is 317 tick(s); callbacks=[[true,317]]` | `The game time is 0 tick(s); callbacks=[[true,0]]` |
| `clock-end` | `The game time is 317 tick(s); callbacks=[[true,317]]` | `The game time is 0 tick(s); callbacks=[[true,0]]` |

The driver preserves each newly generated Java `vanilla.jsonl` and writes the
immutable chosen reference separately as `comparison-reference.jsonl`. In full
mode the new Java process can have a different absolute startup clock; both
Game binaries are compared to the exact same retained fresh reference. An earlier
full-capture helper attempt asserted those independently generated absolute
clocks must be byte-equal and rejected 297 versus 317. Its complete outputs remain
under `rejected-full-reference-guard`; this was a test-helper reference error,
not a gameplay failure. The 435-record script and all its raw comparisons were
unchanged by that correction.

## Local checks and source checkpoint

On the final production patch: `pr_ready.py --quick --base 09450fb6f` passes
formatting, workspace/server Clippy, 71 item tests, 26 prepared-content tests,
doc tests and public-tree checks. Python runs 696 tests with four unrelated
macOS tooling failures (ARM installer-fixture assets and three Bash 3.2 optional
archive cases), one skip. The narrow installer diagnostic is retained. No
packaging repair is included. The later test-helper reference correction has
its own final unchanged-script old/fixed runtime replay and Python compilation;
public-tree and whitespace checks pass after the evidence additions.

Main checkpoint `9f8f6842dbd0f4355284c192e02f983075bdf1ca` changes sculk and launcher
wardrobe/translations, with no item codec, harness or instruction delta. This is
source review, not a combined-latest-base runtime claim. Available build/check
elapsed times and hashes are retained; unknown earlier start times stay unknown.
