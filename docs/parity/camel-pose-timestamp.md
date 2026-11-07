# Loaded Camel pose timestamps

Own frozen base `13f70ad482df0a548e1954df8da4b967be3183cb`, actual Game and fresh pinned Java26.3: **152/170 →170/170** complete command records. All setup, actor, field-type, feedback/style/callback, fresh store and clock controls remain strict. The [complete old residual table](camel-pose-timestamp-residuals.md) retains every one of the18 differences; fixed has none in this contract.

## Common clock and cases

The existing command harness freezes ticking, prepares one loaded stone floor/clear area and establishes a real common game clock1024 before actors. It conditionally schedules one real tick while the observed clock is below1024, at most1024 attempts, using the existing command/tick route. No world clock is assigned directly and no captured value is normalized. Fresh Java validates the complete program before the native pair. Both builds use identical emitted script/reference bytes.

The initial setup clocks differ and remain exact in raw traces. After real alignment, absolute1024 and1025 and actual relative0/1 are canonical strict records. Five independent UUID-bound NoAI/NoGravity persistent adult camels have Health32 and individually guarded positions, floor, type, identity and saved flags. Exactly one declared actor tick separates samples.

| Loaded LastPoseTick | Java at clocks1024/1025 | Untouched Game | Fixed Game |
| --- | --- | --- | --- |
| Absent | 0L /0L | 971L /971L | 0L /0L |
| 0L | 0L /0L | 971L /971L | 0L /0L |
| 1000L | 1000L /1000L | 1000L /1000L | 1000L /1000L |
| -1000L (sitting) | -1000L /-1000L | -1000L /-1000L | -1000L /-1000L |
| 1044L (future) | 1044L /1044L | 1024L /1024L | 1044L /1044L |

Long type/presence, full scalar feedback and result/success stores are independent observations. No failed summons or rejected setup reads count as timestamp failures.

## Source boundary

Pinned `Camel.readAdditionalSaveData` reads LastPoseTick with0 fallback and resets the synchronized value. `Camel.getPoseTime` is actual game time minus abs(timestamp), including negative elapsed for a future tick. `Camel.addAdditionalSaveData` always writes the Long.

The actual Game loader already supplies its world clock to `camel::finish_load`, and the existing Game saver supplies its clock to `synced_pose_tick`. The repair only preserves the signed elapsed value in the existing mob-age conversion: zero does not call stand-up-instantly, and negative elapsed is not clamped to zero. The existing upper bound for very old elapsed values remains. No field/API/writer change or output-only correction is added.

Existing fresh-spawn, sit/stand mutation, client animation, readiness predicates and RNG calls stay unchanged. The source helper reaches Camel and CamelHusk; runtime coverage here is ordinary Camel only. H093's independent Camel legacy temper path is retained without importing its pending production.

## Regression and diagnostics

A registered Game load/save/tick test uses128 real pre-actor ticks, then the same five loaded cases. It first proves actual decoder adoption, keeps UUIDs and Camel legacy temper17, checks sitting/readiness, saves with and without raw input, and admits one actual entity tick through the model age and world clock. Untouched fails at the intended saved0 assertion (actual75); fixed passes. The identical common test is used in both unit artifacts. Existing pose mutation/readiness units remain part of required local tests.

All20 actor body/motion trace records are retained: old10/20 and fixed10/20 match completely (the10 motion records match; complete bodies have separate existing differences). No full-body, random trajectory or complete raw-stream parity is claimed. All2049 pre-alignment command traces retain exact absolute times and actual step outcomes; those deliberately different setup histories are not a parity gate.

Rejected prerequisites stay executable/raw under `.local/h094`: the first Java draft used an already-air clear ACK and invalid UUID selector suffixes; the first Game unit tried an unavailable `@s` teleport; an initial source query lacked the selected JDK on PATH. None is a qualified timestamp baseline. Routine setup corrections preceded the qualified proof.

## Reproduce and evidence

```sh
PYTHONPATH=tools python3 tools/test_camel_pose_timestamp_parity.py --reference-only --work-dir .local/h094/new-java
PYTHONPATH=tools python3 tools/test_camel_pose_timestamp_parity.py --reference .local/h094/new-java/vanilla.jsonl --binary /absolute/path/to/game --work-dir .local/h094/new-game
HORIZON_TEST_RESOURCES=/absolute/path/to/prepared/26.3 cargo test -p horizon-client --lib loaded_camel_pose_timestamps_survive_actual_save_and_tick -- --ignored
```

Run games/tests with fresh scratch homes and working directories as required by AGENTS.md; the harness supplies isolation for its runs. The public wrapper has identical operations to the preserved paired program; its descriptive metadata clarifies that common absolute clocks are strict. It makes no change to captured records.

- Paired program source SHA256: `6553bfa270cf07a2881933289e126dde4851d57f4ee411982076b5c11cd970e9`
- Emitted script SHA256: `e3f0307feaeef15007b4692bcd20a7e603fdc78cd7554a85373c95058d91c7ce`
- Java reference SHA256: `fd50b212d88772c48aff4cbaf1f630c44b545f5fcdba8df54845bf51a93c3d5e`
- Pristine13f old binary SHA256: `c462c800fb8effe46bc35698d10624606fe5bba4c145c47847e10c14dae083e2`

Original scripts, declarations, references, complete raw outputs, invalid attempts, binaries and manifests remain private under `.local/h094`. This covers bounded loaded timestamps and one actual tick, not extreme numeric values, unloaded-clock lifecycle, reload, passengers, physics or all animation policy.
