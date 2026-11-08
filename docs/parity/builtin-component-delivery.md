# Four built-in item components in inventory packets

H283 / issue #12096. The current command-created stack was saved correctly but
`network_stack` omitted four built-in component types. The bridge now encodes
supported typed values into fresh private buffers using actual synced registry
IDs. Any unsupported child discards the whole component buffer. The existing
custom-registered component loop, stack model, saved writer and receive-side
semantic projection are unchanged.

## Frozen proof

Source base: `151aa6c82b45c1f725cb1dd3557b182390f35879`.
Fresh official Java 26.3 admitted 276 facts across 24 actual inventory slots.
The same program and one byte-unchanged reference give **272/276 before,
276/276 after**. Exactly four primary packet-component records change; all 272
other facts match. Real connected-player commands, UUID/Health/mode/position,
loaded floor, relative clock zero, fresh command stores, authoritative saved
item/count/component and actual slot receipts admit the inputs independently.

| Primary measured input | Actual packet result |
|---|---|
| Diamond providing named gold trim material | Component 64 with its named holder |
| Paper providing the globe banner-pattern tag | Component 67 with its tag holder set |
| Stone place-on dirt predicate | Component 14 with the complete block predicate |
| Diamond pickaxe break-stone predicate with empty state | Component 15, preserving present empty state |

The actual registered Game command → authoritative stack → Hub.send_inventory →
PlayerSender → complete protocol decode regression fails before (1 failed,
0 ignored, 8.96s) and passes unchanged after (1 passed, 0 ignored, 9.10s).
Two supporting tests pass: typed predicate/conjunct encoding with whole-form
rejection, and actual saved-stack removal-marker adoption. The latter is a
model-to-wire check, not evidence that commands preserve every removal marker.

The test-only client first uses the official packet codec, then consumes the
original bytes with each actual component's official stream codec. Both passes
consume the complete enclosing payload, including every stack and cursor.
Original value boundaries, full chronological proxy payloads, component order,
IDs, removals and complete saved player bodies remain available in local evidence.
No substring search, truncated decode or expected-payload synthesis is used.

## Typed payloads and limits

Fresh original Java bytes independently establish block-predicate NBT as Tag10
Compound, exact custom_name (component type 6) as Tag8, and partial custom_data
(predicate type 4) as Tag8 SNBT. The encoders preserve those separate codec paths.
The partial matcher is serialized from actual typed compound content through the
existing SNBT writer because this specific network codec requires a String.

| Reach or form | Coverage and remaining limit |
|---|---|
| Four primary forms above | Frozen actual connected-inventory runtime gate |
| Seven historical advanced H283 forms | Full supplementary packet dictionaries also match after; retained diagnostics, not an all-form guarantee |
| Measured state keys half/facing | Existing unchanged JavaHashMap with a private UTF-16 key adapter follows official map iteration; arbitrary collisions/input insertion order remain unproved |
| Unknown matcher/holder/field or unsupported text shape | Whole component unsupported; no partial positive output |
| Named armor trim and instrument | Positive unchanged controls |
| Fully inline armor trim | External H284 / PR #11642, still unclosed |
| Saved Adventure predicate canonicalization | External H285 / PR #11990, untouched |
| Command trim/banner removal markers | Lost before this bridge in the measured commands; retained prerequisite diagnostics, still unclosed |
| Explicit modeled removal markers | Supporting saved-stack adoption test; no generic removal/command bridge repair |
| Hub stacks and recursive containers/equipment/projectiles; remote fallback | Shared production reach, not independent runtime family coverage |
| Receive-side Raw semantic projection | Unchanged and unclosed |

All 20 diagnostic saved-stack typed objects compare exactly equal between old
and fixed native captures. This is typed-object equality, not a claim of raw-byte
body equality. Full raw body/order/epoch differences remain retained, including
armor-trim and removal prerequisites. `--strict-all` executes the full diagnostic
comparison and intentionally remains red on those unrelated differences.
No adventure action, tooltip/rendering, smithing, all-packet, restart, cross-version,
all-component or arbitrary predicate-order closure is claimed. No pending H284,
H285, H197 or command-removal patch was imported.

## Reproduction and evidence

Use `tools/test_builtin_component_delivery_parity.py --side java --binary <unused>`
for fresh official admission, then `--side native --binary <immutable-game>` for
that same program/reference. `--build` selects a persistent capture directory;
`--strict-all` includes all complete diagnostic records. The optional
`components on` test-client action enables full original component decoding.

Raw local evidence is retained under `data/h283/evidence/`: baseline/fixed
captures, full diagnostic comparison, original Java tag boundaries, source and
ELF hashes, phase times, first compile-only invalid unit draft and supporting
fixture-ID failure. A resume disk preflight stopped before engines. Only inactive
owned cache artifacts were retired after archive/SHA/fuser checks. A reused build
helper overwrote its earlier cleanup manifest with an empty list; the two original
binary archives and build logs remain intact. Their verified hashes/sizes and
reconstructed paths are explicitly marked as recovery metadata, not original
manifest bytes. Unknown phase times are null. These incidents did not alter proof
scripts, references, raw captures or production semantics.


## Final-check continuation

The original required quick check is retained red: Python720, both standard
Clippy commands, formatting, docs and Game1139 tests passed, but owned cache
cleanup accidentally archived a newly compiled bin-test executable before launch.
Its SHA-verified archive was restored. Recovery metadata errors and disk pauses
are retained; no source or proof data was lost. A Cargo target-enumeration attempt
began dependency rebuilding and was stopped before tests. The original target
fingerprints and exact executables then completed all17 skipped targets without
filters:2566 passed, including the full shared-client suite2146 passed/74ignored.
Together with the original Game run,3705 Rust tests passed/213existing ignored;
the passed Game suite was not repeated. Source, script, reference and meaningful
Game regression remained unchanged. One ambiguous short-class comment citation
was corrected with an exact byte inverse; only affected public-tree/format/ledger
checks followed, with the original ledger diagnostic retained.
