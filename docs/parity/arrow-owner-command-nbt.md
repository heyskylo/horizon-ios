# Loaded arrow Owner command NBT (H213)

Frozen source `41f822b659789780f58d32c63ddfb6887ea1b8bf`. Own actual Game versus fresh pinned Java 26.3, using `tools/test_arrow_owner_parity.py`. The predeclared complete-record contract has 85 records, with no masks or tolerances. Nine separately declared raw traces retain all actor/arrow bodies and absolute startup clocks.

Five independent ordinary arrows have absent, real pig, real armor stand, actual guarded player and unresolved UUID owners. Each arrow has an independently checked registered UUID/type, Pos/Motion/NoGravity, retained name/tag, loaded floor and actual relative clock0. Every Owner read has freshly reset result/success sentinels followed by their actual reads and a typed IntArray/presence predicate. The player UUID is independently admitted before use as an Owner. Unknown UUIDs must remain unknown, not become the local player.

Untouched Game: **73/85**; fixed Game: **85/85**, using the same program and byte-identical fresh Java reference. All setup/identity admissions pass. The twelve residuals below are exactly four Owner observations for each of the mob, stand and unresolved cases; absent and player controls pass.

## Exact untouched residuals

| Label | Java expected | Untouched Game actual | Callback expected / actual |
|---|---|---|---|
| mob-owner | `Arrow has the following entity data: [I; 213, 1, 2, 3]` | `Found no elements matching Owner` | `[[True, 4]]` / `[[False, 0]]` |
| mob-result | `result has 4 [h213]` | `result has 0 [h213]` | `[[True, 4]]` / `[[True, 0]]` |
| mob-success | `success has 1 [h213]` | `success has 0 [h213]` | `[[True, 1]]` / `[[True, 0]]` |
| mob-owner-type-presence | `Test passed. Count: 1` | `Test failed` | `[[True, 1]]` / `[[False, 0]]` |
| stand-owner | `Arrow has the following entity data: [I; 213, 4, 5, 6]` | `Found no elements matching Owner` | `[[True, 4]]` / `[[False, 0]]` |
| stand-result | `result has 4 [h213]` | `result has 0 [h213]` | `[[True, 4]]` / `[[True, 0]]` |
| stand-success | `success has 1 [h213]` | `success has 0 [h213]` | `[[True, 1]]` / `[[True, 0]]` |
| stand-owner-type-presence | `Test passed. Count: 1` | `Test failed` | `[[True, 1]]` / `[[False, 0]]` |
| unresolved-owner | `Arrow has the following entity data: [I; 213, 91, 92, 93]` | `Found no elements matching Owner` | `[[True, 4]]` / `[[False, 0]]` |
| unresolved-result | `result has 4 [h213]` | `result has 0 [h213]` | `[[True, 4]]` / `[[True, 0]]` |
| unresolved-success | `success has 1 [h213]` | `success has 0 [h213]` | `[[True, 1]]` / `[[True, 0]]` |
| unresolved-owner-type-presence | `Test passed. Count: 1` | `Test failed` | `[[True, 1]]` / `[[False, 0]]` |

## Source and class boundary

`load_arrow` calls the real `arrow_from_nbt`, adopts `Projectile.owner_uuid`, registers identity and resolves owners using the existing route. `entity_nbt_in_level` currently supplies only the acting-player UUID when `Who::Player`, otherwise None. Pinned `Projectile.addAdditionalSaveData` stores `EntityReference`'s UUID even if the entity is unresolved. The persistent save and portal paths already supply the retained UUID.

The repair supplies that retained UUID to `arrow_to_nbt`, with the old player fallback when no reference exists. Other projectile getter branches and all shared writers stay unchanged. Merged #8922/H208 origin resolution remains intact. Scope is ordinary arrow command NBT; spectral arrows share the arrow writer but have no runtime claim here. Fireballs, thrown items, spit/bullets and tridents are untested routes and are not changed.

The reachable Game regression loads/registers actual entities and first asserts real owner_uuid adoption. It checks no-raw and retained-raw states, positive modeled references overriding conflicting raw Owner UUIDs, absent references, identity and unchanged raw name/tags/clock. Generic stale optional-key cleanup for a modeled absent reference is outside this getter repair.

## Evidence and rejected prerequisites

```sh
python3 tools/test_arrow_owner_parity.py --reference-only --work-dir /tmp/arrow-java/run
python3 tools/test_arrow_owner_parity.py --binary /absolute/path/to/horizon-client --reference /tmp/arrow-java/run/vanilla.jsonl --work-dir /tmp/arrow-game/run
```

Directories must be fresh. Complete scripts/references/output, feedback/styles/callbacks, raw traces and immutable binaries remain in `.local/h213/`. Draft v1 exposed independent custom-name feedback; v2 exposed two-tag display ordering. Both executable scripts and raw pairs are retained as rejected prerequisites, with no qualified H213 baseline count. The admitted canonical v3 uses the ordinary Arrow name and one unique tag. No production name/tag policy changes are made.

- Program SHA256: `8d658915b732ba2dd0e2ccca99bbfad61aa5456dfb55e2eb690859f7b184761d`
- Fresh Java reference SHA256: `d864c8ee4f55761c04dfc3c7a35b147c864e95b82c24f2fe0576ef5078a2c9f3`
- Untouched Game SHA256: `e45a7e8023e21869d10c1b010f3c3617bb092f62e3b941a94bbd986619d1e131`

## Full raw body diagnostics

Untouched paired absolute time is Java305/Game0 at both start/end; fixed is Java317/Game0 at both start/end. The exact relative delta is separately admitted as0; no absolute value is normalized.

6/9 separately declared raw traces match after the fix. Exact unmatched text follows; complete styles/callbacks and all matching bodies remain in the retained logs. No full-body or absolute-clock parity claim.

```text
/execute store result score start h213 run time query gametime
JAVA: The game time is 317 tick(s)
GAME: The game time is 0 tick(s)
/data get entity 000000d5-0000-0001-0000-000200000003
JAVA: Pig has the following entity data: {NoGravity: 1b, AgeLocked: 0b, Brain: {memories: {}}, Invulnerable: 0b, FallFlying: 0b, ForcedAge: 0, sound_variant: "minecraft:classic", PortalCooldown: 0, AbsorptionAmount: 0.0f, InLove: 0, DeathTime: 0s, variant: "minecraft:temperate", PersistenceRequired: 0b, UUID: [I; 213, 1, 2, 3], Tags: ["h213_owner"], Age: 0, Motion: [0.0d, 0.0d, 0.0d], Health: 10.0f, LeftHanded: 0b, fall_distance: 0.0d, Air: 300s, OnGround: 0b, NoAI: 1b, Rotation: [0.0f, 0.0f], current_impulse_context_reset_grace_time: 0, Pos: [3.5d, 200.0d, 4.5d], Fire: 0s, CanPickUpLoot: 0b, attributes: [], HurtTime: 0s}
GAME: Pig has the following entity data: {NoGravity: 1b, AgeLocked: 0b, Brain: {memories: {}}, Invulnerable: 0b, FallFlying: 0b, ForcedAge: 0, sound_variant: "minecraft:classic", PortalCooldown: 0, AbsorptionAmount: 0.0f, InLove: 0, DeathTime: 0s, variant: "minecraft:temperate", PersistenceRequired: 0b, UUID: [I; 213, 1, 2, 3], Tags: ["h213_owner"], Age: 0, Motion: [0.0d, 0.0d, 0.0d], Health: 10.0f, LeftHanded: 0b, fall_distance: 0.0d, Air: 300s, OnGround: 0b, NoAI: 1b, Rotation: [0.0f, 0.0f], current_impulse_context_reset_grace_time: 0, Pos: [3.5d, 200.0d, 4.5d], Fire: 0s, CanPickUpLoot: 0b, attributes: [{id: "minecraft:movement_speed", base: 0.25d}], HurtTime: 0s}
/execute store result score end h213 run time query gametime
JAVA: The game time is 317 tick(s)
GAME: The game time is 0 tick(s)
```

## Local validation

The identical actual registered Game load/getter test fails old (`Owner: None`, expected the already-adopted pig UUID), then passes fixed with real prepared resources. Final required `pr_ready --quick --verbose --base 41f822b659789780f58d32c63ddfb6887ea1b8bf` ran once: **1078 Rust tests passed**; rustfmt, clippy (including server features), doc tests and public-tree checks passed. Overall exit1 is the three previously recorded macOS optional-release-archive Python subtests (masks0/1 `musl[@]`, mask2 `arm64[@]` unbound Bash arrays); 708 ran, one skipped. The 382 broad affected harnesses were listed for CI by `--quick`; the scoped actual Game pair was run locally.
