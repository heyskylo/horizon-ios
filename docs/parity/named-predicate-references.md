# Named deterministic predicate references (H121)

The measured repair binds named predicate IDs to the actual loaded Game store. The existing root lookup works, but strings inside condition references previously fell through to false. The additive read-only `World::predicate` defaults to `None`; Game's `View` returns `fns.store.predicates.get(id)`. Evaluation keeps the same `Entities`, optional source and origin.

## Frozen valid-graph contract

Base: `4774d07dd515449aeadd9a23deca70e30be871ea`. A valid format-121 pack supplies direct pig/cow entity predicates, a named reference, inversion, conjunction, an always-false conjunction and disjunction. Two real UUID-bound NoAI/NoGravity actors have independently admitted type, Health10, position, floor and loaded-chunk guards. A separate valid function changes its own score to prove pack admission. No random-chance predicate or invalid function/tag dependency is used.

The predeclared **282 complete records** keep both if/unless action paths, exact messages/styles/callbacks, fresh branch/result/success stores, direct true/false controls, top-level unknown-name errors and relative-clock zero strict. Full bodies, pack lists and absolute startup clocks are separately declared raw diagnostics. The root unknown-name control is a command error in an otherwise valid world; it is distinct from an invalid referenced graph.

Untouched Game matches **242/282**. All 40 differences are pig reference/inversion/conjunction/disjunction and cow disjunction. Direct predicates, top-level unknown-name behavior and setup/clock controls match. The fixed Game matches **282/282** using the unchanged program and byte-identical fresh Java reference.

- Program SHA-256: `bf943b7680654fa6e7f9c8f0ffc9cdc25b0882548576e86327eb4dc25720f653`.
- Emitted script SHA-256: `e8b73fbbb2bd97c5136900c6185c523a3dab488c2032f886006d7cef5f8cf216`.
- Fresh Java reference SHA-256: `cf244127847404f2582af9cc64d4f488cde1a22416ba75a016f8ca79bbc216cb`.

```sh
PATH=/opt/homebrew/opt/openjdk/bin:$PATH PYTHONPATH=tools \
  python3 tools/test_named_predicate_reference_parity.py \
  --work-dir .local/named-predicate-java --reference-only
PATH=/opt/homebrew/opt/openjdk/bin:$PATH PYTHONPATH=tools \
  python3 tools/test_named_predicate_reference_parity.py \
  --work-dir .local/named-predicate-game --binary /path/to/horizon-client \
  --reference .local/named-predicate-java/vanilla.jsonl
```

Both runtime runs use the same script and freshly capture Java, checking its canonical bytes against the retained reference. No masks, tolerance or output normalization are applied. The first Java actor setup incorrectly assumed cow Health20; its actual saved Health10 rejected that prerequisite. Original v1 input/reference/raw output remain executable and retained, with no native defect count. Corrected v2 was declared before new captures; its cases and conditions are unchanged.

## Whole-graph safety and source reach

Official `LootItemCondition.CODEC` uses registry holders. A separate private reference check walks only typed condition positions (condition arrays, inverted term, all_of/any_of terms and string IDs), canonicalizes names through the existing helper, and tracks the active path. Missing references and cycles return a distinct invalid result before boolean evaluation. They cannot become true inside an inverted node or be hidden behind a boolean-skipped branch. Shared acyclic references are allowed; no arbitrary depth cap is introduced. Existing valid boolean short-circuit evaluation remains unchanged.

The actual Game regression loads predicate files from a real pack before testing registered pig/cow actors through commands. Its intended old assertion fails on the pig reference and passes fixed. It also checks valid inversion/composition, repeated references, a 33-file acyclic chain, canonical-name cycles, an actual server-source factory with no entity, unchanged clock and defensive missing/cycle graphs. No resolved JSON is manually supplied as the target proof. The test-only initial compile correction for the current JavaString API is retained separately; the identical corrected unit is used old/fixed.

The shared source reach includes existing `adv::conditions` users and Game `View` consumers: named/inline execute predicates, selector predicates, advancement criteria and item-filter conditions. Other `World` implementations retain absent lookup by default. Runtime proof covers the measured command route and already-populated pig/cow facts; it does not establish every reference context, player/guest attribution or additional fact projection. Predicate-tag holder-set forms and additional fact projection remain unmeasured. No RNG/provider behavior, advancement producer, generic loot evaluator or registry engine changes.

## Invalid-world diagnostics: not false evaluations

Independent fresh Java runs establish that invalidity applies at load time, before command execution, including boolean-skipped branches:

| Isolated form | Java outcome |
| --- | --- |
| direct missing referenced ID | unbound registry value; world creation fails |
| indirect/nested missing | unbound registry value; world creation fails |
| inverted missing | unbound registry value; world creation fails |
| any_of reached missing | unbound registry value; world creation fails |
| any_of skipped missing | unbound registry value; world creation fails |
| all_of reached missing | unbound registry value; world creation fails |
| all_of skipped missing | unbound registry value; world creation fails |
| direct cycle | recursive-reference validation; world creation fails |
| inverted cycle | recursive-reference validation; world creation fails |
| any_of reached cycle | recursive-reference validation; world creation fails |
| any_of skipped cycle | recursive-reference validation; world creation fails |
| all_of reached cycle | recursive-reference validation; world creation fails |
| all_of skipped cycle | recursive-reference validation; world creation fails |

The valid isolated control admits 43 diagnostic records. The combined invalid probe also fails startup and is retained. None of the invalid worlds supplies command-execution results; absent output remains absent. [The complete public JSONL artifact](named-predicate-invalid-worlds.jsonl) contains exact original programs, every pack/script, full Java logs and actual exit results. Its first record holds executable diagnostic scripts; extract them under the recorded filenames and run with `PYTHONPATH=tools` and a fresh `--work-dir`. Do not treat their expected startup failures as a canonical parity gate.

Defensive Game/evaluator checks are **safety/structural controls**, not Java startup or command-availability proof. No loader or registry lifecycle changes are made. The independent `predicate-reference-graph-loader-validation` lead remains unassigned and native-startup-untested; it needs its own current startup/availability capture and registry lifecycle design before repair. Invalid graph `unless`/availability behavior is not part of this command contract. H122 RNG, H144 ordinary function admission, H145 required function tags and H146 metadata remain separate.

All declarations, corrected/rejected scripts, raw bodies/traces, Java loader failures, reference files, binaries and source/check manifests are retained under `.local/h121/`. [The residual report](named-predicate-reference-residuals.md) publishes all 40 old strict differences and raw diagnostics. No full-body, raw absolute-clock, full-registry or all-reference-context equality is claimed.

Runtime artifact SHA-256 values:

- `old-game`: `e245e98296e8236495d2aa8f0436ac072b5cfeeadd554d854603c908afef4559`.
- `fixed-game`: `afd57162d570741a8bf0a1e4e1296fc1782ab15eec5c9438b91ee7f8f90caa73`.
