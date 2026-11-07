# Folder required data-pack metadata (H146)

The repair admits a folder only after the existing metadata parser succeeds, its root and `pack` section are objects, and `description` is present and non-null. This is **required-section/description-presence admission**, not full component/schema/format validation. The official `Pack.readPackMetadata` fallback still requires the description; description-only packs are a positive control.

## Frozen runtime proof

Base: `41944ed91474c6af28572d2e2273b6b2bd0bcf1e`. Eight independent folders contain valid ordinary functions with distinct score effects and no function tags. They are measured at initial load and after actual `/reload`.

| Metadata | Fresh Java | Untouched Game |
| --- | --- | --- |
| broken JSON `{` | unavailable, score -99 | exposed, score 11 |
| empty root `{}` | unavailable, score -99 | exposed, score 12 |
| array root `[]` | unavailable, score -99 | exposed, score 13 |
| empty `pack` object | unavailable, score -99 | exposed, score 14 |
| string `pack` | unavailable, score -99 | exposed, score 15 |
| missing metadata file | unavailable, score -99 | unavailable, score -99 |
| full valid metadata | admitted, score 17 | admitted, score 17 |
| description-only metadata | admitted, score 18 | admitted, score 18 |

The predeclared **241-record** program keeps all complete command feedback/styles, callbacks, fresh result/success stores, per-pack availability and function calls, score sentinels, actual owner identity/body, floor/load and relative-zero-clock controls strict. Complete pack lists and absolute startup clocks are separate raw diagnostics declared before capture. Their differences do not change the canonical denominator. Each valid file independently proves pack/content admission.

Untouched Game matches **191/241**. All 50 residuals concern the five malformed folders, at both phases. The missing-file negative and both passing metadata controls match. The fixed Game matches **241/241**, using the same original script and byte-identical fresh Java reference.

- Program SHA-256: `fac040797aea7b8a7190919581677fa0566b0c1105c7c31c07b0ee27f3206656`.
- Emitted script SHA-256: `3e72d31d7b03e10c1051cad53f15a76bc252d94ae39412d74174ce5308720c89`.
- Fresh Java reference SHA-256: `59972462b2579fd5a4f035ad0147d0505c9ebb823f77db463da9bb62e7911b98`.
- Untouched binary SHA-256: `a378b4f7dff3d35c3dbe56cf5106d326c0a9497f19c912182d464512d62a11a4`.

```sh
PATH=/opt/homebrew/opt/openjdk/bin:$PATH PYTHONPATH=tools \
  python3 tools/test_folder_datapack_metadata_parity.py \
  --work-dir .local/folder-metadata-java --reference-only
PATH=/opt/homebrew/opt/openjdk/bin:$PATH PYTHONPATH=tools \
  python3 tools/test_folder_datapack_metadata_parity.py \
  --work-dir .local/folder-metadata-game --binary /path/to/horizon-client \
  --reference .local/folder-metadata-java/vanilla.jsonl
```

Each runtime capture uses the same program and checks fresh Java canonical bytes against the retained reference. The production gate never executes commands to validate metadata; the fixture invokes functions only after availability checks to measure actual side effects. There is no H144 syntax or H145 tag prerequisite in these files.

## Source reach and regression controls

The condition is local to the directory branch of `datapack::discover_folder`. Its world-discovery list feeds `pack_stack`/enabled selection and the content consumers, so invalid folders are excluded before files are exposed. Runtime coverage here is functions; the independently implemented Create World pack screen still uses `packs::folder` and is not changed or claimed. `packs::parse_meta` gains only `pub(crate)` visibility; its first-JSON/BOM behavior is unchanged. Existing resource-pack folder/ZIP callers keep the same parser and behavior. Data-pack ZIP discovery, builtin/feature constructors, metadata interpretation, format policy, overlays and priority are unchanged.

The real discovery/DataStore unit and resource-backed Game function-command unit fail old and pass fixed. The Game control exercises both initial load and reload and proves invalid folders cannot expose or run their valid functions. Numeric description, null rejection, BOM, trailing comment, builtin and feature-construction controls are supplemental units; the runtime matrix remains the original eight forms. No JSON value is routed through the NbtOps component decoder. Numeric description bytes remain numeric; existing description interpretation is preserved.

Older positive-only synthetic pack fixtures now provide a valid description instead of empty/missing required metadata. Their assertions, content and model state are unchanged. The same test-only corrections are used old/fixed; the common test patch and raw intended failures are retained. The Game unit is placed away from H144's concrete insertion point. No pending H144/H145 production is imported, and the actual function lifecycle/tag code is untouched.

## Limits and evidence

Unmeasured malformed rich descriptions may still be admitted. The next action for full description-codec validity is a separately scoped JSON component admission review; this patch is not full H146 schema closure. ZIP/resource/feature validation and format compatibility are not claimed. H144 ordinary syntax and H145 required tags remain independent.

Complete loader logs, original declarations/program/reference, runtime records, test failures, raw diagnostics, binaries and manifests are retained in `.local/h146/`. [The residual report](folder-datapack-metadata-residuals.md) publishes all old strict differences and the predeclared raw trace records. No all-pack-list or raw absolute-clock equality is claimed.

Fixed binary SHA-256: `a2e9779136effbbbf31852b588cb5907f1bfd73bd225994e300d580156480c89`. Required local checks are reported in the PR; the 241-record runtime pair is separate from the wider harness list.
