# Numeric damage component decoding

H087 covers only the `minecraft:damage` component's numeric conversion at the
item model boundary. Command validity, other components, generic NBT helpers,
wear, default elision and simulation are unchanged.

## Authority and scope

Pinned Java 26.3 `DataComponents.DAMAGE` uses `ExtraCodecs.NON_NEGATIVE_INT`.
`Codec.INT.read` calls `DynamicOps.getNumberValue` and `Number.intValue`;
`NbtOps.getNumberValue` exposes the boxed number through `Tag.asNumber`.
This truncates Float/Double toward zero and narrows Long to its low 32 bits.
It is distinct from NumericTag's floor conversions. Encoding uses `createInt`.

Horizon command validation already performs that conversion. The item model
instead used an integral-only helper for damage. The existing
`horizon_world::datafix::dynamic::as_int` supplies the boxed-number semantics;
only the damage decoder should use it here. The integral helper and every other
component decoder remain unchanged.

## Existing actual Game route

`tools/test_numeric_damage_component_parity.py` reuses the existing
`test_command_exec_parity.py` integrated Game and official Java oracle. It uses
one loaded, supported chest and the actual connected owner. There is no new
observer or datapack. Every case writes through the actual item command, then
reads the saved chest stack, typed component, item/count/slot and unrelated
custom data. Fresh result/success sentinels preserve actual callbacks, including
rejected input and missing-field behavior.

Twelve independent slots cover plain/default, integer1, Float1.5, Double2.75,
Float-0.5, Long4294967297, positive Float/Double overflow, and four rejected
negative/narrowed-negative inputs. Rejected writes start with an independently
admitted stone2/custom-data stack and must retain it. Java independently measures
the parse failure's absent callback, rather than assuming a callback of zero.
The positive overflow cases save Int2147483647. The negative-half default saves
no damage patch. Full saved stacks retain exact tags and styles.

Before capture the 329-record canonical program was declared in
`.local/h087/declared-canonical.jsonl`, SHA256
`f15bf4ed8400a3b37b2b958369896ef14022dcdd4e6e2ee935eb0403517f2607`.
The complete332 program remains executable with `--full-diagnostic`, SHA256
`de28da09f9f02a36449ae440c849c956aaba606dea3f8aa7b7d3a245eb53e9e9`.
Exactly `clock-start`, `parent-block`, `clock-end` are predeclared raw diagnostics:
absolute startup clocks and complete parent-block presentation. Every saved
stack, damage value/type, component presence, callback/store, setup guard and
actual relative-clock delta remains strict. The canonical program retains those
three raw command replies in the execution logs. No record masking or numeric
tolerance is used.

The world is frozen and no observation tick is added. Actual start/end clocks
remain raw; the separately checked relative delta is zero. Positive admission
covers owner UUID/type/Health/mode/Pos, forced loaded chunk, stone support, exact
chest block state and coordinates, initially empty inventory and each slot's
initial absence. This does not establish persistence/reopen, packets or GUI.

The initial Java draft used an invalid typed-match path and assumed an absent
callback for a missing data field. Its complete script/output remain in
`.local/h087/java-draft` and `rejected-typed-path-driver.py` as a rejected test
prerequisite, not a native defect. The corrected Java-only validation is also
retained before the declared paired program.

## Reproduction

```sh
python3 tools/test_numeric_damage_component_parity.py --reference-only --work-dir /tmp/h087-java
python3 tools/test_numeric_damage_component_parity.py --binary /path/to/game --reference /tmp/h087-java/vanilla.jsonl --work-dir /tmp/h087-game
python3 tools/test_numeric_damage_component_parity.py --full-diagnostic --binary /path/to/game --work-dir /tmp/h087-full
```

Fresh scratch directories keep all command scripts, references, native raw
records, full logs, exact mismatch tables and world data. Official bytecode and
other nonredistributable artifacts stay local. Stage times are retained where
recorded; unobserved times remain null.

## Measured result

Both actual Game binaries use frozen main
`76b8b4eb8336e397555b326281a0bcc27b8e2ed1`; the fixed binary adds only the
local damage conversion. No pending prototype, selector or command fixes are
imported. The final fetch advanced to `a1710b696233b7cb3a1c1d3482aeeb8e270ad6e3`,
which only reuses the mob Seen snapshot buffer. The decoder, validator, item
command and boxed-number helper are unchanged; this zero-tick chest fixture
has no mob-snapshot input. This is source review, not a combined-latest runtime
claim.

| Program | Untouched | Fixed |
| --- | ---: | ---: |
| Strict329, identical program/reference bytes | 308/329 | 329/329 |
| Complete332 diagnostic, identical full comparison reference | 308/332 | 330/332 |

The strict reference SHA256 is
`d8ba27021581bcd7bd6dd4e6242e6fcdeb16d9dd2f277ca70bf824b63630cd90`.
Every raw record includes feedback text/styles and callbacks; no comparison
mask or tolerance is applied. All admission controls pass. There are zero actor
observation ticks and an exact zero relative world-clock delta.

| Accepted input | Java saved damage | Old Game | Fixed Game |
| --- | --- | --- | --- |
| Float1.5 | Int1 | absent | Int1 |
| Double2.75 | Int2 | absent | Int2 |
| Float2147483648 | Int2147483647 | absent | Int2147483647 |
| Double1e20 | Int2147483647 | absent | Int2147483647 |

Each of those four inputs accounts for five old differences: full stack, damage
read, stored result, stored success and typed-presence match. The final full
inventory accounts for the21st. Plain/integer1, negative-half default0, narrowed
Long1 and all four rejected-input/sentinel-stack controls are unchanged. The
full diagnostic adds the parent tag containing those state differences and two
raw startup-clock differences. The fixed parent tag now matches too.

Every remaining fixed full-diagnostic residual is listed here. The styles are
identical (unset colour, all flags false); only the literal number differs:

| Label | Java comparison-reference feedback; callback | Fixed feedback; callback |
| --- | --- | --- |
| clock-start | `The game time is 281 tick(s)`; `[[true,281]]` | `The game time is 0 tick(s)`; `[[true,0]]` |
| clock-end | `The game time is 281 tick(s)`; `[[true,281]]` | `The game time is 0 tick(s)`; `[[true,0]]` |

The independent fresh Java runs retain their actual absolute start/end values:
canonical reference313/313, old canonical321/321, fixed canonical281/281;
full reference281/281, old full rerun301/301, fixed full309/309. Native is0/0 in
those captures. These values are neither replaced nor subtracted in captured
records. Relative delta0 is a separate strict invariant, not raw clock equality.
There is no332/332 claim or clock repair.

The resource-free decoder/save unit fails unchanged code at Float1.5 (0 versus1)
and passes fixed code with ten input/default/boundary controls. It checks typed
saved Ints, unrelated custom data, absence of any retained damage/wire source,
and a separately constructed model with no raw input. No invalid-input acceptance
or save-default policy is changed.

Local evidence: `.local/h087/{java,old-final,fixed,old-full-final,fixed-full}`,
`unit-{old,fixed}.log`, immutable executables, bytecode, declaration files and
`manifest.json` hashes/timestamps. An earlier full-diagnostic checker incorrectly
treated its predeclared absolute clocks as admission after writing the complete
raw/report; that rejected wrapper/capture is retained. Correcting the diagnostic
assertion changed neither program nor comparisons, and the final old/fixed
captures use the same corrected checker. No failed setup is counted as a defect.

## Local checks

The final production patch passes rustfmt, Clippy with warnings denied,
70 horizon-items tests,26 content-root tests, doctests and the port-ledger check
(no new gaps). Python696 reports four existing macOS tooling failures and one
skip: the installer signature fixture lacks its ARM archive; three optional
release-archive cases hit Bash3.2 unset arrays. A narrow current installer run
confirms that independent prerequisite. No tooling repair is included.
Required checks ran once on the final Rust/harness; later changes only add these
evidence notes. Final public-tree/whitespace checks are recorded separately.
