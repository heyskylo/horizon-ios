# Empty saved entity Tags

Pinned Java 26.3 `Entity.saveWithoutId` calls `tags.isEmpty()` and writes `Tags`
only for a nonempty set. The existing `Base::tag` save builder now omits a raw
empty Tags list. It still clones other fields and preserves nonempty tag values
and their order. It does not mutate the raw live membership list. No tag-command,
selector, formatter, cap, normalization or other optional-field change.

## Declared actual Game/Java program

```
python3 tools/test_saved_empty_tags_parity.py --binary /path/to/game --work-dir /scratch/run
python3 tools/test_saved_empty_tags_parity.py --full-diagnostic --binary /path/to/game --work-dir /scratch/full
```

`--reference /scratch/old/vanilla.jsonl` requires byte-identical fresh Java reference
bytes. The existing command harness runs actual authoritative Game with its
connected owner and independently runs pinned Java, using isolated fresh data.
No new observer or synthetic entity/progress state.

Two UUID-bound NoAI/NoGravity pigs, Health 10, actual types/Pos/Persistence, loaded
stone floor/air, owner UUID/Health/mode/Pos and zero actual relative-clock delta
are strict positive controls. The first starts with absent Tags, then receives
`audit`, loses it, and repeats the add/remove cycle. The second is freshly loaded
with explicit `Tags:[]`, then receives and retains `keep`. PortalCooldown 17/23 is
read at every phase as an unrelated raw-field preservation control. No observation
ticks, empty-tag selector predicates, arbitrary multi-tag ordering or data writes.

Every phase observes actual `/tag list`. The saved Tags read has fresh result and
success sentinels at -99; successful zero remains distinct from missing-field
failure. Both `if data` and `unless data` use real scoreboard actions with fresh
branch sentinels, preserving callbacks and false/no-action behavior. Positive
actor identity prevents interpreting a missing entity as a missing field.

Before native capture, the canonical **185** record selection was declared:
every actual membership, Tags read, callback/store, presence/action, typed body,
load/geometry/setup and relative-clock record. Whole body and absolute start/end
clocks are the only nine diagnostic labels from the start. Their commands remain
traced; `--full-diagnostic` records and strictly compares all **194**. Comparisons
use complete raw records, with no masks, tolerances or style normalization.

Frozen source: `4f4a931b21d37bc64ce77081bb0a674248f71ad7`.

| Program | Exact script SHA256 | Old fresh Java reference SHA256 |
|---|---|---|
| Scoped185 | `92b933e511306c3b0eefb568828916f99dabad6d9aff5555133a9fbfdb2698a2` | `374afd9e8d60f0141af349d7d41b67250d0899d807d14d94cbe83b5e2bc53c8f` |
| Full194 | `cebe4bdd29973ab343b1b0668477ab192c3cd8931c2b561ab1dfeacc27b9ecc0` | `1c66164200ac9cbfbfe8e0465c9e27a4ef985b1ab0f33543acb8d6fc1db1126e` |

Unchanged actual Game: **167/185**, with these six target differences in EACH of
`removed`, `removed-again`, `explicit-empty-load` (18 exact labels):

| Label suffix | Java expected | Old native actual |
|---|---|---|
| -Tags | Missing-field feedback, callback false/0 | Tags `[]` feedback, callback true/0 |
| -success | 0 | 1 |
| -if-presence | no action, no callback | action, callback true/1 |
| -if-result | -99 | 1 |
| -unless-presence | action, callback true/1 | no action, no callback |
| -unless-result | 1 | -99 |

Initial absent Tags, all live tag membership, successful nonempty reads, add/remove
acknowledgements, result 0 for the missing/empty read, unrelated fields and clock
admissions pass old. Full old diagnostics: **167/194**, including all raw bodies.

## Reachable regressions and retained evidence

The ignored resource-backed `tag_commands_and_explicit_empty_loads_save_without_empty_tags`
uses actual Game `/summon`, `/tag add`, `/tag remove`, UUID targeting and the real
entity data accessor. Its old assertion fails at `Some(List([]))` versus None after
a successful removal; earlier live membership and identity/Health/flags/raw-field
controls pass. It also repeats writes/reads and tests explicit-empty admission and
retained nonempty state. No world ticks or global clock setter.

The resource-free `empty_saved_tags_are_omitted_without_changing_raw_or_nonempty_tags`
uses the shared real save builder, including rawNone, valid nonempty tags,
raw empty Tags, unchanged input raw state, PortalCooldown and Invulnerable controls.
Its old failure is also `Some(List([]))` versus None.

Programs, references, full raw messages/styles/callbacks and bodies, transcripts,
old executables, source hashes and pinned bytecode are retained in the owner's
`.local/h328`; no prepared game files are published. Filesystem capture timestamps
are retained; unobserved stage times remain unknown. A test-only command launch
was stopped before its Game stage to add isolated home/config environment; its
completed resource-free failure remains retained, with no Game prerequisite claim
from the stopped launch. The actual Game unit ran in its isolated test environment.

Runtime coverage is the two pigs through the shared Base builder; no Player/Guest
or whole-class save census is claimed. H328 does not close H252. Pending H267
selector behavior and H134 frame Facing are separate, untouched production paths.

## Fixed result and all full-diagnostic residuals

The identical scoped 185 script and fresh Java reference bytes give **185/185**
fixed, compared with **167/185** old. Both units fail at their intended old
assertions and pass fixed. The full 194 program is byte-identical across builds:
**167/194 old**, **185/194 fixed**. No full 194 or whole-body equality claim.

The independent full Java references differ only at their absolute clocks:
309 old/317 fixed versus native 0 in both. Their non-clock records are identical.
Actual relative gametime remains 0, independently strict in the scoped gate.
All nine remaining full-record differences are below; complete bodies, styled
feedback and callbacks remain raw in the captures. No diagnostic rows are
normalized into matches.

| Exact label | Java expected | Fixed native actual |
|---|---|---|
| `clock-start` | `309 old / 317 fixed` | `0` |
| `initial-full-tag` | `attributes: []` | `attributes: [{id: "minecraft:movement_speed", base: 0.25d}]` |
| `added-full-tag` | `attributes: []` | `attributes: [{id: "minecraft:movement_speed", base: 0.25d}]` |
| `removed-full-tag` | `attributes: []` | `attributes: [{id: "minecraft:movement_speed", base: 0.25d}]` |
| `readded-full-tag` | `attributes: []` | `attributes: [{id: "minecraft:movement_speed", base: 0.25d}]` |
| `removed-again-full-tag` | `attributes: []` | `attributes: [{id: "minecraft:movement_speed", base: 0.25d}]` |
| `explicit-empty-load-full-tag` | `attributes: []` | `attributes: [{id: "minecraft:movement_speed", base: 0.25d}]` |
| `retained-full-tag` | `attributes: []` | `attributes: [{id: "minecraft:movement_speed", base: 0.25d}]` |
| `clock-end` | `309 old / 317 fixed` | `0` |

The extra default attribute is an independent saved-body diagnostic, not a Tags
repair target. Old removed/removed-again/explicit-empty-load full bodies also
contained Tags:[]; their missing-field state is now covered by the strict saved
Tags, callback and presence controls. No attribute/formatter change was made.

Fixed full Java reference SHA256: `1e9a8bc384c1f32f41f73a9c0ddeac17f219105b3cce9ef42da30d23bf518137`.

## Local readiness and reviewed checkpoints

Required `tools/pr_ready.py --quick --base 4f4a931b21d37bc64ce77081bb0a674248f71ad7`
ran once on the final production/test patch. Formatting, workspace/headless clippy,
public tree and doc tests pass. Combined Rust suites: **2909 passed, 1 failed**.
The sole failure is unchanged `entities::drop_motion_tests::drops_match_vanillas_motion_bit_for_bit`:
Z bits 4597448471913462162 versus 4597448471913462163 at yaw 37.5/pitch -21.25, reproduced
in this task's archived old unit executable. `entities.rs` is unchanged. The new
pure save unit passes; the ignored actual Game resource test was run separately
and passes with prepared resources and isolated home/config/data directories.

Python: **690 run, 4 failures, 1 skipped**, in existing ARM64/Bash 3.2 release
packaging tests. A narrow unchanged install-signature run confirms the mock's
missing `horizon-macos-arm64.zip`; the other failures are unset optional archive
arrays. No packaging or drop-motion fix. Broad parity was skipped by --quick;
the actual Game/Java paired fixture was explicitly run. Ledger check reports
only the two touched Rust files, with no newly reported gaps.

Actual main was pulled before/after batches through
`6d99e14ff0c70ae60bc56b67829b526409fa068e`. Later changes are UI/keybinding and
client ray/remote-mining work. The Scoreboard change is player-list key handling;
its tag membership/filter paths are unchanged. Shared entity saves, tag/data
commands, the command harness and pinned oracle remain unchanged. These source
reviews do not constitute a combined latest-main runtime claim: the pair remains
qualified to the frozen 4f4 base. Only evidence text was appended after required
checks; public-tree and whitespace checks were repeated on the final staged tree.
