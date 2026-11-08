# Literal random chance in command predicates

The actual command predicate context evaluates numeric literal `random_chance` leaves using the active world's RNG. The chance is converted to f32 before one unconditional `next_float() < chance`, including chance zero and one. Arrays, inversion, all_of and any_of retain their existing order and short-circuit behavior.

Only `Game::check_predicate` supplies this context. Its single mutable-Game callback creates an immutable `View` only while evaluating an ordinary leaf; that borrow ends before the next random leaf. Entity views are owned before evaluation. No RNG is cloned, removed, replaced, reseeded or stored by production. Existing `conditions` callers keep their old no-RNG behavior through the mechanically extracted traversal and original leaf dispatch. This source reach includes advancements, selectors, modifiers and native criteria, but those consumers do not gain command RNG support.

This is **partial H122**: direct top-level named predicates and inline logical forms with numeric literal chances. Nested string references require the independent H121 binding. Dynamic chance-provider objects and additional unsupported condition fields retain prior behavior, without evaluating a partial shape or drawing. No reference graph/world admission, provider engine, distribution, random schedule, all-advancement or all-loot claim.

## Reproducer

Use `tools/named_random_chance_fixture.py` with the existing command harness. First run `--reference-only --work-dir /fresh/java-path`; then run the same script with `--binary /absolute/frozen-game --reference /fresh/java-path/vanilla.jsonl --work-dir /fresh/native-path`. Each output path must be new. The fixture uses prepared resources and the pinned toolchain from `test_command_exec_parity.py`.

The predeclared 309-record direct contract admits a valid real pack/function and actual player and fixed-UUID pig. Health, type, position, NoAI/NoGravity, floor/load and zero relative clocks are independent controls. For both sources, both signs of direct zero/one, inline inversion/all/any and a deterministic entity predicate retain complete feedback/styles/callback/result/success and branch score sentinels. Full bodies, absolute clocks and pack-list replies are retained raw diagnostics.

Frozen main `1d63e0a5a37a7a708eb0b32b49bf0dfeb28ed8a9`: pristine Game matches **245/309**, fixed Game **309/309**, using byte-identical script/reference. All 64 differences concern random-chance branch outcomes and their feedback/stores. Independent setup, entity predicate and clock controls pass. A registered Game regression first proves real pack function and selected pig identity, then fails old on chance zero's missing RNG draw despite its correct false branch.

## Official seeded controls

`random-chance-seeded.jsonl` contains 33 complete official Java 26.3 observations: seeds 1/122/-1, literal zero/one/0.5/0.1/two rounding boundaries, inversion and reached/short-circuited all/any. Each result includes four following longs. `mods/horizon/src/test/java/dev/horizonlab/parity/VanillaNamedRandomChanceOracle.java` is the small dev-only probe (compile against the pinned server-game jar and its libraries). It parses each actual official condition codec and invokes the official predicate with its real RandomSource inside a constructor-created LootContext. Its other context dependencies are null because these constant conditions never access them. This is a pure codec/evaluation control, not a loaded-world or packet claim. The actual Game unit independently uses real pack lookup and actual world RNG.

Pinned `ExecuteCommand` creates a LootContext with no seed or sequence override; `LootContext.Builder` falls back to ServerLevel.getRandom. `LootItemRandomChanceCondition` evaluates its provider before always drawing. Unsupported providers are not implemented here. Existing H121 reference input/proofs remain protected separately; no pending production was imported.

Private `.local/h122` retains original scripts, raw outputs, compile attempts, source/bytecode audits, frozen binaries and hashes. The initial Python-3.9 harness failure and incorrect test import are rejected prerequisites, not runtime failures. Known phase times are recorded; unknown times remain null.

The original seeded probe has 30 observations; the final 33 add a decimal just above the seed-1 float that rounds to the same f32 value, requiring false rather than a double-precision comparison. Both probe versions and original unit results are retained.

The actual registered Game test passes all final 33 seeded observations against the same shared comparison and active world RNG; the original 30-case old failure and fixed pass remain preserved. The affected library run passed 44 tests (one ignored). Required checks separately rerun those pure controls with the final 33-case fixture. Of five predeclared raw trace records, old and fixed each match one; the four full-body/absolute-clock differences remain unmodified diagnostics. No all-body or absolute-clock parity is claimed.

The ledger reports the cited `ExecuteCommand.checkCustomPredicate` as stale, while pinned private bytecode contains that exact method. Both evidence files are retained. Traversal/leaf extraction equivalence was checked before the pair: the leaf body is byte-identical after removing only the extracted branches.

Integration note for independent H121: retain its named lookup and defensive graph validation when combining the changes, while preserving this same-context callback/draw order. This branch does not import that pending implementation or claim nested-reference support.

## Required checks

The single required local run passed **3,348 Rust tests**, workspace/server Clippy, formatting, public-tree checks and doctests. It exited **1 solely for three existing optional-release Python/Bash empty-array failures** (`musl[@]`/`arm64[@]`): 720 Python tests, two skips. The quick run listed 423 affected parity harnesses without running them. The actual 309-record pair and resource-backed Game regression were run separately. No full-green claim.

Publication followup: the commit hook rejected the original dev-oracle path under `tools`. It was moved to the existing allowed original mod-test source path; the packaged oracle reproduced all 33 observations and the staged public-tree check passed. Rust production and the paired proof were unchanged. The original required run was not repeated.
