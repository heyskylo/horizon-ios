# Ordinary arrow first-tick shoot events

The existing `ArrowState` flag/event is the ordinary arrow shoot-event authority.
Game forwards its `ShootEvent` with the actual projectile owner/guest context, including
`finish_entity_hit`. Client `arrow_event` remains unchanged. The original marker remains
in the native impact event list, preserving its indices/ranges and unrelated event order.

Bow, crossbow-arrow and Mob-arrow launch adapters now defer their immediate post to that
first tick. This is an intentional event timing change; the saved pre-tick flag remains
false. Crossbow rockets and Mob tridents retain their immediate posts. No arrow movement,
hit, sound, damage, RNG or non-arrow producer logic changes.

The pinned 26.3 authority inspected was `Projectile.tick`, `spawnProjectile` and
`applyOnProjectileSpawned`: the tick posts once before marking the projectile shot;
spawning does not set that flag. The existing native `arrows::Ticker::finish` implements
that state/event transition and is unchanged.

## Reproduction

`tools/loaded_arrow_shot_fixture.py` is a manual driver using the existing command oracle
and integrated-server command route, not a new observer. It requires prepared local game
resources and a compiled Game binary. Run from the repository root:

```sh
python3 tools/loaded_arrow_shot_fixture.py --reference-only --work-dir /tmp/arrow-java
python3 tools/loaded_arrow_shot_fixture.py --binary /absolute/path/to/horizon-client \
  --reference /tmp/arrow-java/vanilla.jsonl --work-dir /tmp/arrow-native
```

Use fresh output directories. The native command reruns Java and checks that the reference
bytes match. The driver retains complete raw bodies and command feedback/styles/callbacks,
fresh result/success stores, exact relative clocks and its predeclared comparison labels.
It uses actual independently summoned stationary arrows with omitted/false/true flags,
a registered pig owner and a real charged-crossbow use. A pig hurt event independently
proves the real sensor is ready. Loaded stationary position/motion, item and owner are
strict; natural random UUID/position/motion and complete listener bodies/epochs/source UUID
are raw diagnostics. This does not claim full listener serialization or trajectory parity.

Frozen base: `2535dd9715803135a75aadad5cb2f3f641b783ab`. The old Game passes 485/516
records; all 31 differences concern loaded omitted/false event receipt or crossbow timing.
All admission controls pass. Fresh Java independently admits the same 516-record contract.
V1's 514-record reference remains retained; V2 improves actual selected inventory/ammunition
admission before the natural launch rather than treating missing Player equipment NBT as a
weapon read.

Resource-backed Game units invoke real registered loads, real launch adapters, projectile
ticks and hit finalization, using the existing posted-event recorder. They check the actual
model flag and owner, launch/tick1/tick2 counts, duplicate suppression, registered guest
context restoration and non-arrow rocket/trident controls. Bow and Mob launch coverage is
unit/source-only; the paired natural runtime capture covers crossbow arrows. No claim is
made for every owner kind, launch path, projectile family or native-hook cancellation.

All seven focused Game units pass on the fixed source. The original five-test run had four
meaningful failures and one passing non-arrow control; the separately added actual-owner
finish-hit test also fails old and passes fixed. The guest test's initial permission-0
setup failure and the rejected Vec3-conversion build are retained as invalid attempts.
The final guest fixture has actual permission4 and uses the registered guest's real
crossbow/owner state, without a fabricated source UUID. This is adapter/unit coverage,
not a connected-guest runtime capture.

World-RNG continuation after actual launch and two direct Game projectile ticks is unchanged:
bow `4488738784875719115`, crossbow `-9049827048921811932`, Mob adapter
`-5091662216707286027`. These seeded controls do not assert every random stream or exact
natural trajectory. The extracted owner-dispatch helper body is identical after indentation.
Later main `fd06d1c2650b52d8d4bb3d808ca73e55971ba717` only adds an unrelated terrain-model
module in the reviewed source set; the frozen pair remains on its declared base.

The complete old 31 differing comparison records are retained in
[loaded-arrow-shot-old-differences.jsonl](loaded-arrow-shot-old-differences.jsonl).
Private task archives retain the full reference/native command bodies, full sensor/arrow
traces, programs, build/unit logs and binary/source hashes. No binary, game asset or official
bytecode is committed. Unknown setup/proof start times remain unknown in the local manifest.

The final fixed Game passes **516/516** with byte-identical script and fresh-reference
files. All declared admissions pass. The final production source hashes match the frozen
fixed binary build inputs; no proof was rerun merely for later unrelated main changes.
The first-tick producer has one flag gate shared by ordinary ticking and hit finalization:
main-drain, breeze-deflection-drain and final-hit consumers forward the existing event;
a subsequent finish/tick emits none. The remote client drain still delegates to its
unchanged local adapter and does not become a server game-event producer.

## Local checks

The final required `pr_ready.py --quick --base 2535dd9715803135a75aadad5cb2f3f641b783ab`
run passes public-tree, formatting, 1,138 Rust tests and doc tests. It exits 1:
three pre-existing `test_optional_release_assets` cases fail in system Bash with
`musl[@]`/`arm64[@]` unbound variables; workspace Clippy stops at five lints in untouched
`horizon-core/src/density/interpolation.rs` (`manual_is_multiple_of` and
`chunks_exact_to_as_chunks`). No unrelated source repair or lint suppression is included.
Its 415 selected broad harnesses are listed, not run by `--quick`; the scoped runtime
516-record pair and seven resource-backed Game regressions were run separately.

Source reach also includes tipped/spectral ammunition represented by the same existing
`ProjectileKind::Arrow`; those ammunition variants are not separate runtime captures.
The single atomic state/event repair has substantial regression-fixture code; it does
not split the authority into independent launch and loaded-arrow policies.

The affected follow-up `cargo clippy -p horizon-client --all-targets --locked --no-deps --
-D warnings` passes. This checks the changed crate without linting the unrelated dependency
that blocked the required workspace Clippy run; the required run remains reported failed.
