# Note-block instrument normalization during fill

Non-strict Note-class fill inputs now use the existing `block_input_state` helper against the actual Game level before placement. That helper normalizes neighboring shapes, falls back to the input when normalization returns AIR, and restores explicit properties. Its body is unchanged. A non-strict Note input also bypasses the raw-state equality shortcut: an existing strict `harp` block above stone must become `basedrum`, even though the input default is `harp`.

Strict mode and all non-Note fill paths keep their existing behavior. Hollow interior AIR is not normalized as a note block. The source change reaches non-strict Note-class targets across fill modes; runtime proof below covers single-block ordinary/keep/filtered fills on stone or air, strict/explicit controls and setblock counterparts. Destroy/outline/hollow, other supports/top instruments, derived/mod classes, other block properties and all-fill/redstone behavior are not runtime claims.

## Reproduce and evidence

Use `tools/note_block_fill_fixture.py` with the existing prepared Java 26.3/Game command harness. Run `--reference-only --work-dir /fresh/java-path`, then the same script with `--binary /absolute/frozen-game --reference /fresh/java-path/vanilla.jsonl --work-dir /fresh/pair-path`. A supplied binary prevents a game build. Every work directory must be new.

The final predeclared contract has **394 strict records** over 11 cases: ordinary default, strict default, explicit banjo/note 7, strict explicit, air support, keep, filtered replace, setblock default/explicit, explicitly supplied default harp, and normalization of a pre-existing strict harp state. It admits the actual Player UUID/body/mode, loaded chunks, stone floor/support or air counterpart, source initial states, upper air, unaffected neighbor and zero relative clock. Harp/basedrum/banjo complements, note/powered properties, command counts, full feedback/styles/callbacks and fresh result/success stores remain strict. Skipped keep/filter commands preserve the actual state. Absolute-clock trace replies are retained separately without clock normalization.

Frozen main base: `b893b6ae31a1a589eadf1416e7f6efc6cf2f3196`. Fresh Java admits the final contract. Pristine native matches **361/394** and fixed native matches **394/394**, all independent admissions pass; the 33 complete old differences are in `note-block-fill-old-differences.jsonl`. Target placement outcomes remain in the strict gate, including the existing-harp count; they are not treated as independent setup guards.

Three registered Game command units use actual prepared block data and resident Game chunks. Old has two meaningful failures (actual harp instead of basedrum, and rejection of an update to an existing default state) plus one passing strict/explicit/filtered-out control. The fixed units pass all three. They inspect actual world states and unchanged Game time. The unit fixture's initial `execute if loaded` check overspecified readiness: that predicate requires lighting/entity admission in addition to fill's actual resident-chunk requirement. The rejected attempt is preserved; the corrected unit checks the actual resident chunk and real floor/air states. The live pair retains its full `execute if loaded` guards unchanged.

The original v1 Java program/reference has 329 records. V2 adds explicit-default restoration and existing-state normalization before any native capture. V3 changes only target-outcome admission metadata for all placements; it removes no record or expected output, and fresh Java outputs remain byte-identical to V2. All programs/references/raw replies are preserved.

## Build prerequisite

The initial untouched `0628aa587` build failed in native API generation because current-level Host and LevelRef methods shared private Rust names. This is rejected build evidence, not H164 runtime proof. The exact independent repair landed as [PR11767](https://github.com/horizon-rust/horizon/pull/11767). No duplicate local compatibility patch is present: the clean task branch advanced to that main commit before the native pair.

All 15 existing generator tests pass. Source comparison verifies that the Host methods changed only private identifiers and the LevelRef file is byte-identical; actual generated mapping/signature checks retain both public forms and their original bodies. The initial build log, landed diff/hash, source checks, generated bindings, frozen Game/unit binaries, program/reference hashes and complete raw outputs remain in protected `.local/h164`. A failed implementation compile using the private Class path is retained separately; it is not runtime evidence.

This is the measured note-block slice of H164. Historical fence/wire/kelp and other-property diagnostics are separate, with no broader repair or closure claim.

The two raw absolute-clock trace replies differ in both old/fixed pairs (**0/2** match); original values/styles/callbacks are retained in `raw-clock-diagnostics.json` and the logs. The strict relative-zero clock records pass. No absolute-clock normalization or clock parity claim is made.

Required final check ran once: **1,156 Rust tests passed**, workspace/server Clippy, formatting, public tree and docs passed. Overall exit **1** is explicit: three known optional-release Python/Bash empty-array failures (`musl[@]`/`arm64[@]`), 720 run with two skips. The 423 listed parity harnesses were not run by `--quick`; the actual H164 pair and three prepared-resource Game units were run separately.
