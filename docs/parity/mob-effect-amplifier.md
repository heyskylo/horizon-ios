# Mob effect amplifier numeric decode

The Mob active-effect load boundary uses `MobEffectInstance.Details`' unsigned-byte codec. Its primitive codec reads boxed `Number.byteValue`; `UNSIGNED_BYTE` then interprets the resulting byte as 0–255. This is truncation, wrapping and Java floating-to-integer saturation before byte narrowing, rather than NumericTag's scalar floor conversion. The existing boxed-number helper supplies that conversion without changing any shared helper.

Only the amplifier read changes. The existing Mob writer still saves a nonzero modeled amplifier as a signed Byte and omits modeled zero. Duration, effect flags, Player loading/saving, hidden chains, native hooks and effect behavior remain outside this repair.

## Reproduce

Run `tools/mob_effect_amplifier_fixture.py` through the existing actual Game/Java command harness, with `--reference-only` first, then `--binary /absolute/path/to/frozen-game --reference /path/to/vanilla.jsonl --work-dir /fresh/path`. Use the prepared pinned 26.3 resources and Java toolchain required by `test_command_exec_parity.py`. Each work directory must be new. The fixture driver does not build the game when a binary is supplied.

The declared contract has 838 strict records: 16 independent UUID-bound NoAI pigs with night vision, actual UUID/type/Health/Pos/NoGravity/floor/load guards, effect/duration/ambient/particles/icon reads, fresh result/success sentinels and relative clock zero. Inputs cover absent/zero, ordinary Int, signed Byte boundaries, Int256/-1, exact Long values beyond 2⁵³, finite fractions of both signs and finite overflow of both signs. Complete effect lists and full bodies are separate raw `command trace` records in the Java/native logs, without normalization. A cow without effects is an independent control.

Frozen base: `18840db9146b2521dd46ad52a4559a49be180f8f`. Fresh Java admits the final program; old native matches **814/838** and fixed native matches **838/838**, with all admissions passing. The 24 complete old mismatches are in `mob-effect-amplifier-old-differences.jsonl`. They concern only amplifier reads/presence and their stored results/successes.

The resource-free decode/model/save test asserts actual numeric model values, then saves with and without raw input and reloads. It fails old on Byte(-128) loading as -128 instead of 128. A separate writer mutation control overrides contradictory raw amplifier values and verifies default omission, signed Byte output, duration/flags and unrelated raw tags. That existing-writer control already passes old.

The original v1 program's aggregate selector counted four natural pigs alongside the 16 fixture pigs. Its complete program/reference/native capture is retained as rejected setup. V2 adds only the fixture tag to that aggregate count; it retains all numeric expectations and records. The initial wrong-package zero-test run and unit compile prerequisite are also retained as rejected test attempts, not passing evidence.

Private provenance, original programs, fresh references, full logs, binary hashes and bytecode evidence remain under `.local/h158`. Runtime scope is pigs/night vision at zero subject ticks. The source read is shared by existing Mob effect loaders; this is not all-effect/all-entity, simulation or restart coverage.

The fixed raw trace comparison is **6/35**, with 29 full-body/effect-list/absolute-clock differences retained verbatim in the private `raw-trace-comparison.json` and original logs. These raw traces were declared diagnostics before the pair and are not part of the 838-record gate. Typed amplifier, duration, flag, presence, actor and relative-clock checks remain strict. This is not whole-body/formatting parity.

Required final `pr_ready.py --quick --verbose --base 18840db9146b2521dd46ad52a4559a49be180f8f` ran once: **2,185 Rust tests passed**, Clippy (workspace/server), formatting, public-tree and doc checks passed. Overall exit **1** remains explicit: three known optional-release Python failures (`musl[@]`/`arm64[@]` unbound on macOS Bash), 719 run with two skips. The 414 listed parity harnesses were not run by `--quick`; the actual H158 Game/Java pair above was run separately.
