# Item frames with an absent Facing

The only gameplay change is the fallback in `item_frame_from_nbt`: DOWN (0) rather
than NORTH (2). The pinned 26.3 `ItemFrame.readAdditionalSaveData` reads
`Direction.LEGACY_ID_CODEC` and calls `orElse(Direction.DOWN)`.

Both ordinary and glow frames use the actual loader. Explicit directions 0..5,
including SOUTH (3), keep their existing behavior. The landed immutable-glow and
old-map-detachment code in `Game::apply_entity_nbt` is untouched. No missing
`block_pos`, invalid-direction, attachment lifecycle or map behavior claim.

## Reproduction and declared comparison

```
python3 tools/test_frame_facing_default_parity.py --binary /path/to/game --work-dir /scratch/run
python3 tools/test_frame_facing_default_parity.py --full-diagnostic --binary /path/to/game --work-dir /scratch/full
```

`--reference /scratch/old/vanilla.jsonl` requires byte-identical fresh Java output
before comparing a second binary. The driver uses the existing command oracle and
actual authoritative Game command route with a connected owner; no synthetic
frame state or new observer. Every run gets fresh isolated data.

Fourteen frames: each actual class crossed with omitted Facing and explicit 0..5.
Each has a unique fixed UUID, explicit `block_pos`, `Fixed:1b`, diamond count 1,
ItemRotation 5 and ItemDropChance 0.25f. Positive UUID/type, loaded chunk, stone floor,
air gap, owner UUID/Health/mode/Pos and command callbacks guard admission. Setup
settles before actors; after summons there are zero world ticks, verified by an
actual relative gametime delta of zero. No `/data` writes.

Predeclared before native captures: the scoped **198** complete raw command records
include every Facing/Pos observation, setup acknowledgement, class/UUID/block_pos,
Fixed/Item/ItemRotation/drop-chance and relative-clock record. The additional **30**
records are diagnostic from the start: both absolute clocks and each actor's
Rotation and full tag. Those commands remain executable and traced in scoped
mode; `--full-diagnostic` records and strictly compares all **228**. No projections,
style normalization or numeric tolerances are applied to the selected records.

Frozen base: `3c7bb44aa12407aff43fe8c9adab51a9b9ebc61c`.

| Program | Exact script SHA256 | Java reference SHA256 |
|---|---|---|
| Scoped198 | `5eb2c1e15f38b887b6eace1eb01206f1b8ceb249c45924f7f6b1559125662cfd` | `8a01196b9da48bec92c5f3470272c3b577d09d9ca7e3e710eaf178e5323b6c8f` |
| Full228 | `bb36f1aa4bf5976ed6421a2d3141195c34b72edf61007bdd390c6bf58771c157` | `1d43e476c7a2c3368bbd3634e7b204a34f724999a1894a5d905b5ba7c399b765` |

The unchanged Game passes **194/198**, with exactly these target failures:

| Label | Java | Old Game |
|---|---|---|
| item_frame-absent-Facing | 0b | 2b |
| item_frame-absent-Pos | [2.5d,102.96875d,4.5d] | [2.5d,102.5d,4.96875d] |
| glow_item_frame-absent-Facing | 0b | 2b |
| glow_item_frame-absent-Pos | [2.5d,102.96875d,10.5d] | [2.5d,102.5d,10.96875d] |

## Regression and retained evidence

`frame_missing_facing_loads_down_and_valid_directions_round_trip` calls the real
loader and saver for both actual classes, with and without raw NBT. Explicit
valid-direction controls precede the absent-field assertion. The old code fails
at `Facing` Byte2 versus Byte0; the identical test passes fixed, including the
DOWN-derived position, Fixed, Item, ItemRotation and drop-chance controls.

Complete programs, fresh references, old/fixed raw JSONL, full styled feedback and
callbacks, transcripts, reports, pinned bytecode, source/checkpoint hashes and
immutable old executables are retained under the owner's `.local/h134`. They
contain no inferred event timestamps. Capture/log filesystem timestamps are
preserved; unobserved stage timestamps remain unknown. Prepared resources/jars
are not published.

The first Java-only setup draft used item count2; it is retained separately and
was revised to count1 before either paired program. A draft unit missing a type
qualification failed compilation and was corrected before the old assertion run.
Neither is counted as a parity failure.

## Fixed result and complete diagnostic residuals

The fixed actual Game passes **198/198**, with byte-identical scoped script and
fresh Java reference bytes on old/fixed. All explicit-direction and item controls
remain exact. Full diagnostics are **198/228 old** and **202/228 fixed**; there is
no full-tag or 228-green claim. The full program is byte-identical across builds.
Its independent Java captures have different absolute startup clocks, so their
full references are not byte-identical. Their non-clock records are identical.
The scoped reference above excludes exactly the predeclared 30 diagnostic labels.

A first fixed full run attempted to reuse the old full reference and stopped on
its byte-identity assertion: Java clocks 325 versus 313, solely at clock-start/end.
That complete capture is retained. The independent full fixed reference below
has clocks 301; native clocks are 0. Each run's actual relative delta is 0. No clock
values were edited, replaced or subtracted inside raw captured records.

The 26 remaining full-record differences are listed below. For full-tag rows,
the table gives the differing field; complete bodies, styles and callbacks stay
in raw evidence. Every fixed full-tag residual differs only in Rotation. This is
the existing `item_frame_to_nbt` Rotation projection, separate from Facing default;
no encoder/rotation production repair is included. Old absent full tags also had
the Facing/Pos differences tabulated above.

| Exact label | Java expected | Native actual (old and fixed unless noted) |
|---|---|---|
| `clock-start` | `325 old / 301 fixed` | `0` |
| `item_frame-absent-Rotation` | `[0.0f, 90.0f]` | `[0.0f, 0.0f]` |
| `item_frame-absent-full-tag` | `Rotation: [0.0f, 90.0f]` | `Rotation: [0.0f, 0.0f]` |
| `item_frame-0-Rotation` | `[0.0f, 90.0f]` | `[0.0f, 0.0f]` |
| `item_frame-0-full-tag` | `Rotation: [0.0f, 90.0f]` | `Rotation: [0.0f, 0.0f]` |
| `item_frame-1-Rotation` | `[0.0f, -90.0f]` | `[0.0f, 0.0f]` |
| `item_frame-1-full-tag` | `Rotation: [0.0f, -90.0f]` | `Rotation: [0.0f, 0.0f]` |
| `item_frame-2-Rotation` | `[180.0f, 0.0f]` | `[0.0f, 0.0f]` |
| `item_frame-2-full-tag` | `Rotation: [180.0f, 0.0f]` | `Rotation: [0.0f, 0.0f]` |
| `item_frame-4-Rotation` | `[90.0f, 0.0f]` | `[0.0f, 0.0f]` |
| `item_frame-4-full-tag` | `Rotation: [90.0f, 0.0f]` | `Rotation: [0.0f, 0.0f]` |
| `item_frame-5-Rotation` | `[270.0f, 0.0f]` | `[0.0f, 0.0f]` |
| `item_frame-5-full-tag` | `Rotation: [270.0f, 0.0f]` | `Rotation: [0.0f, 0.0f]` |
| `glow_item_frame-absent-Rotation` | `[0.0f, 90.0f]` | `[0.0f, 0.0f]` |
| `glow_item_frame-absent-full-tag` | `Rotation: [0.0f, 90.0f]` | `Rotation: [0.0f, 0.0f]` |
| `glow_item_frame-0-Rotation` | `[0.0f, 90.0f]` | `[0.0f, 0.0f]` |
| `glow_item_frame-0-full-tag` | `Rotation: [0.0f, 90.0f]` | `Rotation: [0.0f, 0.0f]` |
| `glow_item_frame-1-Rotation` | `[0.0f, -90.0f]` | `[0.0f, 0.0f]` |
| `glow_item_frame-1-full-tag` | `Rotation: [0.0f, -90.0f]` | `Rotation: [0.0f, 0.0f]` |
| `glow_item_frame-2-Rotation` | `[180.0f, 0.0f]` | `[0.0f, 0.0f]` |
| `glow_item_frame-2-full-tag` | `Rotation: [180.0f, 0.0f]` | `Rotation: [0.0f, 0.0f]` |
| `glow_item_frame-4-Rotation` | `[90.0f, 0.0f]` | `[0.0f, 0.0f]` |
| `glow_item_frame-4-full-tag` | `Rotation: [90.0f, 0.0f]` | `Rotation: [0.0f, 0.0f]` |
| `glow_item_frame-5-Rotation` | `[270.0f, 0.0f]` | `[0.0f, 0.0f]` |
| `glow_item_frame-5-full-tag` | `Rotation: [270.0f, 0.0f]` | `Rotation: [0.0f, 0.0f]` |
| `clock-end` | `325 old / 301 fixed` | `0` |

Fixed full Java reference SHA256: `a82805f86496c65dd5b872bbc3d63a82cd71971e234eaa840d0dafd106b6977c`.

## Local readiness and source checkpoints

Required `tools/pr_ready.py --quick --base 3c7bb44aa12407aff43fe8c9adab51a9b9ebc61c`
ran once on the final production and test patch. Formatting, workspace/headless
clippy, public tree and doc tests pass. Rust: **1885 passed, 1 failed, 62 ignored**.
The failure is unchanged `entities::drop_motion_tests::drops_match_vanillas_motion_bit_for_bit`:
Z bits 4597448471913462162 versus 4597448471913462163 at yaw 37.5/pitch -21.25. The archived
old unit executable reproduces that exact failure; `entities.rs` is unchanged.
The added frame regression passes separately and in the full suite.

Python: **686 run, 4 failures, 1 skipped**. Existing release-packaging tests fail on
this ARM64 Mac: the install-signature mock only supplies an x86_64 archive, and
Bash 3.2 optional-archive subcases access unset musl/arm64 arrays. A narrow unchanged
install-signature rerun retains the exact missing `horizon-macos-arm64.zip` error.
No packaging repair is included. Broad parity was skipped by `--quick`; the
explicit actual Game/Java fixture above was run independently.

The ledger check reports direct-callee heuristics after adding the loader's
method citation; no broader ItemFrame method parity is claimed. Full map-check
also reports the pre-existing `PrimedTnt.explode -> Game::tick_tnt` stale mapping
at row 45 of the same TSV, identical at the frozen base. No unrelated map repair.

Actual main was pulled before/after batches. Checkpoints 83da211d3, 0f5dc8d6d and
4f4a931b21d37bc64ce77081bb0a674248f71ad7 add schematic/redstone test tooling only;
the frame loader, serializer, model, Game route and command oracle are unchanged
from the frozen proof base. These are source reviews, not combined latest-main
runtime claims. Only evidence text was appended after required checks; public-tree
and whitespace checks were repeated on the final staged tree.
