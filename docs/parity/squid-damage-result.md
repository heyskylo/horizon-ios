# Squid's inherited damage acceptance result

Pinned Java26.3 `Squid.hurtServer` first calls its parent, then reports true only
with a non-null last living attacker. `GlowSquid.hurtServer` returns that same
result and sets dark ticks100 only when true. A false subclass result does not
undo the parent's health loss.

Current main already has the correct ink/dark guards, from #8864 (closed #8724).
They remain untouched. The sole gameplay change replaces the shared adapter's
final unconditional true with the inherited squid/glow acceptance condition.
Other species retain true after their accepted shared parent path. The old guard
test's erroneous environment-hit true assertion is corrected, adding health-loss
checks without changing that guard's implementation.

No parent-health/identity refactor, H176 timer, stale-attacker lifecycle, ink RNG,
particles, sound, packet, rendering or other-species behavior repair/claim.

## Declared actual Game/Java program

```
python3 tools/test_squid_damage_result_parity.py --binary /path/to/game --work-dir /scratch/run
python3 tools/test_squid_damage_result_parity.py --full-diagnostic --binary /path/to/game --work-dir /scratch/full
```

`--reference /scratch/old/vanilla.jsonl` requires byte-identical fresh Java bytes.
The existing command harness runs actual authoritative Game with its connected
owner and independently runs pinned Java in isolated fresh data. No new observer.

Six fresh, unique-UUID victims: GlowSquid NoAI0/1 crossed with generic/cow-attributed
hits; ordinary Squid NoAI1 is the independently Java-measured inherited control.
Each starts Health10 and receives one generic1 hit. A real healthy NoAI cow causes
the attributed hits. Actor UUID/type/Health/Pos/NoAI/NoGravity/Silent/Persistence,
loaded water/floor/glass geometry, owner identity/position and actual relative
clock0 guard admission. No world steps, reused victim, stale attacker or cooldown
history; the cow remains independently alive/unchanged after all actions.

Each hit has fresh result/success sentinels set to-99. Fresh Java FIRST measured
failure feedback/callback/stores: this program invokes false/0 and stores0 for
unattributed hits, not an assumed missing callback. Health becomes9 and glow dark
remains0. Cow-attributed hits give callback true/1 and both stores1, Health9 and
glow dark100. Every damage message/style/callback/store remains strictly compared.

Predeclared before native captures: **193** complete raw records. Only full bodies,
Motion vectors and absolute clock-start/end are diagnostic; all commands remain
traced. `--full-diagnostic` strictly compares all **221** records. No masks,
tolerances, feedback normalization or selection changes after capture. Complete
Motion records are retained without an RNG/physics parity claim.

Frozen source: `2266c4de14561146044c422e6c525b3ce52b0f79`.

| Program | Exact script SHA256 |
|---|---|
| Scoped193 | `25eec1729a070935d3b4bf075978bd1a44399df11a86e770a6387a8d862e4a7b` |
| Full221 | `548fe826c429f551e55012a16bdff078ffca9e5147e4c7e58b009674028c3dc1` |

Old current Game: **184/193**. Health9 and dark0 already match; historical dark100
failures are not recycled as this baseline. Exactly these three suffixes differ
for EACH of glow_ai0_generic, glow_ai1_generic and squid_generic (nine records):

| Suffix | Java expected | Old native actual |
|---|---|---|
| -hit | Invulnerable failure, callback false/0 | Applied1 damage success, true/1 |
| -result | 0 | 1 |
| -success | 0 | 1 |

Every attributed hit, actual health/dark/type/identity/geometry/clock control
passes old. Full old diagnostic: **196/221**, including full bodies and Motion.

## Reachable regressions and retained evidence

`glow_squid_damage_result_keeps_parent_health_side_effect` and its Squid twin
call actual `Mob.damage` on real prepared entity definitions. Both fail old at
returned true versus false AFTER asserting Health9, last attacker and dark0.
Both pass fixed. Fresh victims cover both NoAI modes, with prior positive cow-hit,
other-species Cow generic acceptance and parent-invulnerability controls. No
copied health algorithm, synthetic return value or clock setter. The existing
resource-backed glow guard test also passes with its corrected return expectation.

Programs, byte-identical scoped references, raw styled feedback/callbacks, full
bodies/Motion, transcripts, source/checkpoint hashes, pinned bytecode and immutable
old/fixed executables are retained under the owner's `.local/h177`. No game files
are published. Available file timestamps and logged stage durations are retained;
unobserved event stage times remain unknown.

## Fixed result and complete diagnostic residuals

Final actual Game: **193/193**, versus **184/193** old, with byte-identical scoped
script and fresh Java reference bytes. Both resource-backed Mob.damage tests and
the corrected existing dark-guard test pass. Full221: **196/221 → 205/221**. No
full221, whole-body, RNG/ink or raw-clock parity claim.

The full program is byte-identical across builds. Its fresh Java references differ
only at absolute clocks; non-clock records are identical. All Motion records in
this fixture match, but do not establish a general movement/RNG contract. All16
remaining full-record differences are listed below. In addition to extra default
attributes, the three attributed after-bodies omit Java's saved attacker fields.
This saved-attacker projection remains an unrelated diagnostic; runtime attributed
acceptance/dark100 are strictly covered. Complete raw bodies/styles/callbacks
remain mismatches in the full diagnostic. No normalization changes the counts.

| Exact label | Java expected | Fixed native actual |
|---|---|---|
| `clock-start` | `305 (old fresh reference: 265)` | `0` |
| `before-cow-body` | `attributes: []` | `attributes: [{id: "minecraft:movement_speed", base: 0.20000000298023224d}]` |
| `glow_ai0_cow-before-body` | `attributes: []` | `attributes: [{id: "minecraft:movement_speed", base: 0.7d}]` |
| `glow_ai0_cow-after-body` | `attributes: []; last_hurt_by_mob: [I; 177, 0, 0, 100]; ticks_since_last_hurt_by_mob: 0` | `attributes: [{id: "minecraft:movement_speed", base: 0.7d}]; both saved attacker fields absent` |
| `glow_ai0_generic-before-body` | `attributes: []` | `attributes: [{id: "minecraft:movement_speed", base: 0.7d}]` |
| `glow_ai0_generic-after-body` | `attributes: []` | `attributes: [{id: "minecraft:movement_speed", base: 0.7d}]` |
| `glow_ai1_cow-before-body` | `attributes: []` | `attributes: [{id: "minecraft:movement_speed", base: 0.7d}]` |
| `glow_ai1_cow-after-body` | `attributes: []; last_hurt_by_mob: [I; 177, 0, 0, 100]; ticks_since_last_hurt_by_mob: 0` | `attributes: [{id: "minecraft:movement_speed", base: 0.7d}]; both saved attacker fields absent` |
| `glow_ai1_generic-before-body` | `attributes: []` | `attributes: [{id: "minecraft:movement_speed", base: 0.7d}]` |
| `glow_ai1_generic-after-body` | `attributes: []` | `attributes: [{id: "minecraft:movement_speed", base: 0.7d}]` |
| `squid_cow-before-body` | `attributes: []` | `attributes: [{id: "minecraft:movement_speed", base: 0.7d}]` |
| `squid_cow-after-body` | `attributes: []; last_hurt_by_mob: [I; 177, 0, 0, 100]; ticks_since_last_hurt_by_mob: 0` | `attributes: [{id: "minecraft:movement_speed", base: 0.7d}]; both saved attacker fields absent` |
| `squid_generic-before-body` | `attributes: []` | `attributes: [{id: "minecraft:movement_speed", base: 0.7d}]` |
| `squid_generic-after-body` | `attributes: []` | `attributes: [{id: "minecraft:movement_speed", base: 0.7d}]` |
| `after-cow-body` | `attributes: []` | `attributes: [{id: "minecraft:movement_speed", base: 0.20000000298023224d}]` |
| `clock-end` | `305 (old fresh reference: 265)` | `0` |

Scoped reference SHA256: `6b3a5398408e2bcdac6d6d949fb5bed88e8cb6d25d96fc97b913321bfcb492ef`.
Full fixed fresh reference SHA256: `2856764df3a97dbf707dc6ad5417a205f34b53c3a473ce05062f0e42b9a04f5e`.

## Local readiness and source checkpoint

Final production/test patch: rustfmt, clippy with warnings denied, doc tests and
public-tree checks pass. `pr_ready.py --quick --base 2266c4de14561146044c422e6c525b3ce52b0f79`
runs 1887 passing Rust tests, one failure, 64 ignored. The failure is the existing
`drops_match_vanillas_motion_bit_for_bit` Z last-bit difference at yaw37.5,
pitch-21.25: actual4597448471913462162 versus expected4597448471913462163.
The own archived OLD executable reproduces exactly the same failure. No drop
physics edit. Both new ignored resource-backed tests were explicitly run and pass,
as does the corrected existing guard test.

Python: 696 tests, four failures and one skip. One is the installer signature
fixture offering x86_64 assets on an ARM64 host; three are optional-release
archive combinations hitting macOS Bash3.2 unset arrays. Narrow installer rerun
confirms its prerequisite. No installer/release repair. Broad parity is skipped
by `--quick`; the explicit actual Game/Java pairs above were run independently.
The readiness command exits nonzero for these unrelated failures; it is not green.

Latest actual main pull/checkpoint: `42e6d9cc498998b7bd9d5648360f72531ae0fd8e`.
The relevant Mob/species/damage-command and command-harness/Java-oracle files have
no delta from the frozen proof base. No rebase, main merge or combined-latest
runtime claim. Evidence-only documentation was completed after the required run;
whitespace/public-tree checks are repeated on the final staged files.
