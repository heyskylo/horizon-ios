# Spellcaster SpellTicks load/save binding

This change binds the Evoker and Illusioner `SpellTicks` NBT field to the existing
`extra.counter` used by casting goals and server-AI countdown. It changes neither
countdown placement nor casting policy. In particular, NoAI still suppresses the
server-AI countdown; no additional Illusioner branch is added.

## Authority and frozen source

Pinned Java 26.3 `SpellcasterIllager.readAdditionalSaveData` calls
`getIntOr("SpellTicks", 0)` and assigns `spellCastingTickCount`.
`addAdditionalSaveData` always writes that counter using `putInt`.
`customServerAiStep` decrements only a positive counter after the parent call.
The server-side `isCastingSpell` and casting goals use that same counter.
The ledger and pinned bytecode were inspected independently.

The actual Game pair is frozen at
`a589a3dd9be04fa7f17667c0175711aaa92e1147`. Both binaries use the same declared
programs and own fresh Java reference bytes. Merged PR9306 already supplies the
Illusioner countdown; this patch only connects the model's NBT boundary.

## Actual runtime fixture

`tools/test_spell_ticks_parity.py` uses the existing command-execution harness,
real Game, and fresh pinned Java server. Twelve UUID-bound persistent actors cover
both species × NoAI 0/1 × initial Int 30/0/-2. Each has Health20, NoGravity and an
actual zero movement-speed attribute. A loaded stone platform, creative owner,
no other actors and disabled mob spawning/griefing isolate the counter without
substituting a model or a synthetic tick. Readings are at cumulative 0/1/3/10/31
actual world ticks, with identity, AI, Health, position, load and callback guards.

Expected values independently validated in fresh Java:

| AI | Input | Ticks0/1/3/10/31 |
|---|---|---|
| enabled | 30 | 30/29/27/20/0 |
| disabled | 30 | 30/30/30/30/30 |
| either | 0 | 0/0/0/0/0 |
| either | -2 | -2/-2/-2/-2/-2 |

The 598-record canonical program compares complete raw commands, feedback styles,
callback records, result/success stores, typed Int timer predicates, actor positions
and the exact 31-tick relative clock. All 60 complete actor compounds and two raw
absolute startup clock replies remain trace diagnostics, and are executable in the
660-record `--full-diagnostic` program. These categories were declared before the
fresh captures. No numeric tolerance, record mutation or observation mask is used.

Program SHA256:

- canonical: `b2f62b40dc8f0e591759bd1fc09a73227054db1f1e0210bc989d03fb4938190a`
- full: `92cbc4e4e42ccc9f26326940ef6abf8c5d5279d4e310691612bb62e47583cc53`

All generated scripts, exact references, complete raw outputs, saved worlds,
bytecode, binary hashes, manifests and rejected attempts are retained locally in
`.local/h180`. No Mojang resources or coordinator payloads are committed.

## Resource-free regression

`cargo test -p horizon-client-lib --lib spell_ticks_nbt_tests` exercises the real
Mob NBT decode/save functions with fresh modeled mobs, absent/positive/zero/negative
inputs, no raw output source, mutated counters, stale raw counters and an unrelated
raw field. The same tests fail on unchanged production because the writer omits
the modeled Int even at default 0 and29. The load test also independently asserts
the counter after each decode. The initial gameplay-library test target ran zero
tests; its log is retained as a rejected target choice, not a passing result.

This is partial coverage of absent SpellTicks default output, not whole H110
closure. Fractional/overflow conversions, other species, spell selection, particle
or damage parity, network metadata and save/reopen lifecycle are outside this proof.

## Paired results and retained residuals

- Canonical: unchanged 574/598 → fixed 598/598. All 24 original differences are
  eight normal-AI positive-counter samples, each witnessed by raw Int reply,
  result store and typed NBT predicate. All admissions pass in both binaries.
- Full diagnostic: unchanged 626/660 → fixed 657/660. No 660/660 claim.
- Resource-free regression: unchanged 0/2 → fixed 2/2.

Every remaining fixed full-program difference is listed here:

| Label | Fresh Java reference | Actual Game | Scope |
|---|---|---|---|
| clock-start | callback `[true,281]`, feedback281 | `[true,0]`, feedback0 | absolute startup times retained, not compared as relative time |
| 1-31-body | `Rotation:[0.0f,0.0f]` | `Rotation:[0.0f,0.75957566f]` | normal-AI Evoker pitch after its loaded cast expires; no pose/goal repair |
| clock-end | callback `[true,312]`, feedback312 | `[true,31]`, feedback31 | both relative deltas exactly31 |

The body callback is `[true,1]` in both cases; full text/style records and raw
positions are retained without modification. Separate fresh Java runs retain their
own absolute startup values; the full old/fixed report uses one immutable reference.
No generic rotation, timer, clock or formatting change is included.

The fixed-full run's independently fresh Java body also reports
`Rotation:[0.0f,0.75957566f]`, matching that native capture. The immutable common
full reference and old-full Java run report `[0.0f,0.0f]`. This establishes a
noncanonical look/pitch observation across fresh Java runs, not a new confirmed
native pose defect. All three raw Java bodies remain intact; the full diagnostic
comparison is still657/660 against the original common reference.

Canonical fresh Java reference SHA256: `0dddee526379b2e5cddf155788265013a76f468490b667298b7ac356d4f282b4`.

Run with prepared local26.3 resources and a built Game:

```sh
python3 tools/test_spell_ticks_parity.py --binary target/fast/horizon-client --work-dir .local/spell-ticks
# Keep the complete compounds and absolute-clock diagnostics:
python3 tools/test_spell_ticks_parity.py --binary target/fast/horizon-client --work-dir .local/spell-ticks-full --full-diagnostic
```

## Required local checks

`tools/pr_ready.py --quick --base a589a3dd9be04fa7f17667c0175711aaa92e1147`
ran once on the final Rust/driver patch: rustfmt, Clippy with warnings denied,
public tree and doctests pass; Cargo.lock stays unchanged. The affected reusable
client suite has1962 passes,63 ignored and one pre-existing failure:
`entities::drop_motion_tests::drops_match_vanillas_motion_bit_for_bit` at yaw37.5,
pitch-21.25, Z bits actual4597448471913462162 versus expected4597448471913462163.
Our own untouched-production a171 baseline already reproduces that exact result;
entities.rs and jmath have no source delta from that baseline here. Its original
log and provenance are retained, without claiming a new H180 baseline execution.

Python reports696 tests, four failures and one skip. The unchanged installer
signature fixture lacks `horizon-macos-arm64.zip`; three optional archive cases
fail under macOS Bash3.2 with `musl[@]`/`arm64[@]: unbound variable`. The narrow
installer diagnostic confirms the fixture failure. These checks are not green;
no installer, drop-motion or other unrelated repair is included.

The assigned actual Game/Java parity runs above were completed separately; quick
mode does not run the broad harness matrix. The ledger check finds both codec
methods newly mapped and no new gaps. Documentation result notes were added after
the check; the Rust and harness files are unchanged. Disk stayed above the5GiB
hard guard, with one heavy build at a time and jobs2/incremental0. Only verified
inactive artifacts owned by this worker were archived; exact unknown stage times
remain null in the local manifest.

## Conflict maintenance source refresh

The branch is refreshed onto `98af9399cd1c8cd39e153be686808d0cbc71353b`.
Main's live pickup-state writer, conversion-tag helper and armor-stand fall codec
are preserved. H180's complete spell tests now sit before the general test
module, away from the shared EOF append location; the test bodies, production
counter binding, fixture programs and references are unchanged.

Source review finds no change to the spell counter or its countdown. Formatting,
the two spell codec tests, pickup-save and stand-fall regressions, and scoped
Clippy are checked on the refreshed base. The actual Game pair and original full
required-check report remain qualified to frozen a589, with their original
residuals. No latest-base Game pair or whole-suite rerun is claimed. The earlier
action-required notice was transient: old head fcf1f6f's verify/launcher checks
succeeded; broader parity was skipped. New-head checks remain separate.
