# Actual stored bell command identity

A directly placed bell now creates a dedicated, fieldless `BlockEntity::Bell` through the existing factory and collection path. The command getter is unchanged: it requires the real stored entry, then uses the existing common writer for the type id, Int coordinates and empty components. No raw/Mod substitute or command-specific coordinate answer is involved.

`BellBlock.newBlockEntity` constructs `BellBlockEntity`. That class has no additional saved state; inherited `BlockEntity.saveWithFullMetadata` supplies the measured identity. Existing ringing/server animation state remains in `be_anims`, with no new timer, ticker, sound, resonance, raid, memory or update policy.

## Proof

Run `tools/bell_block_entity_fixture.py` through the existing command harness: first `--reference-only --work-dir /fresh/java-path`, then the same script with `--binary /absolute/frozen-game --reference /fresh/java-path/vanilla.jsonl --work-dir /fresh/native-path`. Each directory must be new. The driver uses the existing pinned Java 26.3/prepared-resource route and does not rebuild a supplied binary.

The predeclared 147-record direct-bell contract guards loaded floor/support, exact north-facing floor attachment/powered state, real owner identity/Health/mode/position, and zero relative clocks. Two complete rounds read typed id/x/y/z and saved presence with fresh result/success sentinels. Chest block-entity/empty Items and stone non-block-entity controls are independent. Complete replies/styles/callbacks remain strict; full compounds and absolute clocks are predeclared raw diagnostics.

Frozen current main is `1d63e0a5a37a7a708eb0b32b49bf0dfeb28ed8a9`. The own pristine binary retained from H122 was hash-verified against that exact base and reused read-only, avoiding a redundant build. Fresh Java admits all 147 records; old Game matches **117/147**, fixed Game **147/147** with byte-identical scripts/references, with all 30 differences confined to bell metadata reads and their feedback/stores. All block-state/chest/stone/load/clock controls pass. The complete old differences accompany this document.

The real registered Game regression proves the placed bell's actual block state, then fails old because its real collection entry is missing. The same test passes fixed: actual model id/fits, repeated command getter, no-raw factory/common writer with different negative/positive coordinates, actual decode/save, orientation replacement, removal and recreation all use existing machinery. The chest survives those operations and the clock stays unchanged. This is component-level Game/model evidence beside the runtime command pair.

## Source reach and limits

The new variant is added to `for_block`, `fits`, `id`, empty `contents`/`slots`/`slots_mut`, the existing no-additional-data encode/decode arms and three no-menu exhaustive arms. Other variant bodies are unchanged. Existing constructor users include Game placement, loaded chunk decode, block/structure helpers and clone/removal/storage machinery. These source routes are not all live parity claims. The ordinary ringing pass and block-ticking classification are unchanged.

Runtime scope is the independently admitted **direct bell**. Generated placement, restart, packets, arbitrary extra fields/names and complete bell gameplay remain untested. No feature-placement repair or pending production import. The relevant actual diffs of merged 2992/9288 concern ringing/AI; open 11829 concerns shader velocity, not this identity repair.

All scripts/references, raw compounds/epochs, official bytecode, source/ownership audits, frozen binaries, hashes and known/null phase times remain protected in `.local/h174`. Disk preflight retains the 10 GiB growth gate and 5 GiB hard guard; no shared dependencies or other owners' artifacts were removed.

Separate complete raw traces: old matches 2/6, fixed 4/6. The four complete bell/chest body traces match fixed; the two absolute world-clock traces differ and stay unmodified. The strict gate remains the predeclared typed field/callback contract, with relative clock0.

## Required checks

The single required local run passed **4,997 Rust tests**, workspace/server Clippy, formatting, staged public-tree checks and doctests. Overall exit **1** is solely the three existing optional-release Python/Bash empty-array failures (`musl[@]`/`arm64[@]`), with 720 Python tests and two skips. The quick run listed 425 affected parity harnesses without executing them; the actual 147-record pair and resource-backed regression were run separately. No full-green claim.
