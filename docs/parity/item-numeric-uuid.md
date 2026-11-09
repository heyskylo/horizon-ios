# Item Owner/Thrower numeric UUID decoding

On frozen main `41f822b659789780f58d32c63ddfb6887ea1b8bf`, the actual Game command route matches **183/239** complete fresh Java 26.3 records before the fix. All positive setup guards pass. The fixed runtime passes **239/239** with identical script/reference bytes; required checks are recorded below.

`ItemEntity.readAdditionalSaveData` reads Owner through `UUIDUtil.CODEC` and Thrower through `EntityReference.codec`, based on the same UUID codec. `NbtOps.getIntStream` uses boxed `Number.intValue`: fractional values truncate toward zero, while Long narrowing preserves low32 bits without a floating round trip. `Util.fixedSize` supplies the first four values as an overlong-stream partial result, which `TagValueInput.read` retains. UUID strings are not this numeric codec.

The fix is caller-specific: reuse `horizon_world::datafix::dynamic::as_int_stream`, then retain four entries. The shared `save::uuid_from_tag`, existing writer cleanup, pickup/throw attribution, Animal references and other entity families remain unchanged. No pending H053/H057 code is imported.

## Executable proof

Run `python3 tools/test_item_numeric_uuid_parity.py --binary /absolute/path/to/horizon-client --work-dir /fresh/path`. Use `--reference-only` for Java alone and `--reference /path/to/vanilla.jsonl` to assert byte-identical fresh Java bytes. The fixture uses the existing command oracle and Game command adapter; no new observer.

Fourteen independent UUID-bound, named/tagged stone ItemEntities have Health5s, NoGravity1b, fixed Pos/zero Motion, custom item data and no observation ticks. Real owner UUID/type/Health/mode/Pos, loaded chunk, stone support/air and relative clock0 are positive controls. Each field has independent result/success resets before its typed read, stored results and a presence query. Owner-only/Thrower-only cases verify field independence.

All239 canonical records compare complete feedback/style/callback objects without masks, tolerances or normalization. Sixteen predeclared raw traces retain14 full bodies and two absolute clocks separately. The fixed diagnostic matches all14 whole Item bodies exactly. Its two absolute-clock traces differ: fresh Java309 versus Game0; the old run records Java313 versus Game0. Actual relative delta0 is strict in both. These exact raw values remain unmodified. No wider entity schema or all UUID-reference codec claim follows from these Item observations.

| Input (both fields unless noted) | Java saved reference |
|---|---|
| absent | absent |
| `[I;54,1,-2,3]` | `[I;54,1,-2,3]` |
| `[L;54L,4294967297L,-4294967298L,9007199254740993L]` | `[I;54,1,-2,1]` |
| `[54.9f,1.75f,-2.75f,-0.5f]` or Double equivalents | `[I;54,1,-2,0]` |
| Long numeric list with the LongArray values | `[I;54,1,-2,1]` |
| `[B;54b,1b,-2b,3b]` | `[I;54,1,-2,3]` |
| valid UUID String | absent |
| three-entry IntArray | absent |
| five-entry IntArray `[I;54,1,-2,3,4]` | `[I;54,1,-2,3]` |
| scalar7 / compound `{value:7}` | absent |
| Owner-only / Thrower-only | only supplied field present |

Two resource-free units call the real Item decoder, assert actual target/thrower model adoption first, then save with raw input and without any raw input. They cover numeric forms, malformed/absent forms, stale-raw clearing after modeledNone, and modeledSome replacing invalid raw. Old units fail at LongArray adoption and String rejection respectively.

## Every old canonical residual

All56 residuals are below; the other183 complete records match. Full styled raw JSONL, source scripts, reference, actual binaries/hashes, bytecode and diagnostic traces remain in `.local/h054/`.

| Label | Fresh Java expected | Old Game actual |
|---|---|---|
| `long_array-Owner-read` | h054_3 has the following entity data: [I; 54, 1, -2, 1]; callbacks=[[True, 4]] | Found no elements matching Owner; callbacks=[[False, 0]] |
| `long_array-Owner-result` | result has 4 [h054]; callbacks=[[True, 4]] | result has 0 [h054]; callbacks=[[True, 0]] |
| `long_array-Owner-success` | success has 1 [h054]; callbacks=[[True, 1]] | success has 0 [h054]; callbacks=[[True, 0]] |
| `long_array-Owner-presence` | Test passed. Count: 1; callbacks=[[True, 1]] | Test failed; callbacks=[[False, 0]] |
| `long_array-Thrower-read` | h054_3 has the following entity data: [I; 54, 1, -2, 1]; callbacks=[[True, 4]] | Found no elements matching Thrower; callbacks=[[False, 0]] |
| `long_array-Thrower-result` | result has 4 [h054]; callbacks=[[True, 4]] | result has 0 [h054]; callbacks=[[True, 0]] |
| `long_array-Thrower-success` | success has 1 [h054]; callbacks=[[True, 1]] | success has 0 [h054]; callbacks=[[True, 0]] |
| `long_array-Thrower-presence` | Test passed. Count: 1; callbacks=[[True, 1]] | Test failed; callbacks=[[False, 0]] |
| `float_list-Owner-read` | h054_4 has the following entity data: [I; 54, 1, -2, 0]; callbacks=[[True, 4]] | Found no elements matching Owner; callbacks=[[False, 0]] |
| `float_list-Owner-result` | result has 4 [h054]; callbacks=[[True, 4]] | result has 0 [h054]; callbacks=[[True, 0]] |
| `float_list-Owner-success` | success has 1 [h054]; callbacks=[[True, 1]] | success has 0 [h054]; callbacks=[[True, 0]] |
| `float_list-Owner-presence` | Test passed. Count: 1; callbacks=[[True, 1]] | Test failed; callbacks=[[False, 0]] |
| `float_list-Thrower-read` | h054_4 has the following entity data: [I; 54, 1, -2, 0]; callbacks=[[True, 4]] | Found no elements matching Thrower; callbacks=[[False, 0]] |
| `float_list-Thrower-result` | result has 4 [h054]; callbacks=[[True, 4]] | result has 0 [h054]; callbacks=[[True, 0]] |
| `float_list-Thrower-success` | success has 1 [h054]; callbacks=[[True, 1]] | success has 0 [h054]; callbacks=[[True, 0]] |
| `float_list-Thrower-presence` | Test passed. Count: 1; callbacks=[[True, 1]] | Test failed; callbacks=[[False, 0]] |
| `double_list-Owner-read` | h054_5 has the following entity data: [I; 54, 1, -2, 0]; callbacks=[[True, 4]] | Found no elements matching Owner; callbacks=[[False, 0]] |
| `double_list-Owner-result` | result has 4 [h054]; callbacks=[[True, 4]] | result has 0 [h054]; callbacks=[[True, 0]] |
| `double_list-Owner-success` | success has 1 [h054]; callbacks=[[True, 1]] | success has 0 [h054]; callbacks=[[True, 0]] |
| `double_list-Owner-presence` | Test passed. Count: 1; callbacks=[[True, 1]] | Test failed; callbacks=[[False, 0]] |
| `double_list-Thrower-read` | h054_5 has the following entity data: [I; 54, 1, -2, 0]; callbacks=[[True, 4]] | Found no elements matching Thrower; callbacks=[[False, 0]] |
| `double_list-Thrower-result` | result has 4 [h054]; callbacks=[[True, 4]] | result has 0 [h054]; callbacks=[[True, 0]] |
| `double_list-Thrower-success` | success has 1 [h054]; callbacks=[[True, 1]] | success has 0 [h054]; callbacks=[[True, 0]] |
| `double_list-Thrower-presence` | Test passed. Count: 1; callbacks=[[True, 1]] | Test failed; callbacks=[[False, 0]] |
| `long_list-Owner-read` | h054_6 has the following entity data: [I; 54, 1, -2, 1]; callbacks=[[True, 4]] | Found no elements matching Owner; callbacks=[[False, 0]] |
| `long_list-Owner-result` | result has 4 [h054]; callbacks=[[True, 4]] | result has 0 [h054]; callbacks=[[True, 0]] |
| `long_list-Owner-success` | success has 1 [h054]; callbacks=[[True, 1]] | success has 0 [h054]; callbacks=[[True, 0]] |
| `long_list-Owner-presence` | Test passed. Count: 1; callbacks=[[True, 1]] | Test failed; callbacks=[[False, 0]] |
| `long_list-Thrower-read` | h054_6 has the following entity data: [I; 54, 1, -2, 1]; callbacks=[[True, 4]] | Found no elements matching Thrower; callbacks=[[False, 0]] |
| `long_list-Thrower-result` | result has 4 [h054]; callbacks=[[True, 4]] | result has 0 [h054]; callbacks=[[True, 0]] |
| `long_list-Thrower-success` | success has 1 [h054]; callbacks=[[True, 1]] | success has 0 [h054]; callbacks=[[True, 0]] |
| `long_list-Thrower-presence` | Test passed. Count: 1; callbacks=[[True, 1]] | Test failed; callbacks=[[False, 0]] |
| `byte_array-Owner-read` | h054_7 has the following entity data: [I; 54, 1, -2, 3]; callbacks=[[True, 4]] | Found no elements matching Owner; callbacks=[[False, 0]] |
| `byte_array-Owner-result` | result has 4 [h054]; callbacks=[[True, 4]] | result has 0 [h054]; callbacks=[[True, 0]] |
| `byte_array-Owner-success` | success has 1 [h054]; callbacks=[[True, 1]] | success has 0 [h054]; callbacks=[[True, 0]] |
| `byte_array-Owner-presence` | Test passed. Count: 1; callbacks=[[True, 1]] | Test failed; callbacks=[[False, 0]] |
| `byte_array-Thrower-read` | h054_7 has the following entity data: [I; 54, 1, -2, 3]; callbacks=[[True, 4]] | Found no elements matching Thrower; callbacks=[[False, 0]] |
| `byte_array-Thrower-result` | result has 4 [h054]; callbacks=[[True, 4]] | result has 0 [h054]; callbacks=[[True, 0]] |
| `byte_array-Thrower-success` | success has 1 [h054]; callbacks=[[True, 1]] | success has 0 [h054]; callbacks=[[True, 0]] |
| `byte_array-Thrower-presence` | Test passed. Count: 1; callbacks=[[True, 1]] | Test failed; callbacks=[[False, 0]] |
| `string-Owner-read` | Found no elements matching Owner; callbacks=[[False, 0]] | h054_8 has the following entity data: [I; 54, 1, -2, 3]; callbacks=[[True, 4]] |
| `string-Owner-result` | result has 0 [h054]; callbacks=[[True, 0]] | result has 4 [h054]; callbacks=[[True, 4]] |
| `string-Owner-success` | success has 0 [h054]; callbacks=[[True, 0]] | success has 1 [h054]; callbacks=[[True, 1]] |
| `string-Owner-presence` | Test failed; callbacks=[[False, 0]] | Test passed. Count: 1; callbacks=[[True, 1]] |
| `string-Thrower-read` | Found no elements matching Thrower; callbacks=[[False, 0]] | h054_8 has the following entity data: [I; 54, 1, -2, 3]; callbacks=[[True, 4]] |
| `string-Thrower-result` | result has 0 [h054]; callbacks=[[True, 0]] | result has 4 [h054]; callbacks=[[True, 4]] |
| `string-Thrower-success` | success has 0 [h054]; callbacks=[[True, 0]] | success has 1 [h054]; callbacks=[[True, 1]] |
| `string-Thrower-presence` | Test failed; callbacks=[[False, 0]] | Test passed. Count: 1; callbacks=[[True, 1]] |
| `long-Owner-read` | h054_10 has the following entity data: [I; 54, 1, -2, 3]; callbacks=[[True, 4]] | Found no elements matching Owner; callbacks=[[False, 0]] |
| `long-Owner-result` | result has 4 [h054]; callbacks=[[True, 4]] | result has 0 [h054]; callbacks=[[True, 0]] |
| `long-Owner-success` | success has 1 [h054]; callbacks=[[True, 1]] | success has 0 [h054]; callbacks=[[True, 0]] |
| `long-Owner-presence` | Test passed. Count: 1; callbacks=[[True, 1]] | Test failed; callbacks=[[False, 0]] |
| `long-Thrower-read` | h054_10 has the following entity data: [I; 54, 1, -2, 3]; callbacks=[[True, 4]] | Found no elements matching Thrower; callbacks=[[False, 0]] |
| `long-Thrower-result` | result has 4 [h054]; callbacks=[[True, 4]] | result has 0 [h054]; callbacks=[[True, 0]] |
| `long-Thrower-success` | success has 1 [h054]; callbacks=[[True, 1]] | success has 0 [h054]; callbacks=[[True, 0]] |
| `long-Thrower-presence` | Test passed. Count: 1; callbacks=[[True, 1]] | Test failed; callbacks=[[False, 0]] |

## Final validation

The fixed runtime matches all239 strict records with zero residuals. The old runtime has56 residuals above; all setup controls and183 other records match. Both full diagnostic captures remain executable/raw: old7/16 and fixed14/16, where each total comprises14 bodies plus two absolute clocks. Fixed bodies all match; only the exact clock values above differ.

Required checks ran once on the final production patch: `python3.12 tools/pr_ready.py --quick --verbose --base 41f822b659789780f58d32c63ddfb6887ea1b8bf` (UTC2026-11-06 16:13:19–16:26:20). The suite reports2082 Rust tests passed, including both new tests; integration/doc tests, rustfmt, clippy, public-tree and unchanged Cargo.lock checks pass. Python runs708 with three pre-existing `test_optional_release_assets` failures (masks0/1: `musl[@]`; mask2: `arm64[@]` unbound Bash arrays) and one skip, identical to the prior H190 run. Overall `pr_ready` exits1; no all-checks-green claim. The382 broad affected parity harnesses are listed for CI by `--quick`; the focused actual Game pair ran locally.

The current pinned-jar ledger reports two retained callee-coverage gaps (`Entity.discard` and `EntityReference.read`) on the newly cited Item reader. No generic discard or EntityReference engine is added, and no wider closure is claimed. Pinned bytecode and actual Item state establish this bounded numeric-codec route.

Workflow note: the repository ledger's baseline comparison internally created and automatically removed a temporary analysis worktree. No build or task edits ran there. Only the persistent checkout remains; future ledger checks will use `--no-compare`. The protected perf checkpoint and proof resources are unchanged.

Program SHA256: `d2891333ce0ed8d2c88581511febcd047626ce37bba293284846c8b12a1992e7`. Fresh Java canonical reference SHA256: `8429f9e379740d8db31ab30a7931bc3ee83dd225f1a245c46d3fae1919634b88`.
