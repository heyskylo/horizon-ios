# Lodestone tracker saved true default

Pinned Java 26.3 `LodestoneTracker.CODEC` uses
`Codec.BOOL.optionalFieldOf("tracked", true)`: valid true is omitted when saved,
false remains explicit, and an absent target is independent of that default.
The narrow repair is in `encode_components`, the actual saved ItemStack boundary.
It removes only tracked Byte1 from that component's cloned saved compound.

The decoder, live StackData.other value and `components_tag` remain intact; the
latter also supplies client stack views. No renderer, packet or ticking parity
claim. Existing map-decoration namespace/prototype behavior is unchanged. No
Item ID hard-code, generic boolean conversion, registry/validation expansion,
useOn, inventory ticking, POI/unloaded logic or other component-default cleanup.

## Declared actual Game/Java fixture

```
python3 tools/test_lodestone_default_parity.py --binary /path/to/game --work-dir /scratch/run
python3 tools/test_lodestone_default_parity.py --full-diagnostic --binary /path/to/game --work-dir /scratch/full
```

`--reference /scratch/old/vanilla.jsonl` requires byte-identical fresh Java bytes
before comparing another binary. The existing command harness drives real Game
and independently runs pinned Java, with fresh isolated data and a real owner.
No new observer. Five actual stationary ItemEntities hold compass count 1, with unique
UUIDs/fixed entity names, NoGravity and zero Motion. They are away from the player
and have PickupDelay 32767. No world steps or living-holder inventory updates.

Cases: true+target, false+target, target without tracked, targetless explicit
true, and absent whole tracker component. Every case retains custom_data.keep 17.
A loaded real lodestone guards the codec-valid GlobalPos target. UUID/type,
Health/Age/Pos/Motion/NoGravity/Item ID/count, loaded floor/air, owner identity,
setup acknowledgements and actual relative clock 0 are strict positive controls.
Position/Motion/UUID/count are read again afterward. The targetless default must
stay as a present empty component, distinct from the missing-component control.

Whole saved component and tracked-field reads use fresh result/success sentinels
at -99. Actual data-presence branches have fresh independent sentinels and exact
callbacks/no-action behavior. Successful zero (false or empty compound) remains
distinct from a missing path. Target dimension/typed IntArray position and the
unrelated custom_data value stay strict.

Predeclared before native capture: **199** complete raw records. Only the five
whole entity bodies and absolute clock-start/end labels are diagnostic; they
remain traced, and `--full-diagnostic` compares all **206** records. No masks,
tolerances, feedback/style normalization or dropped commands.

Frozen source: `6d99e14ff0c70ae60bc56b67829b526409fa068e`.

| Program | Exact script SHA256 |
|---|---|
| Scoped199 | `761185d59bbc90249a393a19abbdd33342dee9af0cca2d7956837ff805e91094` |
| Full206 | `f803a10ec5e8fa78f663358d309ecf2a07bd41f764cb37979513735f1a2353a4` |

Old actual Game: **185/199**. The seven suffixes below differ in EACH of `true`
and `targetless`, yielding 14 target observations from this one saved-codec gap.
All false/absent/missing-component, actual actor, type/target/custom-data, clock
and other controls pass. Full old diagnostic: **188/206** (also both affected
whole bodies and two absolute clocks).

| Exact suffix | Java expected | Old actual |
|---|---|---|
| -component-read | tracker with default omitted | tracker retaining tracked:1b |
| -component-result | 1 with target / 0 targetless | 2 with target / 1 targetless |
| -tracked-read | missing-field error, callback false/0 | tracked:1b, callback true/1 |
| -tracked-result | 0 | 1 |
| -tracked-success | 0 | 1 |
| -tracked-presence | no action/no callback | action/callback true/1 |
| -tracked-presence-result | -99 | 1 |

## Reachable regressions and evidence

`lodestone_tracker_saved_default_decoded` and
`lodestone_tracker_saved_default_fresh_model` independently fail old at saved
tracked Byte1 versus omission. Both pass fixed. Each uses the real ItemStack
encoder, with false/absent and missing-component controls preceding the failure.
The fresh model has no original item NBT. Tests cover target/custom_data retention,
full saved ItemStack ID/count, unchanged decoded/live value and unsaved component
view, and a targetless empty component even without unrelated custom_data.

The final source applies the default only at encode_components, after reviewing
its shared helper's client consumers. Intermediate unit drafts and final old/fixed
logs are retained; no intermediate helper implementation is part of the runtime
pair or published production patch.

Complete programs, fresh references, raw styled feedback/callbacks, full bodies,
transcripts, pinned bytecode, source hashes and immutable binaries are retained
under the owner's `.local/h329`, without publishing game files. Available file
capture timestamps and logged stage durations are retained. Unobserved event
stage timestamps remain unknown.

## Fixed results and all full-diagnostic residuals

Final actual Game: **199/199**, versus **185/199** old, using byte-identical
scoped scripts and fresh Java reference bytes. Both final decoded and fresh-model
unit tests fail independently old and pass fixed. Full diagnostics improve
**188/206 → 204/206**. All five complete saved entity body records match in this
fixture. No full 206/raw-clock equality, general entity/packet or tick claim.

The full script is byte-identical across builds; independent full Java references
differ only at clock-start/end. Their non-clock records are identical. The two
remaining raw record differences are listed below. Actual relative delta 0 is an
independently strict admission; no captured absolute value is rewritten.

| Exact label | Java expected (old / fixed) | Native actual (both) |
|---|---|---|
| `clock-start` | `317 / 309` | `0` |
| `clock-end` | `317 / 309` | `0` |

Scoped Java reference SHA256 (both): `fbe623194668b9bf0639ff632198aa9045c61558c3744901ad804dde906eb28d`.
Full Java reference SHA256 old: `2f20e1c75aa35dba8a7c3e02a50ee31286af77650a5c4be67e2dfcc93c43144e`; fixed: `5d3af57ac2dbf287daed95cc9775ef7b8bce9fcdcba3113897eef93aa2829ea9`.

## Required checks and source checkpoints

`tools/pr_ready.py --quick --base 6d99e14ff0c70ae60bc56b67829b526409fa068e`
ran once on the final production/test patch: **70 item tests** and **25 content-root
tests pass**, with formatting, workspace/headless clippy, doc tests and public
checks passing. Jobs 2, incremental 0, Rust test threads 2 and isolated test homes
were used; prepared resources were actually available for the content-root suite.
Broad parity is skipped by --quick; the explicit actual Game/Java pair above ran.

Python: **690 run, 4 failures, 1 skipped** in unchanged ARM64/Bash 3.2 packaging
checks. The narrow install-signature diagnostic confirms the missing ARM64 mock
archive; optional archive subcases fail on unset Bash arrays. No packaging fix.
The ledger check flags a long regression helper (not a vanilla implementation)
and an inference-only DispenserBlock classification change; dispenser source is
unchanged and no dispenser behavior is claimed. No full LodestoneTracker codec,
registry or stream-codec implementation is claimed by this one-field repair.

Actual main was pulled through `2266c4de14561146044c422e6c525b3ce52b0f79` after
batches. Later mod-index, wire-stream/client input/tick-script and HUD changes
were reviewed. The item codec, command harness/driver and Java command oracle are
unchanged. The new optional tick-script route is separate from this fixture's
command-script route. These are source reviews, not combined latest-main runtime
claims: the paired proof remains on frozen 6d99. Only evidence text was appended
after required checks; final public-tree and whitespace checks were repeated.
