# NoAI Nautilus superclass air

Pinned 26.3 `Nautilus.baseTick` records the old air, calls the superclass, and
invokes its custom `handleAirSupply` only with AI enabled on the server. The
custom handler uses that pre-super air on land, and writes 300 in water. With
NoAI the inherited breathing still refills by four, capped at 300; air must not
be frozen or forced to the maximum. Bytecode and the port ledger were inspected
independently.

The fix excludes only `Species::Nautilus` under NoAI from the existing
`needs_water` branch. It reaches the unchanged generic Breath/drown body,
including the current native hooks. Other species and the normal custom-handler
body stay unchanged. The existing normal-handler drown/dry_out damage-kind gap
is retained in the ledger; this is not a whole Nautilus handler port.

## Frozen actual Game/Java proof

`tools/test_noai_nautilus_air_parity.py` uses the existing command harness,
actual Game and fresh pinned Java. Both binaries are built on frozen source
`87c51ab9c9368700223726f03f3d5689f145bb10` with identical declared scripts and
fresh reference bytes. No pending H072/H224 production is imported.

Eight actual UUID-bound, Health 10, persistent Nautilus cover NoAI 0/1 × land/water
× loaded Short 300/100. NoGravity and a real zero movement-speed attribute keep
actors stationary without disabling normal AI. The actual owner, typed actors,
Health/NoAI/Pos, loaded floor and medium, fresh result/success sentinels and
exact two world ticks are guarded. The prepared official type data gives eye
height 0.2751 and height 0.95: both body and eye at Y=201 lie inside the queried
201 cell, with an additional 202 source-cell control. The full unit verifies
actual eye-fluid state from its real world.

Fresh Java independently validates the following Air values at cumulative 0/1/2:

| AI | Medium | Initial 300 | Initial 100 |
|---|---|---|---|
| disabled | land | 300/300/300 | 100/104/108 |
| disabled | water | 300/300/300 | 100/104/108 |
| enabled | land | 300/299/298 | 100/99/98 |
| enabled | water | 300/300/300 | 100/300/300 |

The predeclared 278-record canonical contract retains complete raw air feedback,
styles, callback/store values, Short-typed predicates, all actor/mode/media/load
and relative-clock controls. All 24 complete actor bodies and two absolute startup
clock replies remain executable trace diagnostics in the 304-record full program.
These categories were declared before the first captures. No tolerance, record
mutation or comparison mask is used.

Program SHA256:

- canonical: `778193efd641e930748fe2a9411f41f9066fd6e1df1ed60a8a9af651f418fc42`
- full: `3d19326d3734898a3ce75742021f49c2f947b9f8ac70724ed80bb706e0303573`

All scripts, references, raw replies and bodies, worlds, bytecode, binary hashes,
source checkpoints and manifests are retained locally in `.local/h071`. Unknown
phase times stay null. No game files or coordinator payloads are committed.

## Reachable full-tick regression

`cargo test -p horizon-client-lib --lib nautilus_noai_air_tests` calls the real
full `mobs::ai::tick` in the existing prepared-resource world factory, with real
water or air. It guards the actual Nautilus flags, tick age, eye immersion,
Health and NoAI, and checks the independently measured air values. Old production
has one passing normal-handler test and one failing NoAI test (land 300→299,
expected 300). This is not a copied timer model or a direct reset-helper call.
Prepared data is required; its absence is reported, not counted as runtime
coverage. The own old failure establishes that this local fixture actually ran.

The normal custom-handler body, caps and damage cadence are unchanged. No
Zombie Nautilus, other aquatic species, damage-policy, effect, lifetime, packet,
rendering or general AI closure is claimed.

```sh
python3 tools/test_noai_nautilus_air_parity.py --binary target/fast/horizon-client --work-dir .local/noai-nautilus-air
python3 tools/test_noai_nautilus_air_parity.py --binary target/fast/horizon-client --work-dir .local/noai-nautilus-air-full --full-diagnostic
```

## Paired results and full residual table

| View | Unchanged | Fixed |
|---|---|---|
| Canonical | 260/278 | 278/278 |
| Full diagnostic | 260/304 | 278/304 |
| Actual full-tick tests | 1/2 | 2/2 |

No 304/304 claim. Every fixed full-program residual is listed below. CD denotes
`Brain.memories."minecraft:attack_target_cooldown".value` (Int), not air.

| Label | Expected Java | Actual Game |
|---|---|---|
| clock-start | `[[true, 301]]` | `[[true, 0]]` |
| air_full-0-body | `CD absent` | `CD 0` |
| air_low-0-body | `CD absent` | `CD 0` |
| water_full-0-body | `CD absent` | `CD 0` |
| water_low-0-body | `CD absent` | `CD 0` |
| normal_air_full-0-body | `CD absent` | `CD 0` |
| normal_air_low-0-body | `CD absent` | `CD 0` |
| normal_water_full-0-body | `CD absent` | `CD 0` |
| normal_water_low-0-body | `CD absent` | `CD 0` |
| air_full-1-body | `CD absent` | `CD 0` |
| air_low-1-body | `CD absent` | `CD 0` |
| water_full-1-body | `CD absent` | `CD 0` |
| water_low-1-body | `CD absent` | `CD 0` |
| normal_air_full-1-body | `CD absent` | `CD 0` |
| normal_air_low-1-body | `CD absent` | `CD 0` |
| normal_water_full-1-body | `CD 2816; Rotation: [0.0f, 0.0f]` | `CD 2466; Rotation: [350.0f, 0.0f]` |
| normal_water_low-1-body | `CD 2754; Rotation: [0.0f, 0.0f]` | `CD 3114; Rotation: [350.0f, 0.0f]` |
| air_full-2-body | `CD absent` | `CD 0` |
| air_low-2-body | `CD absent` | `CD 0` |
| water_full-2-body | `CD absent` | `CD 0` |
| water_low-2-body | `CD absent` | `CD 0` |
| normal_air_full-2-body | `CD absent` | `CD 0` |
| normal_air_low-2-body | `CD absent` | `CD 0` |
| normal_water_full-2-body | `CD 2815; Rotation: [0.0f, 0.0f]` | `CD 2465; Rotation: [340.0f, 0.12813261f]` |
| normal_water_low-2-body | `CD 2753; Rotation: [0.0f, 0.0f]` | `CD 3113; Rotation: [340.0f, 0.12813261f]` |
| clock-end | `[[true, 303]]` | `[[true, 2]]` |

All body callbacks remain `[true,1]`. Complete raw strings/styles and Brain
values are retained without modification. These pre-existing Brain projection
and normal-water pose/cooldown observations are outside H071, without a claim
about their full cause or random equality. Both relative clock deltas are
exactly two; raw absolute values remain intact. No Brain, pose or clock repair
is included.

## Required checks and scope limits

`tools/pr_ready.py --quick --base 87c51ab9c9368700223726f03f3d5689f145bb10`
ran once on the final Rust/harness patch:1980 Rust passes,65 ignored and one
pre-existing `entities::drop_motion_tests::drops_match_vanillas_motion_bit_for_bit`
failure. At yaw37.5/pitch-21.25, Z bits are4597448471913462162 versus expected
4597448471913462163. Our own untouched a171 executable already reproduces that
exact failure. The drop-motion function/test and jmath source are unchanged;
the later entities.rs delta is unrelated ArmorStand first-tick state. The prior
log/provenance is retained without claiming a new H071 baseline execution.

Python reports698 tests, four known installer/shell failures and one skip:
the installer signature fixture lacks `horizon-macos-arm64.zip`, and three
optional-archive cases encounter Bash3.2 empty-array unbound variables. No
installer or drop-motion repair is included; the full self-check is not green.

Formatting, Clippy with warnings denied, doctests and public-tree checks pass;
Cargo.lock is unchanged. The ledger check reports the existing callee advisories
and a long-uncited test-fixture helper, with no new gaps. The normal Nautilus
drown/dry_out gap remains explicitly open. Assigned Game parity ran separately
from quick mode's broad-harness skip. Only evidence notes were added afterward;
Rust and harness files are unchanged.

The later98af9399c source checkpoint has no air-source delta; an isolated
armor-stand harness filter does not change this fixture's selected group.
The actual Game pair remains frozen to87c51ab9c, without a latest-base replay
claim. Resource plans retained a5GiB hard guard and combined-link headroom,
jobs2/incremental0 and one heavy build. Only verified inactive OWN artifacts
were archived, preserving original paths and per-file hashes. Unknown phase
times stay null. H072 and H224 remain independent, with no pending production
imported and no combined-runtime claim.
