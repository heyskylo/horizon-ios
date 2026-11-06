# NoAI Dolphin post-superclass air reset

Pinned 26.3 `Dolphin.tick` calls the superclass tick, then checks NoAI. A NoAI
Dolphin sets air to `getMaxAirSupply()` and returns; that accessor returns 4800.
The fix adds exactly that reset after the shared full-Mob tick work, using the
existing `Mob::max_air_supply`. It does not call species AI or change normal
Dolphin countdown, other species, generic breathing hooks or damage policy.

The port ledger and pinned bytecode were read independently. Closed PR9459
contained a proposed NoAI reset, but merged successor PR9658 omitted it while
fixing other Dolphin air/tag behavior. No active duplicate owner was found.

## Frozen actual Game/Java fixture

`tools/test_noai_dolphin_air_parity.py` uses the existing command harness, a
real Game and a fresh pinned Java server. The source base is
`a589a3dd9be04fa7f17667c0175711aaa92e1147`.

The unchanged executable is our preserved H180 baseline on exactly that same
current base, verified against its SHA256/provenance and original build log. Its
H180 unit-only draft was compiled out of the Game executable; no H180 production
change is imported. H072 supplies its own new fixture and fresh Java reference.
The fixed executable is built from this H072 worktree on the same frozen base.

Five actual UUID-bound, persistent, Health 10 Dolphins cover:

| Subject | NoAI | Medium | Air at 0/1/2 ticks |
|---|---|---|---|
| air_full | true | air | 4800/4800/4800 |
| air_low | true | air | 100/4800/4800 |
| water_full | true | source water | 4800/4800/4800 |
| water_low | true | source water | 100/4800/4800 |
| normal_water | false | source water | 4800/4799/4798 |

Fresh Java independently validates these expectations before the pair. The
loaded floor, glass pool, source-water/air samples at body and eye levels,
actual creative owner, actor UUID/type/tag/Health/NoAI/NoGravity/Pos, fresh
result/success sentinels and exactly two actual world ticks are guarded.
NoGravity and a real zero movement-speed attribute isolate stationary actors;
normal AI still runs. No observer or fabricated air state is introduced.

The predeclared 182-record canonical program compares complete raw feedback/styles,
callbacks, saved Short air values, result/success stores, typed NBT predicates,
positions and relative clocks. All 15 full actor bodies and two absolute startup
clock replies remain trace diagnostics, also executable in the 199-record full
program. These categories were declared before any fresh captures. No tolerance,
row mutation or comparison mask is used.

Program SHA256:

- canonical: `24630b6345aa82e2dd9c138d9e46bc002b4ef9cfc0f2785a40384f02aedf5e00`
- full: `0dd0ffca384f7dbe56a89d5730c2fe890c12849cd4f6ae5d8369ebb8af2bd571`

All raw replies, complete bodies, references, saved worlds, scripts, executable
hashes and manifests remain locally under `.local/h072`; no coordinator payload
or game resource is committed. Unknown phase times stay null.

## Actual full-Mob tick regression

`cargo test -p horizon-client-lib --lib dolphin_noai_air_tests` uses the existing
prepared-resource world factory with actual water or air and calls the real full
`mobs::ai::tick`. It checks both media, loaded 4800/100, actual tick age, eye-water
state, unchanged Health and NoAI, plus the normal water countdown. Before the fix,
the NoAI test fails at 104 versus 4800 and the normal-AI control passes. This is
not a copied air model or a direct call to the reset. Prepared resources are
required for this test to execute; absence is reported, not counted as runtime
coverage. The own local old failure proves the real fixture ran.

The initial unit draft had a u64 tick-age assertion, corrected to the actual u32
model type before compilation reached it; that draft is retained, with no runtime
claim. The gameplay change does not touch saved Air conversion, normal refill,
Nautilus, water-breathing effects, goals, particles, sounds, RNG or lifecycle.

```sh
python3 tools/test_noai_dolphin_air_parity.py --binary target/fast/horizon-client --work-dir .local/noai-dolphin-air
python3 tools/test_noai_dolphin_air_parity.py --binary target/fast/horizon-client --work-dir .local/noai-dolphin-air-full --full-diagnostic
```

## Paired results and complete diagnostic residuals

- Canonical: unchanged 164/182 → fixed 182/182. All 18 original differences are six NoAI air values independently checked by raw Short reply, stored result and typed predicate.
- Full diagnostic: unchanged 165/199 → fixed 183/199. No 199/199 claim.

Every remaining full-program difference against the immutable common reference:

| Label | Expected Java | Actual Game |
|---|---|---|
| clock-start | `[[True, 313]]` | `[[True, 0]]` |
| air_full-0-body | `NoAI before GotFish` | `GotFish before NoAI` |
| air_low-0-body | `NoAI before GotFish` | `GotFish before NoAI` |
| water_full-0-body | `NoAI before GotFish` | `GotFish before NoAI` |
| water_low-0-body | `NoAI before GotFish` | `GotFish before NoAI` |
| air_full-1-body | `NoAI before GotFish` | `GotFish before NoAI` |
| air_low-1-body | `NoAI before GotFish` | `GotFish before NoAI` |
| water_full-1-body | `NoAI before GotFish; oxygen_bonus base0.0d` | `GotFish before NoAI; oxygen_bonus absent` |
| water_low-1-body | `NoAI before GotFish; oxygen_bonus base0.0d` | `GotFish before NoAI; oxygen_bonus absent` |
| normal_water-1-body | `Rotation: [350.0f, 5.0f]; oxygen_bonus base0.0d` | `Rotation: [0.0f, 0.0f]; oxygen_bonus absent` |
| air_full-2-body | `NoAI before GotFish` | `GotFish before NoAI` |
| air_low-2-body | `NoAI before GotFish` | `GotFish before NoAI` |
| water_full-2-body | `NoAI before GotFish; oxygen_bonus base0.0d` | `GotFish before NoAI; oxygen_bonus absent` |
| water_low-2-body | `NoAI before GotFish; oxygen_bonus base0.0d` | `GotFish before NoAI; oxygen_bonus absent` |
| normal_water-2-body | `Rotation: [340.0f, 10.0f]; oxygen_bonus base0.0d` | `Rotation: [0.0f, 0.0f]; oxygen_bonus absent` |
| clock-end | `[[True, 315]]` | `[[True, 2]]` |

All twelve NoAI-body rows retain existing compound presentation ordering
(H191). Four of those water rows also retain the pre-existing zero-valued
`oxygen_bonus` attribute-materialization difference. The two normal-body rows
retain that attribute difference and the existing rotation differences. No
attribute materialization, formatter or normal Dolphin logic is changed. Every body
callback remains `[true,1]`; full text, styles, coordinates and callbacks remain
raw and executable. Raw absolute clocks are not replaced or normalized, while
both observed relative deltas are exactly two ticks.

## Required checks and limits

`tools/pr_ready.py --quick --base a589a3dd9be04fa7f17667c0175711aaa92e1147`
ran once on the final Rust and harness files:1962 Rust passes,63 ignored and one
pre-existing `entities::drop_motion_tests::drops_match_vanillas_motion_bit_for_bit`
failure. At yaw37.5/pitch-21.25, Z bits are4597448471913462162 versus expected
4597448471913462163. Our own untouched-production a171 executable already
reproduced that exact failure; entities.rs and jmath are unchanged from that
baseline. Its original log/provenance is retained, not a new H072 baseline run.

Python reports696 tests, four failures and one skip: the unchanged installer
signature fixture lacks `horizon-macos-arm64.zip`, and three optional-archive
cases fail under Bash3.2 with `musl[@]`/`arm64[@]: unbound variable`. The prior
same-base narrow installer diagnostic is retained. No installer or drop-motion
repair is included, and the overall self-check is not green.

Formatting, Clippy with warnings denied, doctests and public-tree checks pass;
Cargo.lock is unchanged. The ledger reports no new gaps, with a long-uncited
advisory on the test fixture helper (it is not itself a vanilla port). The assigned
actual Game/Java proof was run separately from quick mode's broad-harness skip.
Only evidence notes were added after checks; Rust and harness files are unchanged.
The latest source checkpoint has no relevant delta from the frozen base.

Disk stayed above the5GiB hard guard with planned combined-link headroom,
jobs2/incremental0 and one heavy build. Only verified inactive OWN artifacts were
archived; proofs and other owners' artifacts remain preserved. This does not
establish drowning, all air policies, effect behavior, particles, packets,
rendering, lifecycle or all-species parity.

## Publication source refresh

Main advanced after the paired proof and required checks to
`29825eb7747028cad8363b72d023fe8bf69b672a`. The own branch was refreshed without
an upstream merge. The conflict was only the shared append location of the new
flight-effect test module and this Dolphin test module; both complete modules
are preserved. The four-line Dolphin patch is unchanged.

Source review retains the new fall-distance reset (no active effects in this
fixture), merged age-aware HappyGhast voices, conversion tags, model-layer and
isolated-orb-fixture changes. None changes Dolphin air or this fixture's selected
commands/inputs. The original Game pair and full required-check report remain
qualified to a589. Affected formatting, real full-Mob tick tests and scoped Clippy
are checked again on29825; this is not a new latest-base Game pair or a claim that
all latest-base tests were rerun.
