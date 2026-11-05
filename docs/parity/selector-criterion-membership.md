# Advancement selector criterion membership (H266)

A missing criterion is rejected before comparing its requested boolean with progress.
The guard uses the selected advancement's criteria, including when the requested value
is false. It leaves Done predicates, empty criterion maps, missing advancement IDs,
progress mutations and target routing unchanged.

Official pinned Java 26.3 `EntitySelectorOptions.lambda$bootStrap$51` calls
`AdvancementProgress.getCriterion` and returns false for null before the boolean test
(bytecode offsets 41–48). The port ledger and the actual bytecode were reviewed.
Related #10193 concerns zero-criteria completion; pending #10130 concerns Guest progress
routing. Neither production change is imported here. Runtime coverage here is the actual
owner player; this does not establish connected-Guest parity.

## Frozen paired program

Base: `1fdf875085e6713ddcea06b1124164c4ef16355d`. The subsequent main checkpoint
`c58f49828` changes only launcher files, not this route. Both native binaries use the
same prepared resources and identical programs/fresh Java references. No tolerance,
message normalization or value masks are used.

`tools/selector_criterion_corpus.py` uses the existing actual Game command driver and
fresh Java `VanillaCommandExecOracle`. There is no network/progress observer adapter.
The actual owner is Player, UUID `a01e3843-e521-3998-958a-f459800e4d11`; UUID, type,
Health, mode, Pos, empty inventory, loaded chunk and floor are positively observed.
A separately guarded live NoAI/NoGravity pig supplies the nonplayer control.

The actual shared `minecraft:story/root` definition has only `crafting_table`; the
prepared definition and official jar definition are checked independently. Actual grant,
repeat/no-op grant and revoke receipts establish progress before each selector phase.
Before grant, after grant and after revoke, both if/unless run a scoreboard action with
fresh result/success/branch sentinels. All messages, styles and callback records are
compared. Controls cover known true/false, unknown true/false, Done true/false, known
empty maps, and missing advancement false/true/empty maps. Repeated reads do not mutate
progress. There are zero observation world ticks, witnessed by exact relative deltas.

The canonical **444** program was declared before capture. Its six setup/absolute-clock
diagnostics still execute and are traced, and are additionally recorded in the full
**450** program: `initial-clear`, `initial-revoke`, `clock-start`,
`before-clock-raw`, `granted-clock-raw`, `revoked-clock-raw`. Initial clear/revoke have
unknown initial progress and are not the later independent progress admissions. Raw
absolute clock values remain in the full evidence; unequal startup values are not a
selector defect. No full raw clock equality is claimed.

| Artifact | SHA-256 |
|---|---|
| Canonical script (444 records) | `17500deff3f805f71b41492b23a39e6b2a5fb867dd1adbc439b6d711aa9e3d2c` |
| Full script (450 records) | `44a9e38c2b7ef029d1ce6c69ae990a0af29547ca0b14bef6832cb741a9cbdcba` |
| Fresh canonical Java records | `f342351b37f5ad8ead73fb5a1b4f52d84aea24939c8034abc1cd86fa15cbde43` |
| Fresh full Java records | `5475b9fb2d27c829046ba6cc5b4647512ae88ead1ea83e36d98bd6e0ef8caeef` |

The resource-backed actual `Game::select` regression fails old at the unknown-false
assertion after its known-false admission passes. It also tests a criterion name present
only in another actual advancement (`get_stone`), all three progress phases, empty maps,
Done predicates and missing IDs. It uses the real definition/progress APIs, not a copied
selector model. It is ignored by default because it needs prepared resources.

Reproduce (with isolated test homes and the repository's prepared resources):

```sh
python3 tools/test_selector_criterion_parity.py --binary /path/to/horizon-client --work-dir .local/h266/replay
python3 tools/test_selector_criterion_parity.py --full-diagnostic --binary /path/to/horizon-client --work-dir .local/h266/full-replay
HORIZON_TEST_RESOURCES=/absolute/resources/26.3 cargo test -p horizon-client --lib selectors_reject_criteria_absent_from_the_selected_definition -- --ignored --nocapture
```

Raw commands, Java/native records, driver reports, full expected/actual JSON, logs,
bytecode, declarations, hashes and immutable old/fixed executables remain locally under
`.local/h266/`; prepared resources and Mojang files are not committed.

## Results and complete residuals

| Program | Untouched Game | Fixed Game |
|---|---:|---:|
| Canonical complete raw records | 420/444 | **444/444** |
| Full diagnostic, same frozen full reference | 422/450 | **446/450** |

All setup/progress/control admissions pass in both canonical runs. Their independently
regenerated fresh Java records are byte-identical to the frozen canonical reference.
The 24 old selector differences are six wrong unknown-false if/unless actions across
three phases, plus their three stored observations each; they are one lookup defect,
not 24 independent defects. Every one is fixed. Full diagnostics retain exactly four
raw absolute-clock differences, with Java 313 and native 0. Relative deltas remain
strictly zero. Fresh full-run Java startup clocks are also preserved separately; no
full-450 equality or byte-identical startup-time claim is made.

The following tables contain the complete expected/actual JSON records for every
residual against the frozen full reference, including message styles and callbacks.

### old-full: 28 differences

| Label | Expected Java | Actual Game |
|---|---|---|
| `clock-start` | `{"cmd":"/execute store result score start h266_clock run time query gametime","messages":[[["The game time is 313 tick(s)",[null,false,false,false,false,false]]]],"results":[[true,313]]}` | `{"cmd":"/execute store result score start h266_clock run time query gametime","messages":[[["The game time is 0 tick(s)",[null,false,false,false,false,false]]]],"results":[[true,0]]}` |
| `before-clock-raw` | `{"cmd":"/execute store result score now h266_clock run time query gametime","messages":[[["The game time is 313 tick(s)",[null,false,false,false,false,false]]]],"results":[[true,313]]}` | `{"cmd":"/execute store result score now h266_clock run time query gametime","messages":[[["The game time is 0 tick(s)",[null,false,false,false,false,false]]]],"results":[[true,0]]}` |
| `before-unknown_false-if-action` | `{"cmd":"/execute store result score result h266 store success score success h266 if entity @s[advancements={minecraft:story/root={h266_missing=false}}] run scoreboard players set branch h266 1","messages":[],"results":[]}` | `{"cmd":"/execute store result score result h266 store success score success h266 if entity @s[advancements={minecraft:story/root={h266_missing=false}}] run scoreboard players set branch h266 1","messages":[[["Set [h266] for branch to 1",[null,false,false,false,false,false]]]],"results":[[true,1]]}` |
| `before-unknown_false-if-result` | `{"cmd":"/scoreboard players get result h266","messages":[[["result has -99 [h266]",[null,false,false,false,false,false]]]],"results":[[true,-99]]}` | `{"cmd":"/scoreboard players get result h266","messages":[[["result has 1 [h266]",[null,false,false,false,false,false]]]],"results":[[true,1]]}` |
| `before-unknown_false-if-success` | `{"cmd":"/scoreboard players get success h266","messages":[[["success has -99 [h266]",[null,false,false,false,false,false]]]],"results":[[true,-99]]}` | `{"cmd":"/scoreboard players get success h266","messages":[[["success has 1 [h266]",[null,false,false,false,false,false]]]],"results":[[true,1]]}` |
| `before-unknown_false-if-branch` | `{"cmd":"/scoreboard players get branch h266","messages":[[["branch has -99 [h266]",[null,false,false,false,false,false]]]],"results":[[true,-99]]}` | `{"cmd":"/scoreboard players get branch h266","messages":[[["branch has 1 [h266]",[null,false,false,false,false,false]]]],"results":[[true,1]]}` |
| `before-unknown_false-unless-action` | `{"cmd":"/execute store result score result h266 store success score success h266 unless entity @s[advancements={minecraft:story/root={h266_missing=false}}] run scoreboard players set branch h266 1","messages":[[["Set [h266] for branch to 1",[null,false,false,false,false,false]]]],"results":[[true,1]]}` | `{"cmd":"/execute store result score result h266 store success score success h266 unless entity @s[advancements={minecraft:story/root={h266_missing=false}}] run scoreboard players set branch h266 1","messages":[],"results":[]}` |
| `before-unknown_false-unless-result` | `{"cmd":"/scoreboard players get result h266","messages":[[["result has 1 [h266]",[null,false,false,false,false,false]]]],"results":[[true,1]]}` | `{"cmd":"/scoreboard players get result h266","messages":[[["result has -99 [h266]",[null,false,false,false,false,false]]]],"results":[[true,-99]]}` |
| `before-unknown_false-unless-success` | `{"cmd":"/scoreboard players get success h266","messages":[[["success has 1 [h266]",[null,false,false,false,false,false]]]],"results":[[true,1]]}` | `{"cmd":"/scoreboard players get success h266","messages":[[["success has -99 [h266]",[null,false,false,false,false,false]]]],"results":[[true,-99]]}` |
| `before-unknown_false-unless-branch` | `{"cmd":"/scoreboard players get branch h266","messages":[[["branch has 1 [h266]",[null,false,false,false,false,false]]]],"results":[[true,1]]}` | `{"cmd":"/scoreboard players get branch h266","messages":[[["branch has -99 [h266]",[null,false,false,false,false,false]]]],"results":[[true,-99]]}` |
| `granted-clock-raw` | `{"cmd":"/execute store result score now h266_clock run time query gametime","messages":[[["The game time is 313 tick(s)",[null,false,false,false,false,false]]]],"results":[[true,313]]}` | `{"cmd":"/execute store result score now h266_clock run time query gametime","messages":[[["The game time is 0 tick(s)",[null,false,false,false,false,false]]]],"results":[[true,0]]}` |
| `granted-unknown_false-if-action` | `{"cmd":"/execute store result score result h266 store success score success h266 if entity @s[advancements={minecraft:story/root={h266_missing=false}}] run scoreboard players set branch h266 1","messages":[],"results":[]}` | `{"cmd":"/execute store result score result h266 store success score success h266 if entity @s[advancements={minecraft:story/root={h266_missing=false}}] run scoreboard players set branch h266 1","messages":[[["Set [h266] for branch to 1",[null,false,false,false,false,false]]]],"results":[[true,1]]}` |
| `granted-unknown_false-if-result` | `{"cmd":"/scoreboard players get result h266","messages":[[["result has -99 [h266]",[null,false,false,false,false,false]]]],"results":[[true,-99]]}` | `{"cmd":"/scoreboard players get result h266","messages":[[["result has 1 [h266]",[null,false,false,false,false,false]]]],"results":[[true,1]]}` |
| `granted-unknown_false-if-success` | `{"cmd":"/scoreboard players get success h266","messages":[[["success has -99 [h266]",[null,false,false,false,false,false]]]],"results":[[true,-99]]}` | `{"cmd":"/scoreboard players get success h266","messages":[[["success has 1 [h266]",[null,false,false,false,false,false]]]],"results":[[true,1]]}` |
| `granted-unknown_false-if-branch` | `{"cmd":"/scoreboard players get branch h266","messages":[[["branch has -99 [h266]",[null,false,false,false,false,false]]]],"results":[[true,-99]]}` | `{"cmd":"/scoreboard players get branch h266","messages":[[["branch has 1 [h266]",[null,false,false,false,false,false]]]],"results":[[true,1]]}` |
| `granted-unknown_false-unless-action` | `{"cmd":"/execute store result score result h266 store success score success h266 unless entity @s[advancements={minecraft:story/root={h266_missing=false}}] run scoreboard players set branch h266 1","messages":[[["Set [h266] for branch to 1",[null,false,false,false,false,false]]]],"results":[[true,1]]}` | `{"cmd":"/execute store result score result h266 store success score success h266 unless entity @s[advancements={minecraft:story/root={h266_missing=false}}] run scoreboard players set branch h266 1","messages":[],"results":[]}` |
| `granted-unknown_false-unless-result` | `{"cmd":"/scoreboard players get result h266","messages":[[["result has 1 [h266]",[null,false,false,false,false,false]]]],"results":[[true,1]]}` | `{"cmd":"/scoreboard players get result h266","messages":[[["result has -99 [h266]",[null,false,false,false,false,false]]]],"results":[[true,-99]]}` |
| `granted-unknown_false-unless-success` | `{"cmd":"/scoreboard players get success h266","messages":[[["success has 1 [h266]",[null,false,false,false,false,false]]]],"results":[[true,1]]}` | `{"cmd":"/scoreboard players get success h266","messages":[[["success has -99 [h266]",[null,false,false,false,false,false]]]],"results":[[true,-99]]}` |
| `granted-unknown_false-unless-branch` | `{"cmd":"/scoreboard players get branch h266","messages":[[["branch has 1 [h266]",[null,false,false,false,false,false]]]],"results":[[true,1]]}` | `{"cmd":"/scoreboard players get branch h266","messages":[[["branch has -99 [h266]",[null,false,false,false,false,false]]]],"results":[[true,-99]]}` |
| `revoked-clock-raw` | `{"cmd":"/execute store result score now h266_clock run time query gametime","messages":[[["The game time is 313 tick(s)",[null,false,false,false,false,false]]]],"results":[[true,313]]}` | `{"cmd":"/execute store result score now h266_clock run time query gametime","messages":[[["The game time is 0 tick(s)",[null,false,false,false,false,false]]]],"results":[[true,0]]}` |
| `revoked-unknown_false-if-action` | `{"cmd":"/execute store result score result h266 store success score success h266 if entity @s[advancements={minecraft:story/root={h266_missing=false}}] run scoreboard players set branch h266 1","messages":[],"results":[]}` | `{"cmd":"/execute store result score result h266 store success score success h266 if entity @s[advancements={minecraft:story/root={h266_missing=false}}] run scoreboard players set branch h266 1","messages":[[["Set [h266] for branch to 1",[null,false,false,false,false,false]]]],"results":[[true,1]]}` |
| `revoked-unknown_false-if-result` | `{"cmd":"/scoreboard players get result h266","messages":[[["result has -99 [h266]",[null,false,false,false,false,false]]]],"results":[[true,-99]]}` | `{"cmd":"/scoreboard players get result h266","messages":[[["result has 1 [h266]",[null,false,false,false,false,false]]]],"results":[[true,1]]}` |
| `revoked-unknown_false-if-success` | `{"cmd":"/scoreboard players get success h266","messages":[[["success has -99 [h266]",[null,false,false,false,false,false]]]],"results":[[true,-99]]}` | `{"cmd":"/scoreboard players get success h266","messages":[[["success has 1 [h266]",[null,false,false,false,false,false]]]],"results":[[true,1]]}` |
| `revoked-unknown_false-if-branch` | `{"cmd":"/scoreboard players get branch h266","messages":[[["branch has -99 [h266]",[null,false,false,false,false,false]]]],"results":[[true,-99]]}` | `{"cmd":"/scoreboard players get branch h266","messages":[[["branch has 1 [h266]",[null,false,false,false,false,false]]]],"results":[[true,1]]}` |
| `revoked-unknown_false-unless-action` | `{"cmd":"/execute store result score result h266 store success score success h266 unless entity @s[advancements={minecraft:story/root={h266_missing=false}}] run scoreboard players set branch h266 1","messages":[[["Set [h266] for branch to 1",[null,false,false,false,false,false]]]],"results":[[true,1]]}` | `{"cmd":"/execute store result score result h266 store success score success h266 unless entity @s[advancements={minecraft:story/root={h266_missing=false}}] run scoreboard players set branch h266 1","messages":[],"results":[]}` |
| `revoked-unknown_false-unless-result` | `{"cmd":"/scoreboard players get result h266","messages":[[["result has 1 [h266]",[null,false,false,false,false,false]]]],"results":[[true,1]]}` | `{"cmd":"/scoreboard players get result h266","messages":[[["result has -99 [h266]",[null,false,false,false,false,false]]]],"results":[[true,-99]]}` |
| `revoked-unknown_false-unless-success` | `{"cmd":"/scoreboard players get success h266","messages":[[["success has 1 [h266]",[null,false,false,false,false,false]]]],"results":[[true,1]]}` | `{"cmd":"/scoreboard players get success h266","messages":[[["success has -99 [h266]",[null,false,false,false,false,false]]]],"results":[[true,-99]]}` |
| `revoked-unknown_false-unless-branch` | `{"cmd":"/scoreboard players get branch h266","messages":[[["branch has 1 [h266]",[null,false,false,false,false,false]]]],"results":[[true,1]]}` | `{"cmd":"/scoreboard players get branch h266","messages":[[["branch has -99 [h266]",[null,false,false,false,false,false]]]],"results":[[true,-99]]}` |

### fixed-full: 4 differences

| Label | Expected Java | Actual Game |
|---|---|---|
| `clock-start` | `{"cmd":"/execute store result score start h266_clock run time query gametime","messages":[[["The game time is 313 tick(s)",[null,false,false,false,false,false]]]],"results":[[true,313]]}` | `{"cmd":"/execute store result score start h266_clock run time query gametime","messages":[[["The game time is 0 tick(s)",[null,false,false,false,false,false]]]],"results":[[true,0]]}` |
| `before-clock-raw` | `{"cmd":"/execute store result score now h266_clock run time query gametime","messages":[[["The game time is 313 tick(s)",[null,false,false,false,false,false]]]],"results":[[true,313]]}` | `{"cmd":"/execute store result score now h266_clock run time query gametime","messages":[[["The game time is 0 tick(s)",[null,false,false,false,false,false]]]],"results":[[true,0]]}` |
| `granted-clock-raw` | `{"cmd":"/execute store result score now h266_clock run time query gametime","messages":[[["The game time is 313 tick(s)",[null,false,false,false,false,false]]]],"results":[[true,313]]}` | `{"cmd":"/execute store result score now h266_clock run time query gametime","messages":[[["The game time is 0 tick(s)",[null,false,false,false,false,false]]]],"results":[[true,0]]}` |
| `revoked-clock-raw` | `{"cmd":"/execute store result score now h266_clock run time query gametime","messages":[[["The game time is 313 tick(s)",[null,false,false,false,false,false]]]],"results":[[true,313]]}` | `{"cmd":"/execute store result score now h266_clock run time query gametime","messages":[[["The game time is 0 tick(s)",[null,false,false,false,false,false]]]],"results":[[true,0]]}` |

## Local readiness

The unchanged actual Game regression passes fixed (one resource-backed test), after
failing old at the unknown-false assertion. Required `pr_ready.py --quick` ran once on
the final production/test patch: formatting, workspace and headless clippy, 1,015 Rust
tests, doc tests and public-tree checks pass. Python runs 686 tests with four existing
macOS packaging failures and one skip: the signature mock supplies no ARM64 archive,
and optional-archive masks 0/1/2 hit unset Bash 3.2 arrays. These files are unchanged.
The affected parity suite was not broadly rerun by `--quick`; the focused actual
Game/Java programs above were run explicitly. Evidence-only documentation was finalized
after the check, followed by public-tree and whitespace checks. Final main fetch remains
`c58f49828818a0e3fd12c9d1d6159094e53d4523`, with only unrelated launcher changes.
