# Effective item command component validation

On frozen base `13f70ad482df0a548e1954df8da4b967be3183cb`, actual Game/fresh pinned Java 26.3 item commands give:

| Contract | Untouched | Fixed |
| --- | ---: | ---: |
| Predeclared validation139 | 111/139 | 139/139 |
| Original full141 diagnostic | 111/141 | 139/141 |

The 28 validation differences were three invalid inputs executing and replacing a separately admitted emerald4 sentinel. All setup admissions and valid counterparts passed. Both builds use identical script/reference bytes. The complete full141 expected/actual table is [retained separately](effective-item-components-residuals.md), including all 30 old and two fixed residuals.

## Cases and admission

Each case independently resets a real loaded chest slot0 to emerald4 with `custom_data={sentinel:86}` and verifies it before the tested replacement. Slot1 stone7 with unrelated custom data remains unchanged. Real owner UUID/type/Health20/creative mode/position, chest block entity identity/coordinates, stone support, loaded chunk and actual relative clock0 are positive controls. Every command feedback/style/callback and fresh result/success store remains strict.

| Input | Java admission |
| --- | --- |
| iron_helmet[max_stack_size=2,custom_data={keep:86}] | Reject, sentinel unchanged |
| iron_helmet[max_stack_size=99,custom_data={keep:86}] | Reject, sentinel unchanged |
| iron_helmet | Accept |
| stone[max_stack_size=2,custom_data={keep:86}] | Accept |
| iron_helmet[!max_damage,max_stack_size=2,custom_data={keep:86}] | Accept validation; saved removal is separate |
| stone[max_damage=10,max_stack_size=2,custom_data={keep:86}] | Reject, sentinel unchanged |
| stone[max_stack_size=99,custom_data={keep:86}] | Accept |

The revised program was declared before fresh Java and both runs. Exactly these two full141 labels move to diagnostics in validation139:

- `removed-two-saved-stack`
- `removed-two-saved-components."!minecraft:max_damage"`

All removal command ACKs, callbacks/stores, other saved fields and all seven independent admissions remain strict. This is validation parity, not effective runtime removal. The existing item-command-removal-patch-discard bridge still loses the saved removal marker. No complete141 parity claim is made.

## Source boundary

Pinned `ItemInput.createItemStack` checks count first, then `ItemStack.validateStrict`/`validateComponents`: MAX_DAMAGE presence and MAX_STACK_SIZE above1 are incompatible. Missing MAX_STACK_SIZE uses1. The existing command count check stays first.

`Game::item_from_input` now builds a local read-only view from actual loaded `Items` prototype keys/values and the already validated ItemInput component slice. Existing identifier/SNBT parsers recover typed values/removals into the existing `component_patch::Patch`/`ComponentMap`. The view honors removal before testing presence. It does not use the materialized stack as a removal witness. No new ItemInput/context field, decoder, registry, global numeric helper or materialization change is involved.

Only this composition rule is added; other strict component rules are outside this proof. Existing effective-component predicate reads, full inventories and absolute clocks remain unmodified raw diagnostics. Those predicates read saved patches rather than all prototype defaults; they are not progress/setup substitutes. Absolute startup clocks differ and are not compared; actual relative delta0 is strict.

## Reproduce

Use prepared 26.3 resources and the existing command harness/JDK:

```sh
PYTHONPATH=tools python3 tools/test_effective_item_components_parity.py --reference-only --work-dir .local/h086/new-java
PYTHONPATH=tools python3 tools/test_effective_item_components_parity.py --reference .local/h086/new-java/vanilla.jsonl --binary /absolute/path/to/game --work-dir .local/h086/new-game
PYTHONPATH=tools python3 tools/test_effective_item_components_parity.py --full-removal-diagnostic --binary /absolute/path/to/game --work-dir .local/h086/new-full141
```

The public harness generates exactly the original141 or declared139 operations. Every run retains its declaration, complete script, JSONL records, logs and strict expected/actual residual report. A full141 diagnostic with residuals exits nonzero intentionally. Frozen pair hashes:

- validation139 script source: `5481adc8eda230e2caac9735cfbf6c272728992b5b995688b9362f851b12a253`
- validation139 emitted script: `7162bc99f0f35a30463376c94d8b22aef6369c3cd7f8015d1b2fd97582de200c`
- validation139 Java reference: `96b6c4eef9959972ddba90c1b64609052e496823d698457fc002975de183991c`
- original full141 script source: `9c041ebef298f99b045c1c4e9b6cb11e6c470e912aa31f879d3bb0ef3abd777c`

The public wrapper adds the diagnostic flag; operation equality with both preserved scripts was checked. Private originals/captures remain immutable under `.local/h086`. The old runtime is the independently built pristine13f artifact with verified SHA-256 `c462c800fb8effe46bc35698d10624606fe5bba4c145c47847e10c14dae083e2`.

## Regression and limits

An actual Game command regression fails untouched at the intended helmet size2 rejection and passes fixed, checking unchanged destination plus valid helmet/stone/removal controls. A second focused Game view unit uses actual loaded Items, prototype presence/values, typed removal, unrelated custom data and removed-size fallback1. Both fixed units pass. An initial wrong-package unit build ran no target regression and is retained as a rejected build prerequisite; the qualified old unit uses the actual Game library.

No removal-persistence, wear, other component validation, natural item consumer or full item-family claim is made. Source reach is the shared `item_from_input` command boundary; runtime coverage is the seven chest replacements above. H024 enchantment registry validation and H088 saved default encoding remain independent.
