# Item-display saved transformation

Item displays kept in the actual Other collection now own an optional transformation
with only four typed Float arrays: translation, left rotation, scale and right rotation.
Actual item-display loads alone initialize it. Missing input becomes the independently
Java-admitted identity; valid supported compound TRSR uses the existing pure decoder.
Matrix/invalid shapes and unsupported numerical forms stay unprojected on their prior
raw path. The existing parser body is unchanged; this does not establish full codec parity.

The saved writer projects only these four modeled arrays. It does not call the native
writer that also writes interpolation/teleport/start fields. The transformation-only Tag
construction is shared mechanically with that writer; its duration/start body is byte
identical. Successful existing native setter writes carry the item-display state afterward;
its failure guards, order and other-subtype behavior remain unchanged. That setter carry
is Game-unit/source coverage, not a live native-mod delivery claim.

No display data-adoption, rendering, interpolation, packet engine, item codec, other subtype
or general Other schema is repaired. All unrelated raw fields/identity remain on their
existing route. H129 remains independent: when combining the two branches, retain both
class-specific fields/initializers, with knot counter None on displays and transformation
None on knots/Cloud. Preserve H129's own attachment phase and common-default branch.
No pending H129 production is imported here.

## Authority and frozen proof

Pinned Java26.3 `Display.readAdditionalSaveData` reads Transformation.EXTENDED_CODEC with
IDENTITY fallback; `addAdditionalSaveData` always writes Transformation.CODEC. Current
PR8362 is a port-map change citing the mod writer, not this missing default repair; PR4631
is the existing matrix reader and PR5408 the native setter. Actual diffs were reviewed.

The frozen base is `98b882712308386bbb48acbc60968f7d7a991902`. Fresh Java admits three
independent actual item displays: omitted input, explicit identity, nondefault valid
compound. All hold diamond2 with custom_data7, have fixed UUID/type/Pos/Rotation/NoGravity,
loaded floor and real relative clock0. Pig and Marker are unrelated controls. The complete
transformation and all four typed Float lists are read with fresh result/success stores;
full bodies/styles/callbacks are retained.

Original V2: old203/220, including15 omitted-field failures and two independent whole-
compound key-layout differences. All typed explicit controls already match. V1/V2 Java
references and executable inputs are byte-identical; V2 strengthens positive callback guards.
Before production, V3 predeclares all three whole-transformation query text rows diagnostic,
not just the two failing explicit rows. Their stores and all four typed-list reads remain
strict: **203/217 old**. Full220 remains executable and its actual residuals are retained;
there is no output masking, tolerance, formatter repair or claim of full220 parity.

Four registered Game tests run through actual load/model/save: omitted identity,
nondefault/identity without raw source, actual native-setter mutation against contradictory
raw and no-raw saving, and preservation of unsupported forms/non-item classes. Old has
three meaningful failures and one passing preservation control. The fixed source passes
all four. The existing writer's pure start-flip/validation units remain affected controls.

## Reproduction

From the repository root, using prepared local resources and fresh output directories:

```sh
python3 tools/item_display_transformation_fixture.py --reference-only --work-dir /tmp/display-java
python3 tools/item_display_transformation_fixture.py --binary /absolute/path/to/horizon-client \
  --reference /tmp/display-java/vanilla.jsonl --work-dir /tmp/display-native
```

This manual driver reuses the existing Game command harness and reruns fresh Java to
verify the exact reference bytes. It writes a full220 report and separate strict217 report.
Run the unchanged script/reference on old and fixed binaries. Resources, original programs,
full bodies/diagnostics, invalid or unsupported records, source/binary hashes and phase
metadata stay in the protected local task archive. Unknown phase starts remain null.

Runtime scope is the three admitted compound/default item-display cases. Matrix/invalid
unit controls prove preservation of the prior native path, not Java input admission or
matrix equivalence. Negative teleport duration, subtype defaults and full-body formatting
remain separate diagnostics. No all-display, renderer, reopen or complete-codec claim.

The final frozen pair is **203/217 →217/217** on the declared typed contract. The actual
full220 result is **203/220 →217/220**, with all three whole-transformation text rows still
different in key layout. All stores and typed-list reads match. These residuals are public
in [item-display-transform-residuals.jsonl](item-display-transform-residuals.jsonl); the two
old captures have byte-identical full220 observations and share a variant-labelled payload.
No residual is suppressed or counted as a full220 match. Raw whole-body/absolute-clock
traces remain separate protected diagnostics, without normalizing their outputs.

## Local checks

The final required `pr_ready.py --quick --base 98b882712308386bbb48acbc60968f7d7a991902`
run passes public-tree, formatting, 3,310 Rust tests (Game and shared client library) and
doc tests. It exits1: three known optional-release Python cases hit system Bash's
`musl[@]`/`arm64[@]` unbound variables (719 run, two skipped); workspace Clippy stops on
one untouched `horizon-core/src/storage.rs:223` `manual_is_multiple_of` lint. No unrelated
repair, suppression or pending fix is imported. The four resource-backed regressions and
217-record runtime gate pass separately. Its414 broad harnesses are listed, not run.
The full required result is not green.

Affected package-only Clippy (`horizon-client` plus `horizon-client-lib`, all targets,
`--no-deps -D warnings`) and the server configuration (`horizon-client`, all targets,
`--no-deps --no-default-features -D warnings`) both pass. These are scoped diagnostics,
not a replacement for the failed required workspace check. Later main
`18840db9146b2521dd46ad52a4559a49be180f8f` has no relevant source changes; no pair rerun
was needed for those unrelated commits.
