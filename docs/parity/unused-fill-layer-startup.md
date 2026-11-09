# PARTIAL H259: unused fill-layer startup validation

This slice validates winning configured-feature entries of type `minecraft:fill_layer`
before world preparation creates streaming or reports readiness. It calls the existing
`FillLayerFeature::from_json` leaf codec with the actual block registry and the same
registry-derived integer-property classification used by generation. It does not run
feature-tree expansion, check terrain, or implement a general registry loader.

Issue [12264](https://github.com/horizon-rust/horizon/issues/12264) remains open for
unvalidated registry breadth. Other feature types, unknown types, holder dependency
validation, cycles, deep graphs, reload and other registries remain outside this repair.
H006 dimensions and H171 command parsing are separate. No pending patch is imported.

## Frozen reproduction

Own pristine base: `4dbb00502ff162b4ac3472a3e7c85c5d8d88883e`, official Java 26.3.
One selected pack contains only the unused `h259:unused_probe` feature. The two inputs
are identical except height `0` versus `-1`:

```json
{"type":"minecraft:fill_layer","height":-1,"state":"minecraft:stone"}
```

Actual saved settings independently admit a flat overworld with features, lakes and
structures disabled. Nothing references or places the custom feature. Common console
commands query enabled packs, create a fresh objective, set/read score259, query time,
and emit a marker only if that actual score matches. Complete logs and process facts
are retained; timestamps and saved metadata are not normalized.

Java admits the valid entry and serves commands. For the invalid entry it reports the
exact registry key, pack and `Value -1 outside of range [0:4064]`, then exits **0** before
readiness. Pristine native reaches readiness and serves commands for **both** entries.
Every process is reaped; readiness, service, deadline, probe stop and return code are
recorded separately. One startup defect is claimed, not a defect per log/exit fact.

The actual `WorldRequest::prepare` regression first admits valid preparation, discovery,
the exact raw entry and flat preset, then fails at invalid-entry acceptance. Its original
module and immutable old ELF are frozen. Separately labeled native preservation controls
cover a valid fill-layer under another ID, an unrelated `no_op`, and winning-entry
replacement. They add no Java runtime or all-registry claim.

## Source reach

| Path | Reach and limit |
|---|---|
| `WorldRequest::prepare_world` | Selected winning entries checked after real level/pack selection, before `make_streaming`; integrated and dedicated callers share this boundary. |
| `WorldgenOverrides::validate_fill_layers` | Selects the real leaf codec by normalized type only; no fixture ID, height check or copied schema. Other types retain their previous path. |
| `block_feature::integer_properties` | Mechanical extraction of the existing classification; generation and startup use the same expressions and block registry. |
| `Game::load_world` / dedicated startup | Existing preparation error propagates through `?` before `Done`; flat-generation warning/fallback cannot swallow this check. |

## Retained diagnostics and invalid premises

- Historical27 registry cases remain untested.
- Native's runtime enabled-pack command includes `file/h259`, but saved `level.dat`'s
  enabled list omits it. Existing discovery auto-enables new file packs. Complete saved
  metadata is retained separately; pack persistence is not repaired.
- The first Java draft used legacy block-state `{Name:...}` and was rejected. No native
  capture or build followed that draft. The admitted input uses the modern string form.
- A verifier initially expected the literal word `height` in Java's range error. Its failed
  report remains; the actual entry/range error was already captured and is unchanged.
- The first native driver expected the direct game executable to own RCON; the supervisor
  owns that listener. Those captures remain separate. Qualified V3 uses real console
  service on both servers; the earlier positive Java RCON evidence is also retained.
- Cargo reset the original test's cwd to its package directory despite the outer scratch
  cwd. Its own worlds are preserved under evidence. The unchanged archived unit was
  replayed in scratch before fixed proof (intended failure,32.93s); no player data was used. A first replay did not execute because the archive lacked its execute bit; that log is retained.

## Running

```sh
python3 tools/test_unused_feature_startup_parity.py --kind java --output /tmp/h259-java
python3 tools/test_unused_feature_startup_parity.py --kind native --binary /path/to/horizon-client --output /tmp/h259-native
python3 tools/verify_unused_feature_startup.py --reference /tmp/h259-java --native /tmp/h259-native --report /tmp/h259-report.json
```

Use fresh output directories and the pinned resources. The capture stores observations;
the strict verifier distinguishes the invalid-entry contract from the valid control.
Fixed proof preserves the exact V3 inputs/reference: valid command service passes; invalid native
stops naturally with its real leaf-codec error before readiness (exit1, no timeout or probe stop).
The unchanged original unit passes (23.72s), and the labeled preservation unit passes (24.03s).
Final required checks are recorded in the PR. Unknown phase times remain null.
