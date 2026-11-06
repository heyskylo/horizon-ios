# Ominous item spawner warning source (H128)

Official pinned Java26.3 OminousItemSpawner.tickServer chooses SoundSource.NEUTRAL
explicitly for block.trial_spawner.about_to_spawn_item. At a loaded delay40, its
comparison tickCount == spawnItemAfterTicks -36 warns at tick4. The BlockPos overload
uses the block center and volume/pitch1. Own bytecode and port ledger were reviewed.

Native used Audio::play, whose settings::sound_source infers a category from the event
family; this particular block.trial_spawner name resolves to BLOCKS4. Audio::play does
not globally default to blocks. Only this warning call changes to play_on(6,...).
Global inference, other audio, levelEvent3021, H065 extra local sounds, particles,
items, delay/expiry and rendering remain unchanged.

## Frozen actual Game/Java route

Base5849fa33b306beae3f218d03694f4e17497d59c4. The existing dedicated-server and
LocatorBarClient helpers run a fresh pinned Java server and actual headless Game server
against copies of a Java-created flat world. A small opt-in sounds decoder uses the
existing native packet codec and records typed ClientboundSoundPacket fields plus the
complete packet payload. It adds no packet framework and is disabled by default.

Actual nearby Alice is identified by her offline UUID, type, Health20, mode1 and
Pos[4.5,100,4.5]. The actual loaded spawner has UUID[I;128,0,0,1], type,
Pos[6.25,102.75,4.875], item diamond1 and spawn_item_after_ticks40L. Positive actor,
player, loaded chunk, stone floor and air controls run at every observation boundary.
An independent /playsound neutral note-block control proves the observation route and
neutral source value before the actual spawner warning; it does not manufacture a warning.

The declared program advances actual world deltas0/1/3/4/4/5, with frozen-world command
receipts before steps and after each observation. Both absolute game clocks and exact
relative deltas are retained. There is no warning at0/1/3, exactly one at4, none during
the immediate same-tick repeat or at5. It is not a wall-wait or expiry/drop trial.

| Field | Fresh Java | Untouched Game |
|---|---|---|
| Event | minecraft:block.trial_spawner.about_to_spawn_item | same |
| Center | [6.5,102.5,4.5] | same |
| Volume/pitch | 1.0/1.0; bits1065353216 each | same |
| Warning boundary | actual world tick4 | same |
| Category | NEUTRAL6 | **BLOCKS4** |

The strict contract names stage, event, source ordinal/name, position, volume/pitch and
both float-bit values. These fields are compared exactly, without tolerance or repaired
labels. The expected two records are the neutral control and the actual warning; empty
windows and duplicate rejection are enforced too. All sound seeds and payload bytes,
other sound records, full chats and entity bodies remain raw diagnostics. No random-seed,
full-packet-byte, broad delivery/packet-family or audible playback parity is claimed.

Available wall/monotonic stage receipt timestamps and server logs remain unchanged.
The actual sound emission timestamp is unknown and recorded as null, not invented from
a receipt time. Absolute startup clocks are diagnostic, not normalized into equality.

The first Java draft observed the correct sound but rejected successful block/load guards
because its checker incorrectly expected the entity count reply for every guard. That
capture/program/log is retained under .local/h128/draft1 and java-draft1 as a rejected
prerequisite, not a qualified native baseline. Exact per-kind guards and an explicit
pre-step receipt barrier were declared and freshly Java-validated before the real pair.

The reachable Game regression loads a real spawner through its production NBT route,
checks identity/item/delay, and calls the actual warning-dispatch tick from0 through5.
It verifies no early warning, block center and exact float bits, one warning at4 and no
duplicate at5. Independent inferred-block and explicit-neutral controls precede the old
intended4-versus6 assertion; a later block sound checks source-override restoration.
This unit is a Game dispatch test; the dedicated-server pair supplies the real world ticks.

Replay with prepared resources and isolated test homes/cwd as required by AGENTS.md:

```sh
python3 tools/test_ominous_warning_sound_parity.py --binary /path/to/game --reference-only --work-dir .local/h128/reference
python3 tools/test_ominous_warning_sound_parity.py --binary /path/to/game --reference .local/h128/reference/reference.json --work-dir .local/h128/replay
HORIZON_TEST_RESOURCES=/absolute/resources/26.3 cargo test -p horizon-client --lib warning_dispatch_uses_neutral_at_tick_four -- --ignored --nocapture
```

Complete raw sounds/payloads, all transcripts, actor/body guards, stage timestamps, clocks,
worlds, bytecode, declarations, logs and immutable binaries stay under .local/h128.
No Mojang files or prepared resources are committed.

## Declared hashes

| Artifact | SHA-256 |
|---|---|
| Program before paired runs | `d539ab028d1b6f5fbf65ecbdf93cfd85ac33a0cc9b9fd33b29436d77c7c0b5c4` |
| Fresh strict Java sound reference | `cc293f8619552459f0a7f1b1fc27a4c7769bf281ab25d7321ff997f9d9508d25` |

## Paired result

Untouched Game passes the independent sound control but the warning category is
BLOCKS4. Fixed Game matches both declared sound records exactly: controlNEUTRAL6 and
warningNEUTRAL6, including timing, center and exact volume/pitch bits. Both paired
programs and independently regenerated strict Java reference files are byte-identical.
All actor/player/geometry/clock admissions pass. Raw seeds/payloads and timestamp
diagnostics remain complete and unnormalized; they are not asserted byte-equal.

## Local readiness and later source review

The resource-backed Game regression executes one test and passes fixed, after its
intended old category4-versus6 failure. Required pr_ready --quick ran once on the final
production/test patch: formatting, workspace/headless clippy,1015 Rust tests, doc tests
and public tree pass. Python ran686 tests, with four existing macOS packaging failures
and one skip: the signature mock lacks the ARM64 archive and optional-archive masks0/1/2
hit unset Bash3.2 arrays. These files are unchanged; no packaging repair is included.
Focused Java/native network captures and the ignored Game regression were explicitly
run. --quick does not claim the broad parity suite. Only evidence docs were finalized
after checks, followed by public-tree and whitespace verification.

Actual main pulls reached b0a738abc and then
3c7bb44aa12407aff43fe8c9adab51a9b9ebc61c. The warning producer, audio API/inference,
sound packet builder/codec and existing typed-client baseline are unchanged. Other
cosmetics/launcher and profiling changes were reviewed; LAN changes wrap existing calls
with timing and add wire byte counters, without changing this sound's category or order.
The measured pair remains frozen5849, not a combined latest-main runtime claim.
