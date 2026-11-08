# Glow squid NoAI dark-tick upkeep (H176)

A glow squid's positive DarkTicksRemaining counts down after the parent aiStep even
with NoAI. Extract the existing positive-only countdown from glow_squid_ai_step and
call it from Mob.tick's NoAI branch. Normal AI and the existing remote animation helper
keep their one countdown call at the same position before the unchanged glow-particle
RNG draws. The new NoAI call performs no movement, particle or RNG work.

Pinned Java26.3 GlowSquid.aiStep bytecode calls Squid.aiStep at offset1, reads the
counter at5 and decrements only when positive at10–17. Own port ledger/bytecode and
full current/closed duplicate searches were reviewed. Merged3944 introduced the helper;
8864 concerns damage dimming and is not this route. H177 is excluded: there is no damage
in this program and no damage/ink policy change.

## Frozen actual Game/Java program

Base `3d33b0299d5c80769300d8b33dafb389ac9e7656`. The existing actual Game command
harness and fresh pinned Java run four loaded glow squid, NoAI0/1 crossed with loaded
DarkTicksRemaining40/-2, at cumulative actor ticks0/1/5/10 (steps1,4,5).
The UUID words are [176,0,0,1] through [176,0,0,4], with independently observed unique
tags, type, Health10, NoAI/default absence, NoGravity1 and PersistenceRequired1.
They start at [4.5,203,4.5], [10.5,203,4.5], [4.5,203,10.5], [10.5,203,10.5] in a
positively loaded glass pool with stone foundation and full source water. Actual feet
and head water, floor/shell, actual owner UUID/Health/mode/Pos, and exact elapsed game-time
deltas are guarded at every snapshot. No unavailable entity or field can stand in for
a countdown observation. Counter input comes only from summon NBT, never damage.

The strict224 program was declared before fresh Java and the paired native runs.
It compares every recorded JSON record (feedback, styles and callbacks) exactly, without value
masks or tolerances. NoAI positions and all initial positions are strict. Full raw
bodies/Motion, later ordinary-AI positions and absolute clock reads still execute and
are traced; the full267 program records all43 diagnostics too. Those positions are
independently guarded as remaining in loaded water, not claimed as general trajectory
parity. The complete old/fixed programs are identical within each mode.

| Actor | Loaded counter | t0 | t1 | t5 | t10 |
|---|---:|---:|---:|---:|---:|
| NoAI1 positive | 40 | 40 | 39 | 35 | 30 |
| NoAI1 negative | -2 | -2 | -2 | -2 | -2 |
| NoAI0 positive | 40 | 40 | 39 | 35 | 30 |
| NoAI0 negative | -2 | -2 | -2 | -2 | -2 |

The actual Mob.tick regression builds a real resource-backed water world and uses
actual glow-squid types. Normal-AI and negative controls run before the old intended
NoAI positive assertion (40 versus39 at tick1). It checks health and existing particle
dispatch counts as preservation controls, not new official particle parity: NoAI's
existing zero events stay zero, ordinary AI keeps one per tick. The resource requirement
is explicit and the test asserts that the world builder ran, preventing a silent skip.

Replay (resource tests need isolated fresh homes/cwd as in AGENTS.md):

```sh
python3 tools/test_glow_squid_noai_parity.py --binary /path/to/game --work-dir .local/h176/replay
python3 tools/test_glow_squid_noai_parity.py --full-diagnostic --binary /path/to/game --work-dir .local/h176/full-replay
cargo test -p horizon-client-lib --lib mob_tick_keeps_glow_squid_countdowns_with_no_ai -- --ignored --nocapture
```

Raw commands/JSON/bodies/styles, full expected/actual reports, Java logs, bytecode,
source checkpoints, hashes and immutable binaries remain locally under `.local/h176/`.
No prepared data or Mojang files are committed. No new observer or renderer is used.
No damage/ink, movement, metadata, packet, particle/RNG parity or reopen claim is made.

## Declared hashes

| Artifact | SHA-256 |
|---|---|
| canonical program | `d5d4e95b3213478c8707bfe4e156a883a048530bbf8b0822eea65bd3247ccada` |
| Fresh canonical Java records | `36f9303bfdb2fbb953ad32e085d390745613d93aeb81c8bc2499e6c9c219bd4c` |
| full program | `df4e2845f7532c99994b27aa6e4604f0d13a6dd89bf690b7f69a3705d3547af7` |
| Fresh full Java records | `9e3fa1e6e7c88920c8c8270aaf6d7512c83a203781aa62b7fb316f86a5635a07` |

## Paired results and exact residuals

| Program | Untouched Game | Fixed Game |
|---|---:|---:|
| Strict complete records | 221/224 | **224/224** |
| Full diagnostic | 252/267 | **258/267** |

Strict old/fixed programs and independently regenerated Java reference records are
byte-identical. The only three old strict differences are the NoAI positive counter
remaining40 instead of39/35/30. The corresponding three full-body differences are fixed
too. Normal-AI and negative controls pass both builds, without duplicate countdowns.

All nine remaining full records are unchanged by the fix. Exact field/text differences
and callbacks follow; this readable table is not a comparison projection. Complete styled
JSON expected/actual records remain in strict-report.json. There is no full267-green or
raw absolute-clock equality claim.

| Label | Expected Java | Actual native | Separate diagnostic |
|---|---|---|---|
| `clock-start` | `The game time is 309 tick(s)`; `[true,309]` | `The game time is 0 tick(s)`; `[true,0]` | Unequal startup clocks; relative0/1/5/10 remains strict |
| `0-clock-raw` | `The game time is 309 tick(s)`; `[true,309]` | `The game time is 0 tick(s)`; `[true,0]` | Unequal startup clocks; relative0/1/5/10 remains strict |
| `0-noai_positive-body` | `attributes: []`; `[true,1]` | `attributes: [{id:"minecraft:movement_speed",base:0.7d}]`; `[true,1]` | Extra initial default-attribute serialization; not a formatter or H176 repair |
| `0-noai_negative-body` | `attributes: []`; `[true,1]` | `attributes: [{id:"minecraft:movement_speed",base:0.7d}]`; `[true,1]` | Extra initial default-attribute serialization; not a formatter or H176 repair |
| `0-ai_positive-body` | `attributes: []`; `[true,1]` | `attributes: [{id:"minecraft:movement_speed",base:0.7d}]`; `[true,1]` | Extra initial default-attribute serialization; not a formatter or H176 repair |
| `0-ai_negative-body` | `attributes: []`; `[true,1]` | `attributes: [{id:"minecraft:movement_speed",base:0.7d}]`; `[true,1]` | Extra initial default-attribute serialization; not a formatter or H176 repair |
| `1-clock-raw` | `The game time is 310 tick(s)`; `[true,310]` | `The game time is 1 tick(s)`; `[true,1]` | Unequal startup clocks; relative0/1/5/10 remains strict |
| `5-clock-raw` | `The game time is 314 tick(s)`; `[true,314]` | `The game time is 5 tick(s)`; `[true,5]` | Unequal startup clocks; relative0/1/5/10 remains strict |
| `10-clock-raw` | `The game time is 319 tick(s)`; `[true,319]` | `The game time is 10 tick(s)`; `[true,10]` | Unequal startup clocks; relative0/1/5/10 remains strict |

## Local readiness and source checkpoints

The unchanged resource-backed actual Mob.tick regression passes fixed (one executed test),
after the intended old failure. Required pr_ready --quick ran once on the final gameplay
patch: formatting, workspace/headless clippy, doc tests and public tree pass. The library
suite reports1880 passed/1 failed/63 ignored. Its failure is the pre-existing drop-motion
bit test at yaw37.5/pitch-21.25: actual Z bits4597448471913462162 versus expected
4597448471913462163. The archived untouched binary reproduces the identical failure;
entities.rs is unchanged. No movement repair is included.

Python ran686 tests, four failed and one skipped. These are unchanged macOS packaging
failures: the signature mock lacks the ARM64 archive, and optional-archive masks0/1/2
encounter unset Bash3.2 arrays. The focused Game/Java programs and ignored resource-backed
regression were explicitly run; --quick does not claim broad parity-suite coverage.
Only evidence documentation was finalized afterward, followed by public-tree/whitespace.

Actual subsequent main pulls reached86a9d9e66 (launcher screenshots) and finally
`5849fa33b306beae3f218d03694f4e17497d59c4` (mod-browser/UI plus launcher changes).
Relevant mob tick/aquatic/codec/creature/command-driver sources are unchanged. The new
browser polling is idle without an explicit browser/install job; this fixture never
opens it. These source changes were reviewed, not imported or called a combined latest-main
runtime proof. The paired source base remains3d33b0299.
