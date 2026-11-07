# Warden emerging and dig-cooldown save bridge

Issue [#11685](https://github.com/horizon-rust/horizon/issues/11685). Frozen base `004a39be07ad06e33b93e6ebaaf156a2053b02f0`, own fresh pinned Java26.3. Two existing runtime Unit slots were not loaded or projected: raw saved Brain could hide the absent model, and stale values survived ticks/mutation. The repair carries only is_emerging and dig_cooldown through their existing Slot APIs. No model, brain engine, ticker or TTL-policy change.

## Independent saved-NBT command proof

Five UUID-bound subjects cover normal-AI finite, NoAI finite with an unrelated saved sniff cooldown, absent Brain, empty Brain, and permanent Unit memories. Real typed identity/Health500/NoGravity/NoAI/Pos, loaded floor/cells and player controls remain strict at actual relative world clocks0/1/2/3/5. Cells keep positions stable; wardens are neither named nor persistent, so existing persistence behavior does not refresh the dig cooldown. No damage, setter resets or artificial counter/time assignments are used.

Fresh Java independently saves emerging2/1/0/absent/absent and dig1200/1199/1198/1197/1195. NoAI retains2/1200. Absent and empty inputs keep both owned keys absent. Permanent Unit entries remain present without TTL fields. Saved Unit values, exact Long TTL records, key presence, all messages/styles/callbacks and fresh result/success stores are strict. Full entries/Brain/body/pose/motion and exact absolute clocks are separate predeclared raw diagnostics, not normalized records.

Canonical v2 has839 records: pristine actual Game815/839, with all24 differences confined to the normal-AI finite subject's saved expiry/TTL values and all admissions/controls passing. Fixed Game839/839 with the exact unchanged v2 script and fresh Java reference bytes. This saved-NBT proof is independent of the runtime-slot assertions; oracle description text is not substituted for saved state.

Original v1 full839 is immutable and executable. Its pristine790/839 capture is an unqualified prerequisite diagnostic because an unchanged unrelated memory's compound field-order guard fails. The fixed full program is816/839, with23 whole-entry field-order residuals. It is not green and is not a second qualified denominator. V2 uniformly reads each Unit value plus existing TTL/presence fields, with new labels and all full entries retained raw; no failure-based row selection, normalization or formatter repair. [Every original/canonical old and full fixed expected/actual residual](warden-owned-memory-residuals.jsonl) is public. Full body/Brain/pose/height/sound/other-memory differences remain unclaimed diagnostics, retained in complete Java/native logs and extracted trace JSONL under `.local/h029/`.

```sh
python3 tools/test_warden_owned_memories_parity.py --reference-only --work-dir .local/warden-memory-java
python3 tools/test_warden_owned_memories_parity.py --binary /path/to/game \
  --reference .local/warden-memory-java/vanilla.jsonl --work-dir .local/warden-memory-game
python3 tools/test_warden_memory_full_diagnostic.py --reference-only --work-dir .local/warden-memory-full-java
python3 tools/test_warden_memory_full_diagnostic.py --binary /path/to/game \
  --reference .local/warden-memory-full-java/vanilla.jsonl --work-dir .local/warden-memory-full-game
```

## Actual model and tick proof

Three identical regressions fail old and pass fixed. Resource-free real mob decode tests inspect both actual slots, then save with no raw input, original raw and contradictory raw. Absent/default, finite and permanent state is authoritative. Mutation clears one owned slot and changes the other against stale raw, preserving unrelated Brain fields/memories/raw data and listener state. A prepared-resource test decodes the real Warden type with emerging2 and nondefault dig17, snapshots the initial slot state, and runs actual Mob.tick calls, checks actual age increments/live slots and no-raw saved NBT on every tick through expiry; NoAI retains the slots. It reuses the existing MobWorld fixture, not a copied timer. Actual Game clocks are independently established by the command pair.

The first test-only placement could not access the private existing world fixture helper. That compile log is retained as a rejected prerequisite; tests moved into the Mob module, with no visibility/API expansion. The initial tick draft stopped at a load assertion; it is retained separately. The final regression checks the initial snapshot after its tick loop, so the old failure is observed after actual tick1 (live slots absent/1199 versus expected1/16). The resulting old failures are actual missing-slot/stale-save assertions; there was no separate expiry-policy failure to absorb.

## Narrow source reach and limits

The loader preserves finalize timing and the existing listener path, then restores/clears only these slots. Fresh Java's absent behavior justifies clearing their loaded defaults. Proper empty Unit values with absent TTL use the existing permanent setter; typed Long TTL uses set_ttl. No malformed/numeric fallback is inferred. No support or Java equivalence is claimed for scalar/fractional/malformed TTL/value/container shapes or extreme/sentinel numeric edges. Non-Long TTL entries are left at existing initialization rather than inventing a permanent fallback.

The Warden writer projects actual slot presence, Unit value and finite TTL, removing stale owned keys when absent. It mutates only the two keys inside the existing Brain.memories compound and preserves other valid Brain fields/memories/raw data; no complete Brain replacement. Noncompound raw containers remain unsupported. This writer naturally reaches all Warden saves, while runtime proof is the measured command-loaded subjects; no all-Warden/Brain, natural-spawn, restart, packet or selector-height closure.

Production changes are entity_nbt only. Warden brain source additions are tests. Existing Slot expiry, normal/NoAI update policy, anger/listener, pose/animation/readiness, sounds/combat/RNG and all other memory producers/tickers are untouched. [PR10365](https://github.com/horizon-rust/horizon/pull/10365) owns anger/suspect UUID serialization. Keep its independent anger load/save and this owned-memory carry if adjacent hunks meet; no pending implementation or UUID policy was imported.

Known capture/check observations and hashes are in the local manifest. Exact setup/proof/implementation start times not recorded remain null. Required production checks completed once: 2,162 Rust tests, rustfmt, Clippy, public-tree and doc-test checks pass; content root is untouched. Python ran718 with three failures in optional release archive combinations (`musl[@]` / `arm64[@]` unbound under the local Bash), one skip. Overall pr_ready exits1; this is not an all-checks-green claim. It listed412 affected parity harnesses for CI rather than running them all. The final cfg(test)-only tick strengthening and final documentation receive affected follow-ups, without repeating the production suite.

## Frozen provenance

- Canonical script SHA-256: `35a0b6c43ac9e6e69661215cb02d0856d808d48da7ad7b11e5bcfb2bf7d3d761`.
- Canonical fresh Java reference SHA-256: `3f6d373b4651a14d09d84c923ec27dbba7e4239c1a8b94e24a512838604b036e`.
- Full diagnostic script SHA-256: `92ba304ea09a9f21e401057621354e30832b49274fff7d4834d8ec8820805be0`.
- Full fresh Java reference SHA-256: `ca2d346fd7a1fee69a94edd8e8cdab175ff0b044766969dd62be0eb1305e9bfd`.
- Pristine Game binary SHA-256: `a5780e36bcf081f155b924f932800f58c8440f48aa61fbcf66ed894251df9873`.
- Fixed Game binary SHA-256: `b9d611299bdf4a78f1782890ce86965d7c9db975c5bc3caf87c6ea96e6c3d761`.

Later main `6f01443e55f1dc3e472b839ad8d3a8dda094cbeb` was reviewed read-only: command-harness/driver additions concern boat/map operations and unrelated Boat leash getter state; no Warden load/save/Slot/tick path changed. No imports or proof-base refresh. The final test-only follow-up changes neither production bytes nor the captured binary pair.

Final affected follow-ups pass: all three identical final regressions fail on old production and pass fixed, including actual tick1 old failure and fixed ticks1..5; all-target library Clippy, rustfmt, public-tree and diff checks pass. Production SHA-256 remains `345c9000668cbbb707aa1614318be2adf4e2f9e4ebf1dc3bcddab0bd59664cb8`. The final test file differs from the runtime capture source only under cfg(test).

Final fetch review to `7ab92f9eb7e94e15b4f4271440d2f8169a7afed8` adds only launcher work and an Oracle light-observation wait since the prior review. This program uses no light operation; no relevant production delta or proof-base update.
