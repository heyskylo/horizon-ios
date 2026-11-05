# Loaded claimed-bed raid village loss

The focused fixture is `python3 tools/test_raid_village_parity.py`. It uses two
villages 1024 blocks apart, genuine AI-acquired bed claims, real `/raid start 1`
dispatch, and actual first-wave members before removing both parts of one bed.
A zero-world-tick NoAI write isolates the villagers after admission; the audit
checks unchanged identities, health, positions, home memories, POI inputs and
member bindings across that write. Raiders remain AI-enabled.

The Java oracle process alone uses `-DMC_DEBUG_ENABLED=true` and
`-DMC_DEBUG_DEV_COMMANDS=true`; both isolated native baseline/fix processes use
`HORIZON_DEV_COMMANDS=1`. These select the existing development command trees.
No production default or dispatcher adapter changes. Standard harness isolation uses
`HORIZON_NO_AUDIO=1`, `HORIZON_NO_DISCORD=1`,
`HORIZON_LAUNCHER_NO_SELF_UPDATE=1`, `HORIZON_TEST_LOOPBACK=1`, fresh
Horizon/launcher/XDG homes, a refused local API-discovery address, and per-run
command-script/output paths; the owner is `Player`, scratch world `harness`,
seed 1, radius 4. Inspected pinned raid,
raid-manager, POI-manager and server-level lifecycle methods do not branch on
these registration flags. Genuine admission is checked independently of command
success. This is a loss-phase test, not a natural bad-omen admission claim.

All 373 canonical complete JSON records are strict, without exclusions or
comparison overrides. Full raid/entity NBT, random member identities/positions,
current claimed positions and live POI inputs remain in raw logs. The native
observer calls the existing production village answer and exposes its inputs;
it does not reconstruct corrected occupancy. Java independently observes actual
occupied POI counts. Random wave composition and trajectories are diagnostics,
not cross-engine exact claims.

The same-base c04c5ccf9 baseline matches 370/373. The only differences are the
removed-bed observations at elapsed 1, 20 and 600 ticks:

| Observation | Java | Baseline Game |
| --- | --- | --- |
| after-1-2 | loss, village false | ongoing, village true |
| after-20-2 | loss, village false | ongoing, village true |
| after-600-2 | loss, village false | ongoing, village true |

The same-script fixed replay matches **373/373 complete raw records**. Its fresh
Java reference is byte-identical to the baseline reference. Script SHA-256:
`a71464a6ee350e03e0ed590db64f36546d8699a8f667a6bdae65e89e7f8c4dcd`;
reference SHA-256:
`a0e157abb455dfa5e116404d746ab9647ae948890f56c502af46093269413ca9`.

After merging main `624c4f6265c02296fbe8a80f029dd0d987fb94c0`, the same
untouched-production/fixed pair again matches **370/373 → 373/373**, with the
same byte-identical script and fresh reference. The conflict resolution retains
map/frame and raid observation kinds; the existing frame-map fixture also
passes **192/192 complete raw records**. Original c04-based evidence is retained.

The retained-bed raid remains ongoing. Both villagers retain their stale home
memory after removal. The bounded fix validates only known loaded bed claims
through the existing HOME POI lookup; unavailable claims and the existing job and
meeting paths remain unchanged. Those paths and all-unloaded lifecycle behavior
are explicit untested limits, not all-POI parity claims.

The initial full373 capture with Java development commands but native release
commands is retained as a rejected, unmatched test configuration. It has 14 raw
differences (two rejected raid commands and twelve absent raid observations),
not a parser defect or a qualified loss-phase baseline. Reproduce that diagnostic
with `--raid-release-command-diagnostic --allow-gaps`. The strict fixture and its
baseline/fix observations use the matched flags above.

## CI maintenance refresh

Verification on `5cede4ff4` was cancelled while Rust tests were running; its log
contains no failed test before cancellation. Main `ff4b4b17b` was merged without
rewriting history. The sole manual conflict was spacing in the Java command-trace
line. All main Java case registrations and Rust operation arms are retained,
plus `raid_state`; the raid production fix, unit and corpus are unchanged.

The merged Java oracle/driver matches **373/373** against the archived validated
`624c4f626` fixed native binary; script and fresh Java reference hashes remain
byte-identical to those above. The shared frame-map fixture matches **192/192**.
These are harness compatibility checks, not a runtime replay of the newer merged
Rust tree. Formatting, whitespace and public-tree checks pass. New-head CI must
provide verification of that merged tree.
