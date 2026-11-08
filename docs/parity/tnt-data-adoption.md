# Live TNT command data adoption

Own frozen main `41f822b659789780f58d32c63ddfb6887ea1b8bf`, actual Game/fresh pinned Java26.3: **92/100 → 100/100** complete strict records. The same protected script/reference bytes are used across the pair. No masks, tolerance, callback normalization or row selection.

## Actual command proof

Two independent fresh TNTs, UUID-bound, stationary with NoGravity, Short fuse40, Float power2 and a reference to an actually registered Pig. Actual TNT type/UUID/position/motion/fuse, floor/load, rotation, owner, block-state, tags and Silent are read before/after. The actual host's UUID/type/Health/mode/position and strict relative-clock0 are positive controls. A plain Pig Silent update proves the ordinary data route works.

An unchanged update is executed against each independently admitted initial TNT state BEFORE its target mutation. No failed reset supplies setup. Target changes are Short fuse40→60 and Double motion `[0,0,0]→[0.125,0,-0.25]`, with no observation ticks. Every action has fresh result/success sentinels and complete feedback/styles/callbacks. Full saved bodies and absolute clocks are separate predeclared raw traces.

Eight old records differ; all other records and every setup/control match:

| Labels | Java expected | Untouched Game actual |
|---|---|---|
| fuse-update, motion-update | modified TNT feedback; callback true/1 | unchanged/failure feedback; callback false/0 |
| fuse-update-result, fuse-update-success, motion-update-result, motion-update-success | stored1 | stored0 |
| fuse-saved-fuse | Short60 | Short40 |
| motion-saved-Motion | exact Double vector `[0.125d,0.0d,-0.25d]` | `[0.0d,0.0d,0.0d]` (both length-result3) |

## In-place boundary

`apply_entity_nbt` lacked a TNT arm. The patch mutates the existing registered instance with current position/motion/rotation/flag/power/block-state converters and the existing Short fuse reader. It includes TNT in the boundary's normal raw-tag bookkeeping and keeps the actual UUID. `Entity.load` resets previous position to the loaded position; no world tick occurs.

`load_tnt` is deliberately not called: it allocates an id/join, handles passengers and initializes runtime ownership. Existing runtime id/join, owner/guest binding and world clock stay intact. No field/schema, spawn route, global decoder, shared numeric helper, formatter, physics or explosion-policy change.

H090 PR11121 and H091 PR11071 actual diffs were inspected, with no pending production imported. Their fuse numeric-load policy and default-power saved-key cleanup are separate. This proof uses a valid Short and nondefault Float2, and does not claim all numeric forms or H090/H091 closure. Preserve their loader/writer changes and tests when combining; keep this independent live adoption arm and test. Owner editing, attribution-policy, malformed input, generic Entity-load limits and cross-dimension/lifecycle behavior are not claimed.

## Actual Game regression and prerequisites

The registered Game command test admits the TNT model/UUID, unchanged-update result and a real owner Mob reference before asserting a valid update. Untouched fails at that actual command. Fixed passes verification of real modeled fuse/motion, unchanged runtime identity/join/owner/guest/next-id/clock, saved types/fields and supported raw-tag adoption. It does not supply a synthetic observer answer.

The existing prepared-resource unit helper has a resident origin chunk, not a full ticket/streaming-readiness fixture. Runtime readiness remains strict in the independent100-record proof. Retained rejected unit prerequisites: a private-module import, a duplicate force-load assertion and an inappropriate streaming-ready assertion on that resident-only helper. They are not target failures. The first misplaced production draft was caught in the getter match, preserved, and its own obsolete build stopped before any runtime capture; the valid edit is solely in the adoption match/bookkeeping.

Public replay `tools/test_tnt_data_adoption_parity.py` is byte-identical to the protected original. Complete raw/style/callback captures, invalid attempts, source/artifact hashes and known phase times remain in `.local/h089/`; unknown times are null. Required local run passed1,078 Game tests, rustfmt, workspace/server Clippy, public-tree and lock checks. The explicit prepared-resource Game regression passed (one actual test). Overall required-run exit remains nonzero for the three existing optional-release Python Bash-array failures (`musl[@]`/`arm64[@]: unbound variable`,708run,one skipped); no overall green claim. The382 broader harnesses were listed for CI, not run locally. One persistent checkout/git switch, protected perf checkpoint, no worktrees.

## Raw diagnostics

Eight predeclared raw traces remain complete: old2/8, fixed4/8 match. All four full TNT before/after bodies match fixed. The two Pig bodies retain unrelated differences, and absolute clocks remain old Java301/Game0 and fixed Java321/Game0 at both start/end. Relative delta0 stays strict; no raw-clock or all-entity-body equality claim. Original Java/native/reference/style/callback outputs remain protected.
