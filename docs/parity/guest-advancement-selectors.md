# Connected-guest advancement selectors

The H264 fixture is `tools/test_guest_advancement_selector_parity.py`. It uses two
real network connections, actual root/stone advancement grants and revokes, and
if/unless actions with independently reset result/success/branch sentinels. Both
connections' resolved command feedback and styles are compared. Known definitions
are `minecraft:story/root` (`crafting_table`) and `minecraft:story/mine_stone`
(`get_stone`). No advancement progress is supplied by the test adapter.

## Declared comparison

The known-definition program has 1,781 records, 1,462 client commands, six phases
and seven real saved-Time observations. Its JSON SHA-256 is
`aa0f03d9529d3f440d36270c446958af1d4e8bed1dd7a48df36d25ef9611ce7d`.
The full unknown-ID diagnostic has 1,889 records and 1,558 client commands, SHA-256
`12426c9332664a666322e49bbca1187753d882c5f23909f7fe60a6e693927fb5`.
Both programs were declared before the fresh Java, unchanged Game and fixed Game
captures. Every declared record is compared; there is no record-selection mask.

Absolute clocks are deliberately **not** compared as equal across engines. Fresh
Java starts at saved `Data.Time = 66L`; unchanged Game starts at `58L`, fixed Game at `65L`. Complete
copied `level.dat` files and exact absolute values remain in the raw evidence.
Separate exact invariants require each saved Time to remain unchanged within its
own run, and each native model clock to equal that run's actual saved Time. No
world clock is set or fabricated, and no observation tick is added.

Exact absolute saved Time in each final capture (unchanged at every read):

| Program | Java | Unchanged Game | Fixed Game |
| --- | ---: | ---: | ---: |
| Known 1,781 | 66L | 58L | 65L |
| Full 1,889 | 66L | 76L | 68L |


A alone is an operator from startup; B is non-op from its first login. Actual
permission-free same-connection `/list` receipts delimit commands. Completed
console `save-all flush` receipts precede saved-Time reads. Two fixed A receipts
then drain save feedback before the next action label. The commands and their
feedback/stores remain unchanged by these observation receipts.

## Independent state witnesses

Fresh Java phase advancement files establish each player's real progress. Native
uses `HORIZON_DEV_ADVANCEMENT_SELECTOR_AUDIT=1`: at the existing placed-Guest save
loop it logs the actual connection, name, UUID, game time and unchanged
`Guest.advancements.to_json(shared_tree)`. Identical read-only instrumentation is
present in both native binaries. It does not change progress, selectors or saves.
Real disconnect advancement files cross-check both players at the end.

The opt-in test client also retains complete advancement packet bodies and a
bounded replay of the pinned client update path. Login UUID/name, initial reset
and actual receipts are positive controls. Its progress cache is a separate
non-authoritative diagnostic: native omitted empty progress for newly added
entries and retained stale progress after re-addition. Missing packet progress is
never filled from model state. Dates, extra IDs and packet bodies remain raw;
this is not a packet-family, timestamp, UI or reopen claim.

## Reproduction

Use prepared 26.3 resources and the repository's documented isolated test homes:

```sh
python3 tools/test_guest_advancement_selector_parity.py --side java --label java-known
python3 tools/test_guest_advancement_selector_parity.py --side native --label native-known \
  --binary /absolute/path/to/horizon-client \
  --reference .local/guest-advancement-selector-parity/java-known-facts.json
```

Add `--unknown-diagnostic` to both runs for the full diagnostic. Outputs retain
complete wire/partial traces, actual storage, player bodies, phase/final progress,
absolute clocks and a full expected/actual residual report. Use distinct labels;
never overwrite an earlier capture. The reachable Game regression is
`game::scoreboard::guest_advancement_tests::selector_reads_independent_host_and_guest_progress`,
run with `--ignored` and `HORIZON_TEST_RESOURCES` pointing at prepared resources.

## Retained prerequisite diagnostics

Earlier programs remain executable via `--live-save-diagnostic` (1,743/1,851),
`--packet-diagnostic` (1,757/1,865), and `--clock-command-diagnostic` (1,781/1,889).
Their original scripts, references and raw captures remain local. Rejected
criterion-syntax, online-save and packet-reader admissions are not selector
failure counts. The first online-save abort did not retain a full wire dump;
subsequent failures flush and append their actual partial trace.

The earlier model/time-command capture was mixed 1,518/1,781: three additional
records were capture-attribution artifacts (late operator broadcast and a sync
mistaking a scripted time reply for its own). They are not production defects.
The revised program uses saved Time, bounded list receipts and B non-op from
login. It does not repair terminal feedback, recipient routing, packet state,
clock behavior or online save policy.

The gameplay repair only chooses the selected player's existing progress. It
preserves nonplayer rejection and the existing missing-criterion behavior (H266).

## Qualified result

On runtime base `2c6ccefcb7eef239f1ae44d940289a1b47bfd226`, fresh Java admitted
all 1,781 records. Unchanged Game matched 1,521/1,781; fixed Game matched
**1,781/1,781** against the same script/reference. All 260 old residuals are
52 wrong selector actions and their action/state/store-read consequences. Every
independent model, identity, recipient and scene admission passed. Relevant
selector/recipient sources remained unchanged through pulled main `936594e71`;
this does not claim a runtime replay on that later whole-tree revision.

The actual Game traversal regression fails old with `[]` instead of `[Guest(2)]`.
Its independent host/guest progress transitions cover completion and known
criterion true/false comparisons. The network fixture additionally covers global
and named selection, repeated reads, nonplayers and actual recipient actions.

## Full unknown-criterion diagnostic

The unchanged full program matched 1,619/1,889; fixed matched **1,869/1,889**.
The remaining 20 records are H266: a requested false criterion which does not
exist must reject the player before boolean comparison. These are retained
explicit residuals, not part of the known-definition contract and not repaired
by H264. The full program itself has no masked rows. Complete actual report:

| Label | Expected | Actual |
| --- | --- | --- |
| `command1479/feedback` | `{"H264Alice":[[["result has -99 [h264]",[null,false,false,false,false,false]]]],"H264Bob":[]}` | `{"H264Alice":[[["result has 1 [h264]",[null,false,false,false,false,false]]]],"H264Bob":[]}` |
| `command1480/feedback` | `{"H264Alice":[[["success has -99 [h264]",[null,false,false,false,false,false]]]],"H264Bob":[]}` | `{"H264Alice":[[["success has 1 [h264]",[null,false,false,false,false,false]]]],"H264Bob":[]}` |
| `command1481/feedback` | `{"H264Alice":[[["branch has -99 [h264]",[null,false,false,false,false,false]]]],"H264Bob":[]}` | `{"H264Alice":[[["branch has 1 [h264]",[null,false,false,false,false,false]]]],"H264Bob":[]}` |
| `command1487/feedback` | `{"H264Alice":[[["result has 1 [h264]",[null,false,false,false,false,false]]]],"H264Bob":[]}` | `{"H264Alice":[[["result has -99 [h264]",[null,false,false,false,false,false]]]],"H264Bob":[]}` |
| `command1488/feedback` | `{"H264Alice":[[["success has 1 [h264]",[null,false,false,false,false,false]]]],"H264Bob":[]}` | `{"H264Alice":[[["success has -99 [h264]",[null,false,false,false,false,false]]]],"H264Bob":[]}` |
| `command1489/feedback` | `{"H264Alice":[[["branch has 1 [h264]",[null,false,false,false,false,false]]]],"H264Bob":[]}` | `{"H264Alice":[[["branch has -99 [h264]",[null,false,false,false,false,false]]]],"H264Bob":[]}` |
| `command1527/feedback` | `{"H264Alice":[[["result has -99 [h264]",[null,false,false,false,false,false]]]],"H264Bob":[]}` | `{"H264Alice":[[["result has 1 [h264]",[null,false,false,false,false,false]]]],"H264Bob":[]}` |
| `command1528/feedback` | `{"H264Alice":[[["success has -99 [h264]",[null,false,false,false,false,false]]]],"H264Bob":[]}` | `{"H264Alice":[[["success has 1 [h264]",[null,false,false,false,false,false]]]],"H264Bob":[]}` |
| `command1529/feedback` | `{"H264Alice":[[["branch has -99 [h264]",[null,false,false,false,false,false]]]],"H264Bob":[]}` | `{"H264Alice":[[["branch has 1 [h264]",[null,false,false,false,false,false]]]],"H264Bob":[]}` |
| `command1535/feedback` | `{"H264Alice":[[["result has 1 [h264]",[null,false,false,false,false,false]]]],"H264Bob":[]}` | `{"H264Alice":[[["result has -99 [h264]",[null,false,false,false,false,false]]]],"H264Bob":[]}` |
| `command1536/feedback` | `{"H264Alice":[[["success has 1 [h264]",[null,false,false,false,false,false]]]],"H264Bob":[]}` | `{"H264Alice":[[["success has -99 [h264]",[null,false,false,false,false,false]]]],"H264Bob":[]}` |
| `command1537/feedback` | `{"H264Alice":[[["branch has 1 [h264]",[null,false,false,false,false,false]]]],"H264Bob":[]}` | `{"H264Alice":[[["branch has -99 [h264]",[null,false,false,false,false,false]]]],"H264Bob":[]}` |
| `root_B_again.q14_if.action/feedback` | `{"H264Alice":[],"H264Bob":[]}` | `{"H264Alice":[[["Set [h264] for branch to 1",[null,false,false,false,false,false]]]],"H264Bob":[]}` |
| `root_B_again.q14_if/state` | `{"before_branch":-99,"before_result":-99,"before_success":-99,"branch":-99,"result":-99,"success":-99}` | `{"before_branch":-99,"before_result":-99,"before_success":-99,"branch":1,"result":1,"success":1}` |
| `root_B_again.q14_unless.action/feedback` | `{"H264Alice":[[["Set [h264] for branch to 1",[null,false,false,false,false,false]]]],"H264Bob":[]}` | `{"H264Alice":[],"H264Bob":[]}` |
| `root_B_again.q14_unless/state` | `{"before_branch":-99,"before_result":-99,"before_success":-99,"branch":1,"result":1,"success":1}` | `{"before_branch":-99,"before_result":-99,"before_success":-99,"branch":-99,"result":-99,"success":-99}` |
| `root_B_again.q17_if.action/feedback` | `{"H264Alice":[],"H264Bob":[]}` | `{"H264Alice":[[["Set [h264] for branch to 1",[null,false,false,false,false,false]]]],"H264Bob":[]}` |
| `root_B_again.q17_if/state` | `{"before_branch":-99,"before_result":-99,"before_success":-99,"branch":-99,"result":-99,"success":-99}` | `{"before_branch":-99,"before_result":-99,"before_success":-99,"branch":1,"result":1,"success":1}` |
| `root_B_again.q17_unless.action/feedback` | `{"H264Alice":[[["Set [h264] for branch to 1",[null,false,false,false,false,false]]]],"H264Bob":[]}` | `{"H264Alice":[],"H264Bob":[]}` |
| `root_B_again.q17_unless/state` | `{"before_branch":-99,"before_result":-99,"before_success":-99,"branch":1,"result":1,"success":1}` | `{"before_branch":-99,"before_result":-99,"before_success":-99,"branch":-99,"result":-99,"success":-99}` |

The fixed Game traversal regression passes all host/guest transition assertions.

## Earlier capture-attribution records

These three rows belong to the preserved mixed clock-command capture, not the
final program and not production defects:

| Label | Expected | Actual |
| --- | --- | --- |
| `command0001/feedback` | `{"H264Alice":[[["Game rule spawn_mobs is already set to false",[16733525,false,false,false,false,false]]]],"H264Bob":[[["[H264Alice: The game is frozen]",[11184810,false,true,false,false,false]]]]}` | `{"H264Alice":[[["Game rule spawn_mobs is already set to false",[16733525,false,false,false,false,false]]]],"H264Bob":[]}` |
| `command0022/feedback` | `{"H264Alice":[],"H264Bob":[]}` | `{"H264Alice":[[["Test passed. Count: 1",[null,false,false,false,false,false]]]],"H264Bob":[]}` |
| `command0023/feedback` | `{"H264Alice":[[["Test passed. Count: 1",[null,false,false,false,false,false]]],[["Modified storage h264:proof",[null,false,false,false,false,false]]]],"H264Bob":[]}` | `{"H264Alice":[[["Modified storage h264:proof",[null,false,false,false,false,false]]]],"H264Bob":[]}` |

## Local readiness

The scoped Game regression passes fixed (and fails unchanged). `pr_ready.py
--quick --base 2c6ccefcb7eef239f1ae44d940289a1b47bfd226` passed formatting,
workspace/headless Clippy, 1,004 Rust tests, doc tests and the public-tree check.
Python ran 686 tests with four unchanged packaging failures and one skip:
`InstallShSignature.test_signature_cases` supplies only an x86_64 mock release
while this machine selects macOS ARM64; `OptionalReleaseAssetsTests` masks 0/1/2
hit Bash 3.2 unset-array errors. These files and workflow inputs are unchanged.
The final evidence tables were appended after those code checks; the final public
tree and whitespace checks were repeated. Broad automatic parity was skipped by
`--quick`; the declared real two-player fixture was run explicitly above.

After checks, main was actually pulled through `ff4b4b17b`. The only later change
among the reviewed advancement/selector/recipient files adds unrelated structure
tag lookup (#9937); no later whole-tree runtime claim is made.
