# Verified hostile sound families

H068 adds only bogged, breeze, endermite, parched, warden, zombie_villager, vex,
wither and the shared `entity.hostile` family to inferred HOSTILE5. Pinned26.3
`Monster.getSoundSource` returns HOSTILE; the eight verified types inherit that
source. `Monster.getHurtSound/getDeathSound` supply the common pair used by Giant
(merged#7834). Existing25 names, other branches, normalization, explicit sources,
volumes, names, timing and RNG calls are unchanged. No all-hostile census claim.

## Frozen live category contract

Base `69fab597a6f0e8a6ce524d38d1b73e599e343034`. Fresh Java26.3 generated the small
loaded platform/roof world; both actual dedicated Game runs serve independent
copies. One real connected Alice has actual UUID/type/Health20/creative/Pos guards.
Twelve distinct fixed-UUID actors have typed species/Health/NoAI/NoGravity and
loaded floor/air guards. Eleven fresh hurt cases set Health6, require accepted
result/success1 with fresh sentinels, and independently check live Health5. One
fresh Giant takes a lethal hit; raw snapshots independently show Health0.

Eight assigned hostile types, Giant, zombie and cow supply the hurt events;
Giant supplies common death. Each hit is followed by exactly one real world step
to drain native events, for12 total. No hit is retried and no extra world step is
used for sound readiness. Receipt barriers wait only for the actual packet. This
does not claim identical Java/native emission timing. Body/position snapshots
remain raw; post-hit trajectories are not compared.

Predeclared16-row contract: stage, event name, source ordinal and sound position.
Actor/control admissions remain required on both sides. Zombie5 and cow6 are
live controls; explicit block4/player7/record2 and Warden-neutral6 override are
controls. Control volume/pitch1 bits are exact. Random hurt pitch/volume must be
finite/positive; no exact random pitch, seed or full packet-family comparison.
No Breeze ambient output is required (H069 remains independent).

The existing typed SoundPacket switch is reused unchanged from own#10266 in
`LocatorBarClient`: vanilla's protocol decoder reads both servers and retains
all positional sound payload bytes, names, source names, volume/pitch bits,
seeds and arrival order. Nothing is reconstructed from a desired category. All
recorded sound rows stay raw, including any extra ambient sound; only the
predeclared event contract is canonical. No new observer/protocol implementation.
H128 production and H331 arrow production/decoder are not imported or changed.
Keep the identical test-only receipt switch once when combining#10266, while
preserving both drivers.

Program JSON SHA-256 (declared before fresh Java/baseline):
`fc1d6ae79de25bf520ed8e511b9e20a43f1bb4012b6fbcfee7cb0b8ba526c39f`.

```sh
python3 tools/test_hostile_sound_categories_parity.py --binary /path/to/old-game --reference-only --work-dir /tmp/h068-java
python3 tools/test_hostile_sound_categories_parity.py --binary /path/to/old-game --reference-dir /tmp/h068-java --work-dir /tmp/h068-old
python3 tools/test_hostile_sound_categories_parity.py --binary /path/to/fixed-game --reference-dir /tmp/h068-java --work-dir /tmp/h068-fixed
```

Each output directory is fresh; references are immutable. The owned untouched
69fab executable was reused from the completed H041 baseline after source/hash
verification, not from a historical capture. SHA-256:
`440bc67f89ce05a1691798cde40a5f9c05bfb61699cb124a132803672433b3bd`.
The H068 captures themselves are fresh. Fixed Game is independently built from
the same frozen base plus this classifier change.

## Unit and evidence limits

The resource-free classifier regression first verifies existing zombie/cow,
block/player/record/horn/weather/ambient categories and actual `Audio::play_on`
overrides (arrow6 and Warden4). It then checks hurt/death for all nine additions,
with and without the namespace. Old fails at bogged6 versus5; fixed passes. It
preserves the existing arrow inference branch and checks override precedence;
no live arrow-route parity or H331 fix is claimed.

All program/reference hashes, raw typed sound packets/payloads/seeds/pitch/order,
chat, complete actor bodies, command sent/receipt times, actual clocks, logs,
source/header review and immutable executable evidence remain under `.local/h068/`.
Command/stage timestamps are real UTC observations. Packet receipt and sound
emission timestamps are unavailable and explicitly null; original reused build
start is null. Available build/check elapsed times are retained, not inferred.
Verified inactive owned executable archives retain original paths, modes and
hashes; no shared dependency tree or other owner's artifact was removed.

No Breeze ambient routing, mob damage policy, AI/physics, exact random stream,
full sound-event/packet family, client rendering, mixer or reopen closure claim.

## Results

Fresh Java16/16; untouched current Game **6/16 → fixed16/16**. All actor/action,
position, override and twelve-tick admissions pass on both Game binaries.

| Stage / event | Java | Old Game | Fixed Game |
| --- | --- | --- | --- |
| `control_block` / `minecraft:block.note_block.harp` | 4 | 4 | 4 |
| `control_player` / `minecraft:entity.player.hurt` | 7 | 7 | 7 |
| `control_record` / `minecraft:music_disc.13` | 2 | 2 | 2 |
| `control_override` / `minecraft:entity.warden.hurt` | 6 | 6 | 6 |
| `bogged` / `minecraft:entity.bogged.hurt` | 5 | 6 | 5 |
| `breeze` / `minecraft:entity.breeze.hurt` | 5 | 6 | 5 |
| `endermite` / `minecraft:entity.endermite.hurt` | 5 | 6 | 5 |
| `parched` / `minecraft:entity.parched.hurt` | 5 | 6 | 5 |
| `warden` / `minecraft:entity.warden.hurt` | 5 | 6 | 5 |
| `zombie_villager` / `minecraft:entity.zombie_villager.hurt` | 5 | 6 | 5 |
| `vex` / `minecraft:entity.vex.hurt` | 5 | 6 | 5 |
| `wither` / `minecraft:entity.wither.hurt` | 5 | 6 | 5 |
| `giant` / `minecraft:entity.hostile.hurt` | 5 | 6 | 5 |
| `zombie` / `minecraft:entity.zombie.hurt` | 5 | 5 | 5 |
| `cow` / `minecraft:entity.cow.hurt` | 6 | 6 | 6 |
| `giant_death` / `minecraft:entity.hostile.death` | 5 | 6 | 5 |

Only the ten affected category ordinals change in the canonical contract; no
category mask or error normalization. Raw seeds/pitch/packet encodings/order
are preserved separately and are not claimed byte-equal across independent runs.

Immutable fresh Java reference SHA-256:
`f043ae5906dd9e9fabaa9ddd23255ceb0299e0bbe5b86e14fd80bf85b02a98c9`. Program, observer and reference hashes are verified
unchanged across old/fixed captures. Giant death raw snapshots show Health0 on
both sides. No source modification was needed outside the family table.

## Local checks and source checkpoint

Final production/harness:1517 renderer tests, formatting, workspace/server
Clippy, doc tests and public-tree checks pass. Python696 has four existing
macOS tooling failures (ARM installer-fixture assets and three Bash3.2 optional
archive cases), one skip. Narrow installer diagnostic retained; no packaging fix.

Later main1532c9c15 adds tick tracking/post-AI gates, LAN caching and player
metadata. Classifier/audio dispatch are unchanged; nearby NoAI hurt cases have
no pickup/search inputs affected by those post-passes. This is source review,
not a latest-base capture. Proof remains frozen69fab. Subsequent edits only add
evidence notes; final public-tree/whitespace checks pass.
