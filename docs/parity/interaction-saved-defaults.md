# Interaction saved settings

`Interaction.addAdditionalSaveData` in the pinned Java 26.3 server always writes
`width` and `height` as Floats and `response` as a Byte. Its reader defaults them
to `1.0f`, `1.0f` and false. The raw-backed `Target::Other` route previously
omitted all three when they were absent from the summon input.

`Game::other_to_nbt`, shared by command getters and chunk entity snapshots, now
supplies only these missing Interaction settings. Valid explicit settings,
names, tags and optional `attack`/`interaction` records remain intact. Other
kinds receive no new fields. Numeric normalization, live hitboxes/clicks,
last-attacker behavior and common Entity defaults are not covered.

## Reproduction and result

Frozen source base: `a1ff75a4a525b6a9e1ed68f74cf9d54161596370`.
Use an owned prepared 26.3 resource tree and Java on PATH:

```sh
python3 tools/test_interaction_saved_defaults_parity.py \
  --binary /absolute/path/to/horizon-client --work-dir /fresh/scratch/output
```

The driver declares 140 complete command records before capture. It checks three
UUID-bound Interactions: absent settings, explicit defaults, and explicit
`2.5f`/`3.5f`/`1b`. Positive controls cover loaded stone/air, owner mode/health,
actor type/UUID/position/rotation, name/tags/flags, exact saved field types,
presence, fresh result/success stores, optional record UUID/Long types, an
unrelated Marker and a modeled pig. The two historical UUID payloads in optional
records are data-preservation controls, not live actor witnesses. Actual relative
world time remains exactly zero.

A test-driver-only revision supplies a fresh default output directory for the
standard parity runner. All program operations are identical; original script
`67bf9060d86d9115c39b8174019c1b267a9de7a81bd7d507db33b5fc71eb83b1`
and its first paired captures remain preserved. Both versions independently give
the same following result.

The untouched Game matched **126/140**; the fixed Game matched **140/140**.
All admissions passed in both. The 14 old differences are reads/stores/presence/
type observations of the three missing defaults, not 14 independent defects.
Each paired run independently reran Java; its 140 reference records were
byte-identical. `_JAVA_OPTIONS=-Xmx1G` bounded the reference process.

- Program SHA256: `b26cd38ecce17c0622230a61a5524bd7a62499afce9ff59d031249995d803bb1`
- Reference SHA256: `67274d4d511d6482be1bc33d54c3df8b90e9570663c6663ce56290d4d1e52c21`

The ignored resource-backed regression
`game::persist::interaction_saved_defaults_tests::registered_interactions_save_default_and_loaded_settings`
uses real summon registration, command getters and `Game::entity_chunks`, plus a
registered no-raw supplement. Set `HORIZON_TEST_RESOURCES` to prepared resources
and run it explicitly with `--ignored`. Its identical old assertion fails on
missing Float width after registration and chunk-save admission. A rejected
test-only Option/array compile mismatch was corrected before that old assertion;
it is not runtime evidence. The fixed regression passes 1/1 using the same test
and fast profile.

## Retained raw diagnostics

Five full bodies and two absolute clock reads are predeclared raw traces, outside
the 140-record equality contract. Complete styled replies/callbacks remain in
`java.log` and `rust.log`; no field masks or numeric tolerances are applied to the
canonical records. Both old and fixed retain all seven diagnostic differences:

| Observation | Java | Game (old and fixed, unless noted) |
|---|---|---|
| absent Interaction body | common defaults plus Float1/Float1/Byte0 | old lacks the three settings; fixed has them; both lack `Invulnerable:0b`, `fall_distance:0d`, `Air:300s`, `OnGround:0b`, `PortalCooldown:0`; field presentation differs |
| explicit-default Interaction body | common defaults and supplied settings | supplied settings retained; same five common defaults missing and presentation differs |
| nondefault Interaction body | supplied settings and both optional records | all retained; same five common defaults missing and presentation differs |
| Marker body | common defaults including zero Motion | missing Motion and those five common defaults; `data:{audit:7}` retained |
| pig body | `attributes:[]` | `attributes:[{id:"minecraft:movement_speed",base:0.25d}]` |
| start absolute time | first pair317/305; CLI revision293/289 | 0 in both |
| end absolute time | first pair317/305; CLI revision293/289 | 0 in both |

Thus this is neither full-body nor absolute-clock equality. Common Marker fields
(H106), unrelated attributes and presentation remain separate. No claim covers
malformed settings, arbitrary numeric input types, physical interaction or
save/reopen lifecycle.
