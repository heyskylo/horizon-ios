# Saved primed TNT power

Pinned Java 26.3 `PrimedTnt.addAdditionalSaveData` writes `explosion_power` as a
Float only when the actual power differs from 4. Horizon starts from raw input,
so skipping the write alone retained an obsolete loaded key. The writer now
removes that one key when the actual modeled power is 4. It leaves nondefault
output, input conversion/clamps, fuse, block state, owner and other fields alone.

## Actual Game proof

Frozen base: `a1ff75a4a525b6a9e1ed68f74cf9d54161596370`. The untouched executable
is our preserved own build of that identical base, verified against its original
manifest/hash; no pending H109 or H090 changes are imported.

```sh
python3 tools/test_tnt_saved_power_parity.py \
  --binary /absolute/path/to/horizon-client --work-dir /fresh/output
```

The program predeclares 97 complete records: independent actual TNT summons with
power absent, Float 4, Float 0, Float 2, Float 5 and Float 128. Each has a stable UUID,
actual type/position/motion/NoGravity and tag/flag guards, loaded floor, Short 80
fuse and valid stone block-state string. The last case preserves a loaded owner
UUID payload; this is not a claim about live owner attribution. Fresh result and
success sentinels, exact Float types/presence and relative world time 0 are strict.
The Java reference uses the owner's prepared pinned jars and `_JAVA_OPTIONS=-Xmx1G`.

Untouched Game: **93/97**, all admissions passing. The four old differences are
`default-power-read/result/success/absent`; they describe one retained default
key. Fixed result: **97/97**. The same script and exact fresh Java reference bytes
are required for the pair.

- Program SHA256: `90aadc4ce2972c48e850224495fefbc0e7a767e2e687f04116c2a3298849946e`
- Reference SHA256: `b932bf7198c183fd3b447d689c9ff4bb310cc6720936bd5845967081e7302a85`

The resource-free real-writer regression
`entity_nbt::tnt_power_tests::saved_power_uses_the_model_and_omits_stale_defaults`
checks no raw input, raw without power, raw Float 4, and stale raw Float 2 while the
model is 4. Nondefault 0/2/5/128 overwrite stale input; exact fuse, block state,
owner, UUID and an unrelated flag remain. The identical old assertion fails on
raw Float 4. The identical fixed regression passes in the required Rust suite.

## Diagnostics and limits

Six whole bodies and two absolute clock reads are predeclared raw traces, kept
in full in `java.log`/`rust.log` separately from the 97 strict records. Old whole
bodies match 5/6; only the explicit-default body retains `explosion_power:4.0f`.
Old absolute start/end are Java 285 versus Game 0, while relative deltas are both 0.
Fixed bodies match 6/6 exactly; fixed absolute start/end are Java 301 versus Game 0. No absolute-clock or aggregate raw-equality claim.

Rejected attempts remain executable/raw: a compound block-state input defaulted
in Java (corrected to the valid string form); a named-TNT summon reported the
custom name in Java and the generic type name in Game (the next fixture omits
that optional name); an initial unit draft used a read helper instead of a Float
Tag constructor and failed to compile. None is counted as an H091 baseline.
No setter, fuse conversion, blast radius/timing/physics, numeric admission or
other optional-key cleanup is changed or claimed.
