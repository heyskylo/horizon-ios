# Active world-pack enchantment definitions

The existing world-pack content store omitted `enchantment` in both its enumeration and content-key whitelists. Independently, `Enchantments::load_with` read only root definitions. Both omissions prevented a valid active folder-pack definition from entering the actual `Items`/`StationData`/`Game::enchantments` catalog.

The repair adds only that kind to the two existing whitelists and merges `overlay.kind("enchantment")` into the existing definition BTreeMap using `PackOverlay::top` before the existing `parse_entry`. Root definitions survive unless overridden; the last active source for an ID wins. Namespace/path mapping, schema parsing, tags, damage catalog binding, command validation, effects and RNG policy are unchanged. No fields, APIs or loader engine were added.

Pinned Java 26.3 `RegistryDataLoader` registers the enchantment catalog using `Enchantment.DIRECT_CODEC`; actual fresh Java independently admits the fixture definition and its deterministic compatible application. Historical PR #4333 included an earlier overlay merge; current inspected source lacks it. No historical or pending patch was imported.

## Separate loader provenance

Frozen base: `0ce9091c370387638dd5ecc55f2634e7369a9370`. Focused issue: [#11605](https://github.com/horizon-rust/horizon/issues/11605).

The resource-free `world_pack_definitions_join_catalog_in_overlay_priority` test writes actual JSON files and passes their bytes through the existing PackOverlay. On old source it passes vanilla/tag/damage controls, then fails because the custom definition is absent. It also checks the last of two same-ID pack definitions overrides the vanilla root entry, an unknown ID stays absent, and the existing constant damage-value effect changes 1 to 3. It never uses synchronized `with_entries` as a world-pack loader substitute.

The resource-backed `world_pack_enchantments_reach_actual_game_catalog` test uses real folder files and actual Game reload methods. It first admits a valid pack function, then reads actual `pack_files` and the authoritative catalog. Old source fails with raw resource absent and actual catalog entry absent. Fixed source must show the raw file, catalog entry, shared Items/StationData catalog identity, retained vanilla entry, supported/unsupported item compatibility, unchanged game time and the existing deterministic value effect. The initial Game test without independent function admission is retained as a rejected prerequisite; the corrected identical test is used before/after.

## Actual command pair

The predeclared final 103-record program uses a fresh real player, positively loaded chest and floor, actual identity/health/mode/position, an independently admitted pack function, fresh callback/result/success stores and strict relative clock zero. Its custom entry supports only diamond swords and has one level. An existing `enchant_randomly` modifier produces two single-count stacks per table, making both the single candidate and level deterministic. Both output slots are read separately; a third slot remains unchanged. Compatible custom and vanilla Mending swords are positive cases; incompatible stone is a negative for each.

All full feedback, styles, callbacks and saved typed values are compared strictly. Complete stack/pack/absolute-clock records remain unmodified diagnostic traces. The same final script and fresh reference bytes are used before/after. Old actual Game is **97/103**: all six differences are the custom enchantment's saved presence/value/store/level reads. All setup, callback, vanilla and incompatible controls pass. Fixed actual Game is **103/103**, with identical script/reference bytes.

Final reference SHA256: `ba862203391eba277a14c6ec6395407e821ceb0cdc6e5fd3e7c776ddb7e5d2c4`.

Final script SHA256: `7ec8dc574db4ea24bca1d0ed5c8024555ec8c16f6c6b031b617194e27bc3f1d7`.

```sh
python3 tools/test_world_pack_enchantment_loader_parity.py --reference-only --work-dir .local/enchant-java
python3 tools/test_world_pack_enchantment_loader_parity.py --binary /path/to/frozen-game \
  --reference .local/enchant-java/vanilla.jsonl --work-dir .local/enchant-game
```

Catalog membership evidence precedes the defect claim. The command fixture measures the downstream behavior of that loaded catalog; it is not evidence for a separate loot-function repair.

## Rejected inputs and unrelated diagnostics

Original v1/v2 have 91 records. Their one-stack feedback exposes an independent rarity-color discrepancy even for the correctly enchanted vanilla Mending control. v1 also incorrectly classified the custom target's enchanted style as setup. Their full programs, Java/native captures and failures remain intact; they are not qualified loader pairs. The two-stack revision changes actual inputs/feedback shape rather than normalizing output or selecting failed rows away. Its first v3 Java run incorrectly expected the success store to contain the result count 2 rather than Boolean 1; this rejected setup and its raw capture are retained. Final v4 independently passes fresh Java with correct result 2 and success 1.

Both original single-stack versions were also rerun fixed with their own unchanged scripts/references: each retains 89/91, with only `custom_sword-loot` and `vanilla_sword-loot` rarity-color feedback rows different. Their earlier mixed 84/91 outputs remain rejected prerequisites, not qualified loader baseline counts. No message/style normalization or feedback repair is included.

Historical H024457 remains unchanged and executable in `.local/h024/` as a rejected loader prerequisite. No old counts or synchronized-definition units were reused as current loader proof.

Complete manifests, all program versions, pack files, references, raw traces, source/bytecode review, native binaries and build/unit logs remain in `.local/world-pack-enchantments/`. Public original/fixed residuals are in [world-pack-enchantment-loader-residuals.jsonl](world-pack-enchantment-loader-residuals.jsonl).

## Reach and limits

The existing DataStore file channel now carries enchantment definitions to existing active-pack consumers; the existing catalog then serves items, stations and Game enchantment readers. The common file channel also supports other pack sources, but runtime proof here uses one valid folder pack. The overlay unit covers ordered same-ID override input. This is not all enchantments, schema rejection, world-pack lifecycle, network delivery or arbitrary effect parity.

H024 command registry context, H086 composition, H144 function admission, H145 tag dependencies and H146 metadata admission remain separate. Their actual neighboring diffs were reviewed; no pending production was imported.

## Local checks and source review

`pr_ready.py --quick --base 0ce9091c370387638dd5ecc55f2634e7369a9370` ran once on final production: 2,793 affected Rust tests and 27 content-root tests pass, as do formatting, Clippy, doc tests and public-tree checks. The direct catalog/priority regression is included. The actual registered Game catalog regression passed separately. Overall pr_ready exits1 solely because the 718-test Python run has three known optional-release subcase failures (masks0/1 `musl[@]`, mask2 `arm64[@]` unbound arrays), with one skip. The409 broader parity harnesses listed by --quick were not run.

Later main `507e0ae6f48d25732398e87785d74345433dc269` was reviewed read-only. Its instrument/HUD/brush/aim/stats changes are unrelated; the definition loader, enumeration/namespace path, world-table authority, instructions and command fixture driver have no relevant delta. The pair stays on frozen0ce; no pending neighboring production was imported.
