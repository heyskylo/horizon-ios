# Named entity_scores predicates (H270)

Frozen source `41f822b659789780f58d32c63ddfb6887ea1b8bf`. Own actual Game/fresh pinned Java26.3, `tools/test_entity_scores_parity.py`. The declared complete-record contract has **212 records** with no masks/tolerances. Seven separately declared raw traces retain the five full pig bodies and exact absolute startup clocks.

The real fixture pack contains one `h270:min2` entity_scores condition with `entity:this` and `pts:{min:2}`, plus always/type admission predicates. Five actual UUID-bound pigs have scores0/1/2/3/missing. Both if/unless named action paths reset fresh result/success/branch sentinels before each call. Two scores mutate (0→2,3→1), and every score and condition is read again. UUID/type/Health/Pos/NoAI/NoGravity/load/floor/pack, direct scores selectors and relative-clock0 remain strict. The named target proof is not replaced by a direct selector.

Untouched: **180/212**; fixed: **212/212**, with the same program and byte-identical fresh Java reference. All admissions pass. The32 residuals below are four action/store records for both signs at each positive state; negative/missing-score states, actual score reads and direct selectors already match. The actual registered Game named-evaluator unit first proves the holder UUID and score state, then fails old at score2.

## Exact untouched residuals

| Label | Java expected | Untouched Game actual | Callback expected / actual |
|---|---|---|---|
| initial-2-if-named | `Set [h270] for branch to 1` | `` | `[[True, 1]]` / `[]` |
| initial-2-if-result | `result has 1 [h270]` | `result has -99 [h270]` | `[[True, 1]]` / `[[True, -99]]` |
| initial-2-if-success | `success has 1 [h270]` | `success has -99 [h270]` | `[[True, 1]]` / `[[True, -99]]` |
| initial-2-if-branch | `branch has 1 [h270]` | `branch has 0 [h270]` | `[[True, 1]]` / `[[True, 0]]` |
| initial-2-unless-named | `` | `Set [h270] for branch to 1` | `[]` / `[[True, 1]]` |
| initial-2-unless-result | `result has -99 [h270]` | `result has 1 [h270]` | `[[True, -99]]` / `[[True, 1]]` |
| initial-2-unless-success | `success has -99 [h270]` | `success has 1 [h270]` | `[[True, -99]]` / `[[True, 1]]` |
| initial-2-unless-branch | `branch has 0 [h270]` | `branch has 1 [h270]` | `[[True, 0]]` / `[[True, 1]]` |
| initial-3-if-named | `Set [h270] for branch to 1` | `` | `[[True, 1]]` / `[]` |
| initial-3-if-result | `result has 1 [h270]` | `result has -99 [h270]` | `[[True, 1]]` / `[[True, -99]]` |
| initial-3-if-success | `success has 1 [h270]` | `success has -99 [h270]` | `[[True, 1]]` / `[[True, -99]]` |
| initial-3-if-branch | `branch has 1 [h270]` | `branch has 0 [h270]` | `[[True, 1]]` / `[[True, 0]]` |
| initial-3-unless-named | `` | `Set [h270] for branch to 1` | `[]` / `[[True, 1]]` |
| initial-3-unless-result | `result has -99 [h270]` | `result has 1 [h270]` | `[[True, -99]]` / `[[True, 1]]` |
| initial-3-unless-success | `success has -99 [h270]` | `success has 1 [h270]` | `[[True, -99]]` / `[[True, 1]]` |
| initial-3-unless-branch | `branch has 0 [h270]` | `branch has 1 [h270]` | `[[True, 0]]` / `[[True, 1]]` |
| mutated-0-if-named | `Set [h270] for branch to 1` | `` | `[[True, 1]]` / `[]` |
| mutated-0-if-result | `result has 1 [h270]` | `result has -99 [h270]` | `[[True, 1]]` / `[[True, -99]]` |
| mutated-0-if-success | `success has 1 [h270]` | `success has -99 [h270]` | `[[True, 1]]` / `[[True, -99]]` |
| mutated-0-if-branch | `branch has 1 [h270]` | `branch has 0 [h270]` | `[[True, 1]]` / `[[True, 0]]` |
| mutated-0-unless-named | `` | `Set [h270] for branch to 1` | `[]` / `[[True, 1]]` |
| mutated-0-unless-result | `result has -99 [h270]` | `result has 1 [h270]` | `[[True, -99]]` / `[[True, 1]]` |
| mutated-0-unless-success | `success has -99 [h270]` | `success has 1 [h270]` | `[[True, -99]]` / `[[True, 1]]` |
| mutated-0-unless-branch | `branch has 0 [h270]` | `branch has 1 [h270]` | `[[True, 0]]` / `[[True, 1]]` |
| mutated-2-if-named | `Set [h270] for branch to 1` | `` | `[[True, 1]]` / `[]` |
| mutated-2-if-result | `result has 1 [h270]` | `result has -99 [h270]` | `[[True, 1]]` / `[[True, -99]]` |
| mutated-2-if-success | `success has 1 [h270]` | `success has -99 [h270]` | `[[True, 1]]` / `[[True, -99]]` |
| mutated-2-if-branch | `branch has 1 [h270]` | `branch has 0 [h270]` | `[[True, 1]]` / `[[True, 0]]` |
| mutated-2-unless-named | `` | `Set [h270] for branch to 1` | `[]` / `[[True, 1]]` |
| mutated-2-unless-result | `result has -99 [h270]` | `result has 1 [h270]` | `[[True, -99]]` / `[[True, 1]]` |
| mutated-2-unless-success | `success has -99 [h270]` | `success has 1 [h270]` | `[[True, -99]]` / `[[True, 1]]` |
| mutated-2-unless-branch | `branch has 0 [h270]` | `branch has 1 [h270]` | `[[True, 0]]` / `[[True, 1]]` |

## Source and bounded support

Pinned `EntityHasScoreCondition.test` obtains the selected entity and level scoreboard; `hasScore` rejects missing objectives/scores before applying its integer range. The Game named command context uses existing `holder_name` and `Scoreboard::holder_scores` to snapshot actual current objective values. A bound empty map means a real holder without scores; an unbound context remains None. The condition reuses the existing numeric range helper and fails closed for unsupported score providers or entity targets.

Only `entity:this` and constant integer ranges are supported here. The new optional entity-view field is filled by the existing named/inline command predicate adapter; other contexts remain unbound. No global loot engine or automatic-context projection is added. Pig runtime proof does not establish Player/Guest attribution or every target class. Existing holder lookup policy is reused without new identity fallbacks. Its Orb/Projectile branches return None and remain unbound; they require a separate measured holder-binding follow-up. Other supported holder variants have source review only, no runtime claim. Non-score predicate behavior and direct selectors remain unchanged.

## Reproduce and retain

```sh
python3 tools/test_entity_scores_parity.py --reference-only --work-dir /tmp/scores-java/run
python3 tools/test_entity_scores_parity.py --binary /absolute/path/to/horizon-client --reference /tmp/scores-java/run/vanilla.jsonl --work-dir /tmp/scores-game/run
```

Fresh directories are required. Complete scripts, JSONL feedback/styles/callbacks, references, declarations, raw bodies, immutable binaries and manifests are retained under `.local/h270/`. Draftv1 had an invalid legacy pig-admission key (Java registry rejection). Draftv2 omitted actual datapack-list initialization before native registry parsing. Both executable scripts/raws are retained rejected prerequisites, with no qualified H270 failure count. The canonical v3 adds the real pack admission; no parser fix.

- Program SHA256: `70ce1ca3a6577b3922e6913dd1a2e851792adc46336a7dde1601e11b57a2af6a`
- Fresh Java reference SHA256: `4c6f5e6cd1267747ef7863d8297f109c8ddd6b66c157ed0266c567592404745c`
- Untouched Game SHA256: `e45a7e8023e21869d10c1b010f3c3617bb092f62e3b941a94bbd986619d1e131`

## Full raw diagnostics

0/7 separately declared full-body/absolute-clock traces match. Exact unmatched text follows; all complete styles/callbacks stay retained. These are not canonical row masks and no full-body/absolute-clock equality is claimed.

```text
/execute store result score start h270 run time query gametime
JAVA: The game time is 313 tick(s)
GAME: The game time is 0 tick(s)
/data get entity 0000010e-0000-0001-0000-000000000001
JAVA: Pig has the following entity data: {NoGravity: 1b, AgeLocked: 0b, Brain: {memories: {}}, Invulnerable: 0b, FallFlying: 0b, ForcedAge: 0, sound_variant: "minecraft:classic", PortalCooldown: 0, AbsorptionAmount: 0.0f, InLove: 0, DeathTime: 0s, variant: "minecraft:temperate", PersistenceRequired: 0b, UUID: [I; 270, 1, 0, 1], Tags: ["h270_0"], Age: 0, Motion: [0.0d, 0.0d, 0.0d], Health: 10.0f, LeftHanded: 0b, fall_distance: 0.0d, Air: 300s, OnGround: 0b, NoAI: 1b, Rotation: [0.0f, 0.0f], current_impulse_context_reset_grace_time: 0, Pos: [3.5d, 200.0d, 4.5d], Fire: 0s, CanPickUpLoot: 0b, attributes: [], HurtTime: 0s}
GAME: Pig has the following entity data: {NoGravity: 1b, AgeLocked: 0b, Brain: {memories: {}}, Invulnerable: 0b, FallFlying: 0b, ForcedAge: 0, sound_variant: "minecraft:classic", PortalCooldown: 0, AbsorptionAmount: 0.0f, InLove: 0, DeathTime: 0s, variant: "minecraft:temperate", PersistenceRequired: 0b, UUID: [I; 270, 1, 0, 1], Tags: ["h270_0"], Age: 0, Motion: [0.0d, 0.0d, 0.0d], Health: 10.0f, LeftHanded: 0b, fall_distance: 0.0d, Air: 300s, OnGround: 0b, NoAI: 1b, Rotation: [0.0f, 0.0f], current_impulse_context_reset_grace_time: 0, Pos: [3.5d, 200.0d, 4.5d], Fire: 0s, CanPickUpLoot: 0b, attributes: [{id: "minecraft:movement_speed", base: 0.25d}], HurtTime: 0s}
/data get entity 0000010e-0000-0001-0000-000000000002
JAVA: Pig has the following entity data: {NoGravity: 1b, AgeLocked: 0b, Brain: {memories: {}}, Invulnerable: 0b, FallFlying: 0b, ForcedAge: 0, sound_variant: "minecraft:classic", PortalCooldown: 0, AbsorptionAmount: 0.0f, InLove: 0, DeathTime: 0s, variant: "minecraft:temperate", PersistenceRequired: 0b, UUID: [I; 270, 1, 0, 2], Tags: ["h270_1"], Age: 0, Motion: [0.0d, 0.0d, 0.0d], Health: 10.0f, LeftHanded: 0b, fall_distance: 0.0d, Air: 300s, OnGround: 0b, NoAI: 1b, Rotation: [0.0f, 0.0f], current_impulse_context_reset_grace_time: 0, Pos: [4.5d, 200.0d, 4.5d], Fire: 0s, CanPickUpLoot: 0b, attributes: [], HurtTime: 0s}
GAME: Pig has the following entity data: {NoGravity: 1b, AgeLocked: 0b, Brain: {memories: {}}, Invulnerable: 0b, FallFlying: 0b, ForcedAge: 0, sound_variant: "minecraft:classic", PortalCooldown: 0, AbsorptionAmount: 0.0f, InLove: 0, DeathTime: 0s, variant: "minecraft:temperate", PersistenceRequired: 0b, UUID: [I; 270, 1, 0, 2], Tags: ["h270_1"], Age: 0, Motion: [0.0d, 0.0d, 0.0d], Health: 10.0f, LeftHanded: 0b, fall_distance: 0.0d, Air: 300s, OnGround: 0b, NoAI: 1b, Rotation: [0.0f, 0.0f], current_impulse_context_reset_grace_time: 0, Pos: [4.5d, 200.0d, 4.5d], Fire: 0s, CanPickUpLoot: 0b, attributes: [{id: "minecraft:movement_speed", base: 0.25d}], HurtTime: 0s}
/data get entity 0000010e-0000-0001-0000-000000000003
JAVA: Pig has the following entity data: {NoGravity: 1b, AgeLocked: 0b, Brain: {memories: {}}, Invulnerable: 0b, FallFlying: 0b, ForcedAge: 0, sound_variant: "minecraft:classic", PortalCooldown: 0, AbsorptionAmount: 0.0f, InLove: 0, DeathTime: 0s, variant: "minecraft:temperate", PersistenceRequired: 0b, UUID: [I; 270, 1, 0, 3], Tags: ["h270_2"], Age: 0, Motion: [0.0d, 0.0d, 0.0d], Health: 10.0f, LeftHanded: 0b, fall_distance: 0.0d, Air: 300s, OnGround: 0b, NoAI: 1b, Rotation: [0.0f, 0.0f], current_impulse_context_reset_grace_time: 0, Pos: [5.5d, 200.0d, 4.5d], Fire: 0s, CanPickUpLoot: 0b, attributes: [], HurtTime: 0s}
GAME: Pig has the following entity data: {NoGravity: 1b, AgeLocked: 0b, Brain: {memories: {}}, Invulnerable: 0b, FallFlying: 0b, ForcedAge: 0, sound_variant: "minecraft:classic", PortalCooldown: 0, AbsorptionAmount: 0.0f, InLove: 0, DeathTime: 0s, variant: "minecraft:temperate", PersistenceRequired: 0b, UUID: [I; 270, 1, 0, 3], Tags: ["h270_2"], Age: 0, Motion: [0.0d, 0.0d, 0.0d], Health: 10.0f, LeftHanded: 0b, fall_distance: 0.0d, Air: 300s, OnGround: 0b, NoAI: 1b, Rotation: [0.0f, 0.0f], current_impulse_context_reset_grace_time: 0, Pos: [5.5d, 200.0d, 4.5d], Fire: 0s, CanPickUpLoot: 0b, attributes: [{id: "minecraft:movement_speed", base: 0.25d}], HurtTime: 0s}
/data get entity 0000010e-0000-0001-0000-000000000004
JAVA: Pig has the following entity data: {NoGravity: 1b, AgeLocked: 0b, Brain: {memories: {}}, Invulnerable: 0b, FallFlying: 0b, ForcedAge: 0, sound_variant: "minecraft:classic", PortalCooldown: 0, AbsorptionAmount: 0.0f, InLove: 0, DeathTime: 0s, variant: "minecraft:temperate", PersistenceRequired: 0b, UUID: [I; 270, 1, 0, 4], Tags: ["h270_3"], Age: 0, Motion: [0.0d, 0.0d, 0.0d], Health: 10.0f, LeftHanded: 0b, fall_distance: 0.0d, Air: 300s, OnGround: 0b, NoAI: 1b, Rotation: [0.0f, 0.0f], current_impulse_context_reset_grace_time: 0, Pos: [6.5d, 200.0d, 4.5d], Fire: 0s, CanPickUpLoot: 0b, attributes: [], HurtTime: 0s}
GAME: Pig has the following entity data: {NoGravity: 1b, AgeLocked: 0b, Brain: {memories: {}}, Invulnerable: 0b, FallFlying: 0b, ForcedAge: 0, sound_variant: "minecraft:classic", PortalCooldown: 0, AbsorptionAmount: 0.0f, InLove: 0, DeathTime: 0s, variant: "minecraft:temperate", PersistenceRequired: 0b, UUID: [I; 270, 1, 0, 4], Tags: ["h270_3"], Age: 0, Motion: [0.0d, 0.0d, 0.0d], Health: 10.0f, LeftHanded: 0b, fall_distance: 0.0d, Air: 300s, OnGround: 0b, NoAI: 1b, Rotation: [0.0f, 0.0f], current_impulse_context_reset_grace_time: 0, Pos: [6.5d, 200.0d, 4.5d], Fire: 0s, CanPickUpLoot: 0b, attributes: [{id: "minecraft:movement_speed", base: 0.25d}], HurtTime: 0s}
/data get entity 0000010e-0000-0001-0000-000000000005
JAVA: Pig has the following entity data: {NoGravity: 1b, AgeLocked: 0b, Brain: {memories: {}}, Invulnerable: 0b, FallFlying: 0b, ForcedAge: 0, sound_variant: "minecraft:classic", PortalCooldown: 0, AbsorptionAmount: 0.0f, InLove: 0, DeathTime: 0s, variant: "minecraft:temperate", PersistenceRequired: 0b, UUID: [I; 270, 1, 0, 5], Tags: ["h270_4"], Age: 0, Motion: [0.0d, 0.0d, 0.0d], Health: 10.0f, LeftHanded: 0b, fall_distance: 0.0d, Air: 300s, OnGround: 0b, NoAI: 1b, Rotation: [0.0f, 0.0f], current_impulse_context_reset_grace_time: 0, Pos: [7.5d, 200.0d, 4.5d], Fire: 0s, CanPickUpLoot: 0b, attributes: [], HurtTime: 0s}
GAME: Pig has the following entity data: {NoGravity: 1b, AgeLocked: 0b, Brain: {memories: {}}, Invulnerable: 0b, FallFlying: 0b, ForcedAge: 0, sound_variant: "minecraft:classic", PortalCooldown: 0, AbsorptionAmount: 0.0f, InLove: 0, DeathTime: 0s, variant: "minecraft:temperate", PersistenceRequired: 0b, UUID: [I; 270, 1, 0, 5], Tags: ["h270_4"], Age: 0, Motion: [0.0d, 0.0d, 0.0d], Health: 10.0f, LeftHanded: 0b, fall_distance: 0.0d, Air: 300s, OnGround: 0b, NoAI: 1b, Rotation: [0.0f, 0.0f], current_impulse_context_reset_grace_time: 0, Pos: [7.5d, 200.0d, 4.5d], Fire: 0s, CanPickUpLoot: 0b, attributes: [{id: "minecraft:movement_speed", base: 0.25d}], HurtTime: 0s}
/execute store result score end h270 run time query gametime
JAVA: The game time is 313 tick(s)
GAME: The game time is 0 tick(s)
```

## Local validation

The identical actual registered Game named-predicate test fails old at score2 after independent UUID/holder/score admission, then passes fixed1/1 including mutations and missing score. Required `pr_ready --quick --verbose --base 41f822b659789780f58d32c63ddfb6887ea1b8bf` ran once final: **3158 Rust tests passed**, plus rustfmt, clippy (including server features), doc tests and public-tree checks. Overall exit1 is the same three pre-existing macOS optional-release-archive Python subtests (masks0/1 `musl[@]`, mask2 `arm64[@]` unbound Bash arrays); 708 ran, one skipped. The382 broad affected harnesses were listed for CI by `--quick`; the focused actual Game pair was executed locally.
