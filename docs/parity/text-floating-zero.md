# Text floating-zero tag creation (H190)

Frozen source `41f822b659789780f58d32c63ddfb6887ea1b8bf`. Own actual Game/fresh pinned Java26.3 via `tools/test_text_floating_zero_parity.py`. All120 complete command records compare exact typed scalar text, styles, callbacks, copy/equality/no-write results and fresh result/success sentinels without masks/tolerances. Five separately declared raw traces retain full storage compounds and absolute startup clocks; no whole-compound ordering equality is claimed.

The20-field matrix includes Float/Double ±zero in suffix/exponent forms, ordinary/exponent unsuffixed doubles, nonzero ±1.25 in both types and signed subnormals1e-40f/1e-310d. Storage anchors independently prove real source/copy existence. Each initial/copy field is read exactly and checked against its typed expected tag. A duplicate copy is a no-write control; setting the equivalent canonical compound must also report no change. Loaded floor/player identity and actual relative-clock0 remain strict. Original negative literals enter the real commands unchanged; the second positive-zero compound is a separate equality test, not input/output normalization.

Untouched Game: **93/120**; fixed Game: **120/120**, same program and byte-identical fresh Java reference; all control admissions pass. The27 residuals below are six negative-zero variants' source/copy reads and typed predicates, plus three canonical no-write action/store observations. This proves semantic tag equality, not a formatter-only difference.

## Exact untouched residuals

| Label | Java expected | Untouched Game actual | Callback expected / actual |
|---|---|---|---|
| f_plain_neg-source | `Storage h190:source has the following contents: 0.0f` | `Storage h190:source has the following contents: -0.0f` | `[[True, 0]]` / `[[True, 0]]` |
| f_plain_neg-source-type | `Test passed. Count: 1` | `Test failed` | `[[True, 1]]` / `[[False, 0]]` |
| f_exp_neg-source | `Storage h190:source has the following contents: 0.0f` | `Storage h190:source has the following contents: -0.0f` | `[[True, 0]]` / `[[True, 0]]` |
| f_exp_neg-source-type | `Test passed. Count: 1` | `Test failed` | `[[True, 1]]` / `[[False, 0]]` |
| d_plain_neg-source | `Storage h190:source has the following contents: 0.0d` | `Storage h190:source has the following contents: -0.0d` | `[[True, 0]]` / `[[True, 0]]` |
| d_plain_neg-source-type | `Test passed. Count: 1` | `Test failed` | `[[True, 1]]` / `[[False, 0]]` |
| d_exp_neg-source | `Storage h190:source has the following contents: 0.0d` | `Storage h190:source has the following contents: -0.0d` | `[[True, 0]]` / `[[True, 0]]` |
| d_exp_neg-source-type | `Test passed. Count: 1` | `Test failed` | `[[True, 1]]` / `[[False, 0]]` |
| bare_plain_neg-source | `Storage h190:source has the following contents: 0.0d` | `Storage h190:source has the following contents: -0.0d` | `[[True, 0]]` / `[[True, 0]]` |
| bare_plain_neg-source-type | `Test passed. Count: 1` | `Test failed` | `[[True, 1]]` / `[[False, 0]]` |
| bare_exp_neg-source | `Storage h190:source has the following contents: 0.0d` | `Storage h190:source has the following contents: -0.0d` | `[[True, 0]]` / `[[True, 0]]` |
| bare_exp_neg-source-type | `Test passed. Count: 1` | `Test failed` | `[[True, 1]]` / `[[False, 0]]` |
| f_plain_neg-copy | `Storage h190:copy has the following contents: 0.0f` | `Storage h190:copy has the following contents: -0.0f` | `[[True, 0]]` / `[[True, 0]]` |
| f_plain_neg-copy-type | `Test passed. Count: 1` | `Test failed` | `[[True, 1]]` / `[[False, 0]]` |
| f_exp_neg-copy | `Storage h190:copy has the following contents: 0.0f` | `Storage h190:copy has the following contents: -0.0f` | `[[True, 0]]` / `[[True, 0]]` |
| f_exp_neg-copy-type | `Test passed. Count: 1` | `Test failed` | `[[True, 1]]` / `[[False, 0]]` |
| d_plain_neg-copy | `Storage h190:copy has the following contents: 0.0d` | `Storage h190:copy has the following contents: -0.0d` | `[[True, 0]]` / `[[True, 0]]` |
| d_plain_neg-copy-type | `Test passed. Count: 1` | `Test failed` | `[[True, 1]]` / `[[False, 0]]` |
| d_exp_neg-copy | `Storage h190:copy has the following contents: 0.0d` | `Storage h190:copy has the following contents: -0.0d` | `[[True, 0]]` / `[[True, 0]]` |
| d_exp_neg-copy-type | `Test passed. Count: 1` | `Test failed` | `[[True, 1]]` / `[[False, 0]]` |
| bare_plain_neg-copy | `Storage h190:copy has the following contents: 0.0d` | `Storage h190:copy has the following contents: -0.0d` | `[[True, 0]]` / `[[True, 0]]` |
| bare_plain_neg-copy-type | `Test passed. Count: 1` | `Test failed` | `[[True, 1]]` / `[[False, 0]]` |
| bare_exp_neg-copy | `Storage h190:copy has the following contents: 0.0d` | `Storage h190:copy has the following contents: -0.0d` | `[[True, 0]]` / `[[True, 0]]` |
| bare_exp_neg-copy-type | `Test passed. Count: 1` | `Test failed` | `[[True, 1]]` / `[[False, 0]]` |
| canonical-no-write | `Nothing changed. The specified properties already have these values` | `Modified storage h190:source` | `[[False, 0]]` / `[[True, 1]]` |
| canonical-no-write-result | `result has 0 [h190]` | `result has 1 [h190]` | `[[True, 0]]` / `[[True, 1]]` |
| canonical-no-write-success | `success has 0 [h190]` | `success has 1 [h190]` | `[[True, 0]]` / `[[True, 1]]` |

## Source and scope

Pinned FloatTag.valueOf and DoubleTag.valueOf compare numerically with zero and return ZERO for either sign. Their ZERO initializers use positive fconst_0/dconst_0. Both text parser paths directly retained parsed sign bits. The repair canonicalizes only numeric zero at their Float/Double construction sites; nonzero and subnormal bits pass through unchanged.

Resource-free units exercise the real modern command parser and world text parser independently with exact bit/type expectations, including minimum signed subnormals and an unchanged Int control. Both fail old at -0.0f (0x80000000 instead of0). A separate binary-decode control already passes old and covers both zero signs, nonzero signs and signed minimum subnormals. Binary read/write code is unchanged.

No primitive formatting, arithmetic, global numeric coercion, grammar, compound order or data-command route change. H189 modern execution parsing remains independent; its grammar/routing changes must preserve these factory guards. Historical52-route and358-format diagnostics remain historical/unmodified, not this own current denominator. No root code/resource/reference payload is imported.

## Reproduce and retain

```sh
python3 tools/test_text_floating_zero_parity.py --reference-only --work-dir /tmp/zero-java/run
python3 tools/test_text_floating_zero_parity.py --binary /absolute/path/to/horizon-client --reference /tmp/zero-java/run/vanilla.jsonl --work-dir /tmp/zero-game/run
```

Fresh output directories are required. Complete scripts/declarations, references, raw JSONL/logs, immutable binaries and hashes are retained under `.local/h190/`.

- Program SHA256: `acdb6aab09c2c1d7865b4bb932c894fe0f218d1ed3dc1020e3dce69f6ab68284`
- Fresh Java reference SHA256: `f6ae6637b0352fea2417d80ca87ef59697c9064b189beb80932694004701ff1d`
- Untouched Game SHA256: `e45a7e8023e21869d10c1b010f3c3617bb092f62e3b941a94bbd986619d1e131`

## Raw diagnostic limits

All120 typed/equality records match. The five separately declared raw records retain compound display-order and absolute startup-clock differences. Complete expected/actual text follows; full styles/callbacks remain in retained logs. No formatting or compound-history repair, raw-clock normalization or whole-body equality claim.

```text
/execute store result score start h190 run time query gametime
JAVA: The game time is 301 tick(s)
GAME: The game time is 0 tick(s)
/data get storage h190:source
JAVA: Storage h190:source has the following contents: {anchor: 1b, samples: {f_sub_neg: -1.0E-40f, bare_plain_pos: 0.0d, f_sub_pos: 1.0E-40f, d_sub_neg: -1.0E-310d, f_value_neg: -1.25f, d_plain_neg: 0.0d, d_exp_pos: 0.0d, d_sub_pos: 1.0E-310d, f_value_pos: 1.25f, d_plain_pos: 0.0d, d_exp_neg: 0.0d, f_exp_neg: 0.0f, f_plain_neg: 0.0f, d_value_pos: 1.25d, f_plain_pos: 0.0f, bare_plain_neg: 0.0d, bare_exp_pos: 0.0d, f_exp_pos: 0.0f, bare_exp_neg: 0.0d, d_value_neg: -1.25d}}
GAME: Storage h190:source has the following contents: {anchor: 1b, samples: {f_sub_neg: -1.0E-40f, bare_plain_pos: 0.0d, f_sub_pos: 1.0E-40f, d_sub_neg: -1.0E-310d, f_value_neg: -1.25f, d_exp_pos: 0.0d, d_plain_neg: 0.0d, d_sub_pos: 1.0E-310d, f_value_pos: 1.25f, d_exp_neg: 0.0d, d_plain_pos: 0.0d, f_exp_neg: 0.0f, f_plain_neg: 0.0f, d_value_pos: 1.25d, f_plain_pos: 0.0f, bare_plain_neg: 0.0d, bare_exp_pos: 0.0d, f_exp_pos: 0.0f, bare_exp_neg: 0.0d, d_value_neg: -1.25d}}
/data get storage h190:source
JAVA: Storage h190:source has the following contents: {anchor: 1b, samples: {f_sub_neg: -1.0E-40f, bare_plain_pos: 0.0d, f_sub_pos: 1.0E-40f, d_sub_neg: -1.0E-310d, f_value_neg: -1.25f, d_plain_neg: 0.0d, d_exp_pos: 0.0d, d_sub_pos: 1.0E-310d, f_value_pos: 1.25f, d_plain_pos: 0.0d, d_exp_neg: 0.0d, f_exp_neg: 0.0f, f_plain_neg: 0.0f, d_value_pos: 1.25d, f_plain_pos: 0.0f, bare_plain_neg: 0.0d, bare_exp_pos: 0.0d, f_exp_pos: 0.0f, bare_exp_neg: 0.0d, d_value_neg: -1.25d}}
GAME: Storage h190:source has the following contents: {anchor: 1b, samples: {f_sub_neg: -1.0E-40f, bare_plain_pos: 0.0d, f_sub_pos: 1.0E-40f, d_sub_neg: -1.0E-310d, f_value_neg: -1.25f, d_exp_pos: 0.0d, d_plain_neg: 0.0d, d_sub_pos: 1.0E-310d, f_value_pos: 1.25f, d_exp_neg: 0.0d, d_plain_pos: 0.0d, f_exp_neg: 0.0f, f_plain_neg: 0.0f, d_value_pos: 1.25d, f_plain_pos: 0.0f, bare_plain_neg: 0.0d, bare_exp_pos: 0.0d, f_exp_pos: 0.0f, bare_exp_neg: 0.0d, d_value_neg: -1.25d}}
/data get storage h190:copy
JAVA: Storage h190:copy has the following contents: {anchor: 1b, samples: {f_sub_neg: -1.0E-40f, bare_plain_pos: 0.0d, f_sub_pos: 1.0E-40f, d_sub_neg: -1.0E-310d, f_value_neg: -1.25f, d_plain_neg: 0.0d, d_exp_pos: 0.0d, d_sub_pos: 1.0E-310d, f_value_pos: 1.25f, d_plain_pos: 0.0d, d_exp_neg: 0.0d, f_exp_neg: 0.0f, f_plain_neg: 0.0f, d_value_pos: 1.25d, f_plain_pos: 0.0f, bare_plain_neg: 0.0d, bare_exp_pos: 0.0d, f_exp_pos: 0.0f, bare_exp_neg: 0.0d, d_value_neg: -1.25d}}
GAME: Storage h190:copy has the following contents: {anchor: 1b, samples: {f_sub_neg: -1.0E-40f, bare_plain_pos: 0.0d, f_sub_pos: 1.0E-40f, d_sub_neg: -1.0E-310d, f_value_neg: -1.25f, d_exp_pos: 0.0d, d_plain_neg: 0.0d, d_sub_pos: 1.0E-310d, f_value_pos: 1.25f, d_exp_neg: 0.0d, d_plain_pos: 0.0d, f_exp_neg: 0.0f, f_plain_neg: 0.0f, d_value_pos: 1.25d, f_plain_pos: 0.0f, bare_plain_neg: 0.0d, bare_exp_pos: 0.0d, f_exp_pos: 0.0f, bare_exp_neg: 0.0d, d_value_neg: -1.25d}}
/execute store result score end h190 run time query gametime
JAVA: The game time is 301 tick(s)
GAME: The game time is 0 tick(s)
```

## Local validation

Required pr_ready ran once final: **516 Rust tests passed**, including both text-path bit/type regressions and the already-correct binary controls. Rustfmt, clippy (including server features), doc tests and public-tree checks passed. Overall exit1 is the same three pre-existing macOS optional-release-archive Python subtests (masks0/1 musl[@], mask2 arm64[@] unbound Bash arrays);708 ran, one skipped. The436 broad affected harnesses were listed for CI by --quick; the focused actual Game pair was run locally.

Ledger refresh initially found no default jar; that failed tooling attempt is retained. Rebuilding with the actual pinned server-game.jar succeeds, but the indexer unconditionally excludes valueOf methods (tools/port_ledger.py:612). Its four stale-citation warnings are therefore false positives: both factories and positive-zero initializers are directly confirmed in that jar’s javap output. No ledger-engine fix is included. An unrelated rest-010 map-check read_loop row is also stale; the attempted note was reverted, leaving mapping files unchanged.
