# Empty tag selector truth value (H267)

`tag=` selects entities with no tags; `tag=!` selects entities with at least one tag.
The fix changes only the empty-name branch from `!tags.is_empty()` to
`tags.is_empty()` before the existing negation comparison. Named tags, normalization,
the tag cap, entity ordering and other selector predicates are unchanged.

Pinned Java26.3 `EntitySelectorOptions.lambda$bootStrap$43` calls
`Entity.entityTags().isEmpty()` for the empty name (bytecode10–20), then applies
negation. The port ledger and actual bytecode were reviewed. Current/closed issue/PR
searches found no exact competing fix. Merged #6468 preserves tags on more entity kinds;
#6935 changes candidate traversal. Neither fixes this branch.

## Actual Game/Java fixture

Frozen base `f00c4c6392ef444130692da4db99f0a7268dd20b`. No pending #10130 (H264)
or #10219 (H266) production is imported. Two actual NoAI/NoGravity pigs have fixed UUIDs
`0000010b-0000-0000-0000-000000000001` and
`0000010b-0000-0000-0000-000000000002`, Health10 and initial positions
[4.5,100,4.5] and [8.5,100,4.5]. Identity never depends on a tag or entity order.
Initially A has no tags and B has only audit. Actual `/tag UUID add/remove audit` commands
swap their tag states and restore them on those same actors.

Each phase observes actual UUID/type/Health/Pos/NoAI/NoGravity, live `/tag UUID list`,
loaded chunk/floor and the real owner body/mode. For each actor, tag=, tag=!, tag=audit,
and tag=!audit are tested with both if/unless scoreboard actions. Every call resets
result/success/branch independently to-99, then reads all three. True actions and false
no-callback behavior are independently validated by fresh Java. There are zero observation
world ticks, witnessed by exact relative-clock deltas. No new observer or generic
infrastructure is added: this uses the existing actual Game/Java command harness.

The strict **421-record** program was declared before its fresh Java and old/fixed runs.
All recorded JSON is compared in full, including message styles and callbacks, without
normalization, tolerance or value masks. Six saved Tags getters, six full body reads and
four absolute clock reads still execute and are traced; they are additionally recorded
in the executable **437-record** full diagnostic. These 16 declared diagnostic labels are:
clock-start; initial/swapped/restored-clock-raw; initial/swapped/restored-a/b-Tags;
initial/swapped/restored-a/b-body. No full437 equality is claimed.

An earlier **421-record** draft used saved `Tags` getters as live-state admissions.
Its old capture was rejected as a prerequisite: after removing the last tag, Java omits
Tags while native returns Tags:[]. It is not a qualified H267 count. The revised program
uses independent real `/tag list` receipts, retaining every saved Tags read in the full
diagnostic. The original421 script/reference/native capture and original431 Java diagnostic, plus
runnable Python modules, remain unchanged under `.local/h267/original421` and the
original capture directories. No fixed-pass claim is made for that rejected program.
No serializer or tag-normalization repair is included.

Replay:

```sh
python3 tools/test_selector_empty_tag_parity.py --binary /path/to/game --work-dir .local/h267/replay
python3 tools/test_selector_empty_tag_parity.py --full-diagnostic --binary /path/to/game --work-dir .local/h267/full-replay
HORIZON_TEST_RESOURCES=/absolute/resources/26.3 cargo test -p horizon-client --lib empty_tag_selectors_follow_actual_tag_edits -- --ignored --nocapture
```

Resource-backed units require isolated fresh test homes/cwd, as in AGENTS.md. Raw commands,
all reference/native JSON, complete expected/actual differences, logs, bytecode, declarations,
hashes and immutable executables remain locally under `.local/h267/`. No Mojang files or
prepared resources are committed.

## Declared hashes

| Artifact | SHA-256 |
|---|---|
| canonical program | `7aa4443f5a5ec8f805aadb6cd3f675f665f102aef7fd199a7269fc8f94fc00f3` |
| Fresh canonical Java records | `e29c68a719df77c7302e388f22b80cabfdbb0de475fda9b3cbd5d8a064e4b449` |
| full program | `5a4f4418f6da23a3ec3ea756e247c777a51939c9fa34b3a3179650be0763ce42` |
| Fresh full Java records | `72ae7a5daed2f426079efb518f519bc673bc3e46f9323f378f37ff4c5ba4712a` |

## Paired results and residuals

| Program | Untouched Game | Fixed Game |
|---|---:|---:|
| Strict complete records | 325/421 | **421/421** |
| Full diagnostic | 329/437 | **425/437** |

Both old/fixed programs are byte-identical for each mode. Their strict fresh Java records
are byte-identical to the frozen strict reference. All actual identity/live-tag/named-tag
and zero-clock admissions pass. The 96 old target differences are 24 reversed empty-form
actions plus their three stored observations each, not 96 independent defects.
The real Game unit fails old at tag= selecting B instead of A after UUID/tag/named-tag
admissions pass. It uses actual summon/tag commands and Game::select, and compares sets
to avoid relying on entity order.

All 12 full-diagnostic residuals persist unchanged by this fix. The table gives exact
field/text differences and callbacks; it is a readable report, not the comparator.
The comparator retains complete raw JSON, including all styled text fragments. Complete
expected/actual records for all old/fixed differences remain in strict-report.json.

| Label | Expected Java | Actual native | Reason outside H267 |
|---|---|---|---|
| `clock-start` | `The game time is 321 tick(s)`; callback `[true,321]` | `The game time is 0 tick(s)`; callback `[true,0]` | Absolute startup clocks; strict relative deltas are0 |
| `initial-clock-raw` | `The game time is 321 tick(s)`; callback `[true,321]` | `The game time is 0 tick(s)`; callback `[true,0]` | Absolute startup clocks; strict relative deltas are0 |
| `initial-a-body` | `attributes: []`; callback `[true,1]` | `attributes: [{id:"minecraft:movement_speed",base:0.25d}]`; callback `[true,1]` | Extra default movement-speed attribute serialization |
| `initial-b-body` | `attributes: []`; callback `[true,1]` | `attributes: [{id:"minecraft:movement_speed",base:0.25d}]`; callback `[true,1]` | Extra default movement-speed attribute serialization |
| `swapped-clock-raw` | `The game time is 321 tick(s)`; callback `[true,321]` | `The game time is 0 tick(s)`; callback `[true,0]` | Absolute startup clocks; strict relative deltas are0 |
| `swapped-a-body` | `attributes: []`; callback `[true,1]` | `attributes: [{id:"minecraft:movement_speed",base:0.25d}]`; callback `[true,1]` | Extra default movement-speed attribute serialization |
| `swapped-b-Tags` | `Found no elements matching Tags`; callback `[false,0]` | `Pig has the following entity data: []`; callback `[true,0]` | Saved empty-Tags representation after actual last-tag removal |
| `swapped-b-body` | `attributes: []`; callback `[true,1]` | `attributes: [{id:"minecraft:movement_speed",base:0.25d}]`; also `Tags:[]`; callback `[true,1]` | Extra default movement-speed attribute serialization and saved empty Tags |
| `restored-clock-raw` | `The game time is 321 tick(s)`; callback `[true,321]` | `The game time is 0 tick(s)`; callback `[true,0]` | Absolute startup clocks; strict relative deltas are0 |
| `restored-a-Tags` | `Found no elements matching Tags`; callback `[false,0]` | `Pig has the following entity data: []`; callback `[true,0]` | Saved empty-Tags representation after actual last-tag removal |
| `restored-a-body` | `attributes: []`; callback `[true,1]` | `attributes: [{id:"minecraft:movement_speed",base:0.25d}]`; also `Tags:[]`; callback `[true,1]` | Extra default movement-speed attribute serialization and saved empty Tags |
| `restored-b-body` | `attributes: []`; callback `[true,1]` | `attributes: [{id:"minecraft:movement_speed",base:0.25d}]`; callback `[true,1]` | Extra default movement-speed attribute serialization |

These whole-body differences are actual extra serialized fields, not merely insertion
order. No attribute/default codec, saved Tags or clock repair is included. Exact full
bodies/styles remain raw. No full437-green or all-entity-class runtime claim is made.

## Local readiness and source limits

The unchanged real Game regression passes fixed (one resource-backed test), after the
intended old failure. Required `pr_ready.py --quick` ran once on the final gameplay/test
patch: formatting, workspace and headless clippy, 1,015 Rust tests, doc tests and public
tree pass. Python ran686 tests with four existing macOS packaging failures and one skip:
the signature mock lacks the ARM64 archive, and optional-archive masks0/1/2 encounter
unset Bash3.2 arrays. Those files are unchanged. The focused actual Game/Java programs
were run explicitly; `--quick` does not claim the broad parity suite. Evidence-only docs
were finalized after that check, then public-tree and whitespace checks were repeated.

A final actual main pull reached `3d33b0299d5c80769300d8b33dafb389ac9e7656`.
Only launcher/cosmetics files changed after the frozen f00 base. Gameplay/selector/driver
sources are unchanged, so no repeated same-base proof or broader latest-main runtime
claim is made. H264/#10130 and H266/#10219 still need their independent routing/criterion
branches and all three regression modules preserved when combined.
