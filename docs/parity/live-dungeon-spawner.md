# Fresh live dungeon spawner creation (partial H173)

Successful live named dungeon placement now creates its missing actual spawner block entity and projects the already selected entity through the existing model/codec. The live sink verifies the spawner class, then uses an optional class-specific `LevelAccess::create_feature_spawner` callback. GameLevel checks the actual vanilla spawner block again and requires an absent collection entry before calling its existing load/decode/insert route. The getter is unchanged; no fabricated BE or mob choice is used.

**Existing entries of every class retain the old no-op behavior.** Existing configured/weighted spawners remain unsupported, not repaired. This avoids replacing their state or using the level RNG while placement owns a copied feature RNG. No new RNG, fields, event/ticker engine, spawn timing, weighted-state, loot/chest, packet, restart or all-spawner claim.

## Runtime contract

Use `tools/live_dungeon_spawner_fixture.py` through the existing command harness: first `--reference-only --work-dir /fresh/java-path`, then the same script with `--binary /absolute/frozen-game --reference /fresh/java-path/vanilla.jsonl --work-dir /fresh/native-path`. Each directory must be new. It uses the existing pinned Java 26.3/prepared-resource route and does not rebuild supplied binaries.

The predeclared **103-record** contract uses a solid floor/ceiling shell, interior cavity and single two-block door that admit either native room radius. Nine chunks and the actual target/neighborhood are loaded. Actual named feature placement ACK and the spawner block must pass independently before metadata comparisons. A direct typed spawner, ordinary chest, non-BE stone, real owner, untouched outside floor/ceiling and relative clock0 controls remain strict. No spawning ticks, generated-chest opening or loot rolls are used as proof.

Generated `mob_spawner` id, Int coordinates and SpawnData presence use fresh result/success stores. Three predeclared raw per-mob branch replies plus a strict in-world sum verify exactly one typed skeleton/zombie/spider value. Exact unseeded chosen IDs, full bodies and RNG outcomes remain raw diagnostics, without normalization or cross-run equality requirements. The mob is supplied by the existing feature's choice, not this adapter. Of nine predeclared raw traces, old matches two exactly and fixed matches three; six fixed residuals are the random choice/branch/full-body and absolute-clock records, all retained without rewriting.

Fresh Java admits the contract. Frozen pristine main `2ebf00a0281c592f79191e76fce0e7d522c6f425` matches **84/103**; all19 differences concern missing generated metadata/SpawnData and their stores. Placement/block/cavity/load/direct-codec/chest/stone/clock controls pass. The fixed Game matches **103/103** using byte-identical script/reference files. Complete old differences accompany this document.

## Model, preservation and RNG

The real registered Game unit installs actual live features, admits the named feature through the command dispatcher, verifies the block and then fails old because the real collection entry is absent. The frozen test passes fixed, checking actual modeled choice, no-raw typed saved projection and untouched direct spawner/clock. Appended defensive units preserve an existing spawner's SpawnData UUID, saved fields and unsaved spin/client delay; preserve a wrong-class entry and its inventory; and reject creation on stone. These boundary controls are source/unit evidence, not Java runtime parity for existing spawners. An unbound LevelAccess keeps the callback no-op, even when it supports ordinary loading.

The generation sink's existing NBT construction was mechanically extracted into one crate-private pure helper. Its fields/types/insertion order are unchanged, with binary-payload equivalence across multiple supplied entity IDs. GenerationWorld still delegates at the same position after its original state check. No natural-generation runtime claim is added.

The actual MonsterRoom feature body, named/table/chest callbacks, block write flags/order and feature RNG copy/commit path are unchanged. The initializer, load/decode/default constructor and insert make no world-random draw. Native seed173's old and fixed next four longs are exactly `[-2473094904728649983,-54486146291023519,1687810958285586684,-5413558800820152649]`. This proves bounded continuation, not distributions or Java random scheduling. Existing loaded/weighted state is deliberately never passed through a replacement initializer.

All raw attempts, scripts/reference, source/bytecode audits, binaries/hashes and known/null UTC phases remain protected in `.local/h173`. Rejected String-conversion/class-query compile attempts are build prerequisites, not runtime proof. Disk headroom was restored only by archiving verified inactive own executable cache with per-file/extracted/archive hashes and a restore manifest; protected proofs/refs and shared dependencies remain untouched. Growth10/hard5 GiB guards remain in force.

The read-only port-ledger check uses `--no-compare`. Its advisory missing-callee entries include whole-feature chest behavior and existing configured-spawner `setEntityId`/`setChanged`; those are deliberately outside this fresh-entry slice, rather than repaired or claimed here.

## Final local checks

Required `pr_ready.py --quick --verbose --base 2ebf00a0281c592f79191e76fce0e7d522c6f425` ran once on the final production patch: **1,647 selected Rust tests, Clippy, rustfmt, public tree and doctests pass**. Full exit is **1**: Python ran720, with the same three optional-release Bash empty-array failures (`musl[@]` masks0/1, `arm64[@]` mask2), two skipped. The482 affected parity harnesses were listed for CI, not run by `--quick`; the explicit103-record pair and resource-backed Game units above ran separately. Ledger `--no-compare` passed with scoped advisory gaps retained.

Required-check UTC: `2026-11-07T16:48:07.244854+00:00` to `2026-11-07T16:54:12.749475+00:00`. Other unrecorded phase start times remain null in the local ledger. No full-green claim.
