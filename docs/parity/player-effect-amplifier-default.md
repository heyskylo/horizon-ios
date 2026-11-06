# Player saved effect amplifier default

H100 changes only `save.rs::Level::player_tag`: omit an amplifier whose modeled
value is zero. The existing clamp to 0..255 and unsigned Byte conversion stay
unchanged for other values. The loader, live effects, other effect fields and
separate Mob writer are untouched.

Pinned Java 26.3 `MobEffectInstance.Details` builds its map codec with
`ExtraCodecs.UNSIGNED_BYTE.optionalFieldOf("amplifier", 0)`. PR6369 changed only
the Mob writer. PR9061 supplied the Player unsigned-byte reload/clamp behavior,
which this fix preserves.

## Actual Game proof

Frozen base: `0d21d5520ec26c49b774bf1533e53ea8af81278d`.
Script: `tools/test_player_effect_amplifier_parity.py`, SHA-256
`2b2023bb710b24dac97953ee87765a9c1585c3c90ce4842cb3f0eed2e023605c`.
Fresh Java reference SHA-256:
`b4a8db3e861ac399f61093270580a5e2917882a40fa2ce9c1ca615763011f805`.

Real Player commands apply/clear an infinite Luck effect at amplifiers 0/1/255.
Saved reads use the existing `/data get entity @s` route: Game synchronizes the
live player and calls `Level::player_tag`, the same Player saved encoder used by
world/player file writes. No observer or packet implementation was added. This
proves that saved-NBT projection, without a save/reopen or disk-I/O claim.

The predeclared 71 command records compare as complete raw JSON, including all
messages/styles and callbacks, without masks/tolerances. UUID/type/Health20,
position/mode, load/floor/air, exact effect id/duration/type, one-effect count,
particle/icon flags, effective nonambient state, real effect removal and zero
relative world-clock controls stay strict. Each amplifier read has fresh
result/success sentinels. Whole effect/body compounds and absolute startup
clocks are separate complete raw traces, not whole-record equality claims.

Untouched Game matches 68/71; fixed Game matches 71/71 with identical script
and fresh Java reference bytes. Exactly three old records differ for amplifier 0:

| Record | Fresh Java | Untouched Game |
| --- | --- | --- |
| Saved amplifier read | Missing field; callback`[false,0]` | `0b`; callback`[true,0]` |
| Success store | `0` | `1` |
| Field-absence predicate | Test passed;`[true,1]` | Test failed, count1;`[false,0]` |

The positive 1 and 255 controls pass; 255 saves as typed Byte -1. Live effect
presence is independently witnessed by successful real effect removal, rather
than inferred from the saved amplifier's absence.

The resource-free save regression starts both with no raw input and with stale
raw effects. It checks 0 omission and Byte 1/-128/-1, unchanged id/duration/flags,
unrelated raw data and unchanged live model. On the old writer it fails at
`Some(Byte(0))` versus`None`; the existing 255-reload and ended-effect controls pass.

Source checkpoint `932d351e115e9a3e31f29eaaba0614004bbb3621` adds unrelated
region compression selection/tests in `save.rs`; the Player writer and command
read path are unchanged. The paired runtime proof remains qualified to 0d21.

## Retained whole-effect diagnostic

The original 68-record program/reference and complete old output remain local
and executable. It matches 62/68: the three H100 records above plus
`amp0-count`, `amp1-count`, `amp255-count`, which read whole effect lists.
Those lists also contain native `ambient:0b`, whereas Java omits the default.
The 71-record program replaces those three compound comparisons with exact
first-entry/absent-second-entry controls. It preserves their full raw reads as
traces. No ambient/default-flag repair belongs to H100; no whole-diagnostic equality claim. The fixed diagnostic matches 65/68; every
remaining exact saved-list value difference is below (all three callbacks
remain `[true,1]`).


| Label | Java saved list | Fixed Game saved list |
| --- | --- | --- |
| `amp0-count` | `[{duration: -1, show_icon: 0b, id: "minecraft:luck", show_particles: 0b}]` | `[{duration: -1, show_icon: 0b, ambient: 0b, id: "minecraft:luck", show_particles: 0b}]` |
| `amp1-count` | `[{duration: -1, show_icon: 0b, amplifier: 1b, id: "minecraft:luck", show_particles: 0b}]` | `[{duration: -1, show_icon: 0b, ambient: 0b, amplifier: 1b, id: "minecraft:luck", show_particles: 0b}]` |
| `amp255-count` | `[{duration: -1, show_icon: 0b, amplifier: -1b, id: "minecraft:luck", show_particles: 0b}]` | `[{duration: -1, show_icon: 0b, ambient: 0b, amplifier: -1b, id: "minecraft:luck", show_particles: 0b}]` |

Run in a fresh destination using the existing command harness:

```sh
python3 tools/test_player_effect_amplifier_parity.py --reference-only \
  --work-dir .local/player-effect-reference
python3 tools/test_player_effect_amplifier_parity.py --binary /path/to/horizon-client \
  --work-dir .local/player-effect-native --reference .local/player-effect-reference/vanilla.jsonl
```

The native invocation independently reruns Java and requires the exact same
reference bytes. All commands, original diagnostic scripts, raw records and
traces are retained locally, including the earlier 68-record compound admission
failure. The focused 71-record proof independently admits every actual subject/state.
