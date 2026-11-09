# Furnace output component identity

Pinned `AbstractFurnaceBlockEntity.canBurn` rejects a nonempty output unless
`ItemStack.isSameItemSameComponents` matches the assembled result, then checks
result count/capacity. H327 replaces only the item-ID comparison with the
existing `ItemStack::same_item_same_components`. Capacity arithmetic and all
input, timer, fuel, speed, XP and recipe policies remain unchanged.

The real Game/Java fixture installs its own world-only `h327:hot_iron` smelting
recipe: dirt into two iron ingots named `Hot`, cookingtime10. It admits the
actual recipe ID before processing, checks each installed JSON file and actual
recipe-use IDs/counts, and observes four separate furnaces before and after60
actual world ticks. Full initial/final slots/components, typed count/item/name
checks, loaded floor/block identity, placement ACKs and relative clocks remain
strict. No packet/UI observer or stand-in furnace is used.

| Initial output | After60 Java/fixed | Input/fuel outcome |
| --- | --- | --- |
| empty | Hot6 | input gone, coal consumed |
| plain iron1 | plain iron1 | dirt3/coal1 unchanged |
| Hot63 | Hot63 | dirt3/coal1 unchanged, capacity rejects two more |
| Hot60 | Hot64 | dirt1, coal consumed |

## Declared proof

Own runtime base:`f2f95dad8a1a701424a08bd989168297a7172b36`.
Canonical109 script SHA-256:
`b811ef94ccb317c476737307a9e9322fb3ed9b3461325617f64bc56500dc2bc0`;
fresh Java reference:
`21311e47a62f2145a47fbf50db6332f66e5ded00ac08e800071f87099908d7d4`.
The same-script/reference pair is **103/109 → 109/109**; the wrapper independently
regenerates and requires byte-identical Java reference bytes for both captures.
Every complete canonical raw record (resolved text/styles/callbacks) is compared
without masks/tolerances. All six old differences belong to plain output after60:
lit state, complete slots, input, fuel, output count and unused recipe map. Actual
recipe/initial-component admissions and all three other outcomes pass unchanged.

From before capture, whole tags, cooking/burn timers and raw absolute clock
reads are separate executable diagnostics. Full152 script SHA-256:
`3043d8e4fae91261f39eb87e75093bbc4b310d1ecbec53c236092e3ee35442a8`;
frozen fresh Java reference:
`28a8eb2432c31097c11462e42ea2956c95dcd74d362211ddcb8e694f74a74e7a`.
Original/full scripts remain executable. The fixture's default43 diagnostic
labels are exactly `clock-start`, `{0,60}-clock-raw`, and
`{empty,plain,capacity,merge}-{0,60}-{full-tag,cooking_time_spent,cooking_total_time,lit_time_remaining,lit_total_time}`.
They still execute and retain actual traces in canonical mode. Full mode compares
all152 complete records; it does not normalize H119/H191 or startup clocks.

## Regression and H119 overlap

The identical resource-free production tick test fails old because a differently
named output starts burning (`Some(true)` instead of `None`). It covers empty
output, same-component completion/merging, a full output and a merge to capacity.
The real Game fixture additionally proves the named two-item recipe count.

Pending H119/#10170 removes a separate early timer reset in this same function.
H327 is based on main and imports none of that production or test code; the
three-line timer block is deliberately still present in this branch. Keep both
changes and both independent regressions when integrating them. No combined
H119/H327 runtime or full-tag equality claim is made.

## Reproduce

With prepared26.3 resources and isolated test directories:

```sh
python3 tools/test_furnace_output_components_parity.py --reference-only --work-dir .local/h327/java
python3 tools/test_furnace_output_components_parity.py --binary /absolute/path/to/game \
  --work-dir .local/h327/native --reference .local/h327/java/vanilla.jsonl
```

Use `--full-diagnostic` on both runs for all152. Every capture retains full raw
records, logs, script, expected/actual residuals, installed recipe and reference
hashes. Full diagnostic can use its frozen reference while retaining the newly
admitted Java run's raw values separately. Absolute clock values are never
rewritten; separate actual score operations establish elapsed0/60.

## Fixed diagnostics

All six furnace helper tests pass fixed, including the identical old-failing
component regression. Full152 is **131/152 → 139/152**. The remaining13 records
are eight whole-tag H191 presentations (two also retain H119's stored-total
difference), two blocked-furnace H119 total reads, and three raw startup-clock
reads. These are retained diagnostics, not a152-green or combined-timer claim.

## Full expected/actual residuals

Tables quote exact resolved text and callbacks; complete styled-span records stay
in executable `strict-report.json` outputs. Canonical109 compares those complete
records strictly. No full diagnostic text or timer value is normalized.

### Untouched full152: 131/152

| Label | Expected | Actual | Classification |
| --- | --- | --- | --- |
| `clock-start` | `{"messages":["The game time is 321 tick(s)"],"results":[[true,321]]}` | `{"messages":["The game time is 0 tick(s)"],"results":[[true,0]]}` | Unequal raw startup clock; actual elapsed guard passes |
| `0-clock-raw` | `{"messages":["The game time is 321 tick(s)"],"results":[[true,321]]}` | `{"messages":["The game time is 0 tick(s)"],"results":[[true,0]]}` | Unequal raw startup clock; actual elapsed guard passes |
| `empty-0-full-tag` | `{"messages":["2, 100, 2 has the following block data: {lit_total_time: 0, components: {}, cooking_time_spent: 0, speed_multiplier: 1.0f, x: 2, y: 100, cooking_total_time: 0, Items: [{count: 3, Slot: 0b, id: \"minecraft:dirt\"}, {count: 1, Slot: 1b, id: \"minecraft:coal\"}], z: 2, id: \"minecraft:furnace\", lit_time_remaining: 0, RecipesUsed: {}}"],"results":[[true,1]]}` | `{"messages":["2, 100, 2 has the following block data: {lit_total_time: 0, components: {}, cooking_time_spent: 0, speed_multiplier: 1.0f, x: 2, y: 100, Items: [{count: 3, Slot: 0b, id: \"minecraft:dirt\"}, {count: 1, Slot: 1b, id: \"minecraft:coal\"}], cooking_total_time: 0, z: 2, id: \"minecraft:furnace\", lit_time_remaining: 0, RecipesUsed: {}}"],"results":[[true,1]]}` | H191 presentation |
| `plain-0-full-tag` | `{"messages":["4, 100, 2 has the following block data: {lit_total_time: 0, components: {}, cooking_time_spent: 0, speed_multiplier: 1.0f, x: 4, y: 100, cooking_total_time: 0, Items: [{count: 3, Slot: 0b, id: \"minecraft:dirt\"}, {count: 1, Slot: 1b, id: \"minecraft:coal\"}, {count: 1, Slot: 2b, id: \"minecraft:iron_ingot\"}], z: 2, id: \"minecraft:furnace\", lit_time_remaining: 0, RecipesUsed: {}}"],"results":[[true,1]]}` | `{"messages":["4, 100, 2 has the following block data: {lit_total_time: 0, components: {}, cooking_time_spent: 0, speed_multiplier: 1.0f, x: 4, y: 100, Items: [{count: 3, Slot: 0b, id: \"minecraft:dirt\"}, {count: 1, Slot: 1b, id: \"minecraft:coal\"}, {count: 1, Slot: 2b, id: \"minecraft:iron_ingot\"}], cooking_total_time: 0, z: 2, id: \"minecraft:furnace\", lit_time_remaining: 0, RecipesUsed: {}}"],"results":[[true,1]]}` | H191 presentation |
| `capacity-0-full-tag` | `{"messages":["6, 100, 2 has the following block data: {lit_total_time: 0, components: {}, cooking_time_spent: 0, speed_multiplier: 1.0f, x: 6, y: 100, cooking_total_time: 0, Items: [{count: 3, Slot: 0b, id: \"minecraft:dirt\"}, {count: 1, Slot: 1b, id: \"minecraft:coal\"}, {components: {\"minecraft:custom_name\": \"Hot\"}, count: 63, Slot: 2b, id: \"minecraft:iron_ingot\"}], z: 2, id: \"minecraft:furnace\", lit_time_remaining: 0, RecipesUsed: {}}"],"results":[[true,1]]}` | `{"messages":["6, 100, 2 has the following block data: {lit_total_time: 0, components: {}, cooking_time_spent: 0, speed_multiplier: 1.0f, x: 6, y: 100, Items: [{count: 3, Slot: 0b, id: \"minecraft:dirt\"}, {count: 1, Slot: 1b, id: \"minecraft:coal\"}, {components: {\"minecraft:custom_name\": \"Hot\"}, count: 63, Slot: 2b, id: \"minecraft:iron_ingot\"}], cooking_total_time: 0, z: 2, id: \"minecraft:furnace\", lit_time_remaining: 0, RecipesUsed: {}}"],"results":[[true,1]]}` | H191 presentation |
| `merge-0-full-tag` | `{"messages":["8, 100, 2 has the following block data: {lit_total_time: 0, components: {}, cooking_time_spent: 0, speed_multiplier: 1.0f, x: 8, y: 100, cooking_total_time: 0, Items: [{count: 3, Slot: 0b, id: \"minecraft:dirt\"}, {count: 1, Slot: 1b, id: \"minecraft:coal\"}, {components: {\"minecraft:custom_name\": \"Hot\"}, count: 60, Slot: 2b, id: \"minecraft:iron_ingot\"}], z: 2, id: \"minecraft:furnace\", lit_time_remaining: 0, RecipesUsed: {}}"],"results":[[true,1]]}` | `{"messages":["8, 100, 2 has the following block data: {lit_total_time: 0, components: {}, cooking_time_spent: 0, speed_multiplier: 1.0f, x: 8, y: 100, Items: [{count: 3, Slot: 0b, id: \"minecraft:dirt\"}, {count: 1, Slot: 1b, id: \"minecraft:coal\"}, {components: {\"minecraft:custom_name\": \"Hot\"}, count: 60, Slot: 2b, id: \"minecraft:iron_ingot\"}], cooking_total_time: 0, z: 2, id: \"minecraft:furnace\", lit_time_remaining: 0, RecipesUsed: {}}"],"results":[[true,1]]}` | H191 presentation |
| `60-clock-raw` | `{"messages":["The game time is 381 tick(s)"],"results":[[true,381]]}` | `{"messages":["The game time is 60 tick(s)"],"results":[[true,60]]}` | Unequal raw startup clock; actual elapsed guard passes |
| `empty-60-full-tag` | `{"messages":["2, 100, 2 has the following block data: {lit_total_time: 1600, components: {}, cooking_time_spent: 0, speed_multiplier: 1.0f, x: 2, y: 100, cooking_total_time: 10, Items: [{components: {\"minecraft:custom_name\": \"Hot\"}, count: 6, Slot: 2b, id: \"minecraft:iron_ingot\"}], z: 2, id: \"minecraft:furnace\", lit_time_remaining: 1541, RecipesUsed: {\"h327:hot_iron\": 3}}"],"results":[[true,1]]}` | `{"messages":["2, 100, 2 has the following block data: {lit_total_time: 1600, components: {}, cooking_time_spent: 0, speed_multiplier: 1.0f, x: 2, y: 100, Items: [{components: {\"minecraft:custom_name\": \"Hot\"}, count: 6, Slot: 2b, id: \"minecraft:iron_ingot\"}], cooking_total_time: 10, z: 2, id: \"minecraft:furnace\", lit_time_remaining: 1541, RecipesUsed: {\"h327:hot_iron\": 3}}"],"results":[[true,1]]}` | H191 presentation |
| `plain-60-lit` | `{"messages":["Test passed"],"results":[[true,1]]}` | `{"messages":["Test failed"],"results":[[false,0]]}` | Incompatible-output processing |
| `plain-60-slots` | `{"messages":["4, 100, 2 has the following block data: [{count: 3, Slot: 0b, id: \"minecraft:dirt\"}, {count: 1, Slot: 1b, id: \"minecraft:coal\"}, {count: 1, Slot: 2b, id: \"minecraft:iron_ingot\"}]"],"results":[[true,3]]}` | `{"messages":["4, 100, 2 has the following block data: [{count: 7, Slot: 2b, id: \"minecraft:iron_ingot\"}]"],"results":[[true,1]]}` | Incompatible-output processing |
| `plain-60-input` | `{"messages":["Test passed. Count: 1"],"results":[[true,1]]}` | `{"messages":["Test failed"],"results":[[false,0]]}` | Incompatible-output processing |
| `plain-60-fuel` | `{"messages":["Test passed. Count: 1"],"results":[[true,1]]}` | `{"messages":["Test failed"],"results":[[false,0]]}` | Incompatible-output processing |
| `plain-60-output` | `{"messages":["Test passed. Count: 1"],"results":[[true,1]]}` | `{"messages":["Test failed"],"results":[[false,0]]}` | Incompatible-output processing |
| `plain-60-unused-recipe` | `{"messages":["4, 100, 2 has the following block data: {}"],"results":[[true,0]]}` | `{"messages":["4, 100, 2 has the following block data: {\"h327:hot_iron\": 3}"],"results":[[true,1]]}` | Incompatible-output processing |
| `plain-60-full-tag` | `{"messages":["4, 100, 2 has the following block data: {lit_total_time: 0, components: {}, cooking_time_spent: 0, speed_multiplier: 1.0f, x: 4, y: 100, cooking_total_time: 0, Items: [{count: 3, Slot: 0b, id: \"minecraft:dirt\"}, {count: 1, Slot: 1b, id: \"minecraft:coal\"}, {count: 1, Slot: 2b, id: \"minecraft:iron_ingot\"}], z: 2, id: \"minecraft:furnace\", lit_time_remaining: 0, RecipesUsed: {}}"],"results":[[true,1]]}` | `{"messages":["4, 100, 2 has the following block data: {lit_total_time: 1600, components: {}, cooking_time_spent: 0, speed_multiplier: 1.0f, x: 4, y: 100, Items: [{count: 7, Slot: 2b, id: \"minecraft:iron_ingot\"}], cooking_total_time: 10, z: 2, id: \"minecraft:furnace\", lit_time_remaining: 1541, RecipesUsed: {\"h327:hot_iron\": 3}}"],"results":[[true,1]]}` | H191 presentation + incompatible-output processing/H119 overlap |
| `plain-60-cooking_total_time` | `{"messages":["4, 100, 2 has the following block data: 0"],"results":[[true,0]]}` | `{"messages":["4, 100, 2 has the following block data: 10"],"results":[[true,10]]}` | Processing/H119 overlap |
| `plain-60-lit_time_remaining` | `{"messages":["4, 100, 2 has the following block data: 0"],"results":[[true,0]]}` | `{"messages":["4, 100, 2 has the following block data: 1541"],"results":[[true,1541]]}` | Incompatible-output processing |
| `plain-60-lit_total_time` | `{"messages":["4, 100, 2 has the following block data: 0"],"results":[[true,0]]}` | `{"messages":["4, 100, 2 has the following block data: 1600"],"results":[[true,1600]]}` | Incompatible-output processing |
| `capacity-60-full-tag` | `{"messages":["6, 100, 2 has the following block data: {lit_total_time: 0, components: {}, cooking_time_spent: 0, speed_multiplier: 1.0f, x: 6, y: 100, cooking_total_time: 0, Items: [{count: 3, Slot: 0b, id: \"minecraft:dirt\"}, {count: 1, Slot: 1b, id: \"minecraft:coal\"}, {components: {\"minecraft:custom_name\": \"Hot\"}, count: 63, Slot: 2b, id: \"minecraft:iron_ingot\"}], z: 2, id: \"minecraft:furnace\", lit_time_remaining: 0, RecipesUsed: {}}"],"results":[[true,1]]}` | `{"messages":["6, 100, 2 has the following block data: {lit_total_time: 0, components: {}, cooking_time_spent: 0, speed_multiplier: 1.0f, x: 6, y: 100, Items: [{count: 3, Slot: 0b, id: \"minecraft:dirt\"}, {count: 1, Slot: 1b, id: \"minecraft:coal\"}, {components: {\"minecraft:custom_name\": \"Hot\"}, count: 63, Slot: 2b, id: \"minecraft:iron_ingot\"}], cooking_total_time: 10, z: 2, id: \"minecraft:furnace\", lit_time_remaining: 0, RecipesUsed: {}}"],"results":[[true,1]]}` | H191 presentation + H119 total |
| `capacity-60-cooking_total_time` | `{"messages":["6, 100, 2 has the following block data: 0"],"results":[[true,0]]}` | `{"messages":["6, 100, 2 has the following block data: 10"],"results":[[true,10]]}` | H119 total retained |
| `merge-60-full-tag` | `{"messages":["8, 100, 2 has the following block data: {lit_total_time: 1600, components: {}, cooking_time_spent: 0, speed_multiplier: 1.0f, x: 8, y: 100, cooking_total_time: 10, Items: [{count: 1, Slot: 0b, id: \"minecraft:dirt\"}, {components: {\"minecraft:custom_name\": \"Hot\"}, count: 64, Slot: 2b, id: \"minecraft:iron_ingot\"}], z: 2, id: \"minecraft:furnace\", lit_time_remaining: 1541, RecipesUsed: {\"h327:hot_iron\": 2}}"],"results":[[true,1]]}` | `{"messages":["8, 100, 2 has the following block data: {lit_total_time: 1600, components: {}, cooking_time_spent: 0, speed_multiplier: 1.0f, x: 8, y: 100, Items: [{count: 1, Slot: 0b, id: \"minecraft:dirt\"}, {components: {\"minecraft:custom_name\": \"Hot\"}, count: 64, Slot: 2b, id: \"minecraft:iron_ingot\"}], cooking_total_time: 10, z: 2, id: \"minecraft:furnace\", lit_time_remaining: 1541, RecipesUsed: {\"h327:hot_iron\": 2}}"],"results":[[true,1]]}` | H191 presentation |

### Fixed full152: 139/152

| Label | Expected | Actual | Classification |
| --- | --- | --- | --- |
| `clock-start` | `{"messages":["The game time is 321 tick(s)"],"results":[[true,321]]}` | `{"messages":["The game time is 0 tick(s)"],"results":[[true,0]]}` | Unequal raw startup clock; actual elapsed guard passes |
| `0-clock-raw` | `{"messages":["The game time is 321 tick(s)"],"results":[[true,321]]}` | `{"messages":["The game time is 0 tick(s)"],"results":[[true,0]]}` | Unequal raw startup clock; actual elapsed guard passes |
| `empty-0-full-tag` | `{"messages":["2, 100, 2 has the following block data: {lit_total_time: 0, components: {}, cooking_time_spent: 0, speed_multiplier: 1.0f, x: 2, y: 100, cooking_total_time: 0, Items: [{count: 3, Slot: 0b, id: \"minecraft:dirt\"}, {count: 1, Slot: 1b, id: \"minecraft:coal\"}], z: 2, id: \"minecraft:furnace\", lit_time_remaining: 0, RecipesUsed: {}}"],"results":[[true,1]]}` | `{"messages":["2, 100, 2 has the following block data: {lit_total_time: 0, components: {}, cooking_time_spent: 0, speed_multiplier: 1.0f, x: 2, y: 100, Items: [{count: 3, Slot: 0b, id: \"minecraft:dirt\"}, {count: 1, Slot: 1b, id: \"minecraft:coal\"}], cooking_total_time: 0, z: 2, id: \"minecraft:furnace\", lit_time_remaining: 0, RecipesUsed: {}}"],"results":[[true,1]]}` | H191 presentation |
| `plain-0-full-tag` | `{"messages":["4, 100, 2 has the following block data: {lit_total_time: 0, components: {}, cooking_time_spent: 0, speed_multiplier: 1.0f, x: 4, y: 100, cooking_total_time: 0, Items: [{count: 3, Slot: 0b, id: \"minecraft:dirt\"}, {count: 1, Slot: 1b, id: \"minecraft:coal\"}, {count: 1, Slot: 2b, id: \"minecraft:iron_ingot\"}], z: 2, id: \"minecraft:furnace\", lit_time_remaining: 0, RecipesUsed: {}}"],"results":[[true,1]]}` | `{"messages":["4, 100, 2 has the following block data: {lit_total_time: 0, components: {}, cooking_time_spent: 0, speed_multiplier: 1.0f, x: 4, y: 100, Items: [{count: 3, Slot: 0b, id: \"minecraft:dirt\"}, {count: 1, Slot: 1b, id: \"minecraft:coal\"}, {count: 1, Slot: 2b, id: \"minecraft:iron_ingot\"}], cooking_total_time: 0, z: 2, id: \"minecraft:furnace\", lit_time_remaining: 0, RecipesUsed: {}}"],"results":[[true,1]]}` | H191 presentation |
| `capacity-0-full-tag` | `{"messages":["6, 100, 2 has the following block data: {lit_total_time: 0, components: {}, cooking_time_spent: 0, speed_multiplier: 1.0f, x: 6, y: 100, cooking_total_time: 0, Items: [{count: 3, Slot: 0b, id: \"minecraft:dirt\"}, {count: 1, Slot: 1b, id: \"minecraft:coal\"}, {components: {\"minecraft:custom_name\": \"Hot\"}, count: 63, Slot: 2b, id: \"minecraft:iron_ingot\"}], z: 2, id: \"minecraft:furnace\", lit_time_remaining: 0, RecipesUsed: {}}"],"results":[[true,1]]}` | `{"messages":["6, 100, 2 has the following block data: {lit_total_time: 0, components: {}, cooking_time_spent: 0, speed_multiplier: 1.0f, x: 6, y: 100, Items: [{count: 3, Slot: 0b, id: \"minecraft:dirt\"}, {count: 1, Slot: 1b, id: \"minecraft:coal\"}, {components: {\"minecraft:custom_name\": \"Hot\"}, count: 63, Slot: 2b, id: \"minecraft:iron_ingot\"}], cooking_total_time: 0, z: 2, id: \"minecraft:furnace\", lit_time_remaining: 0, RecipesUsed: {}}"],"results":[[true,1]]}` | H191 presentation |
| `merge-0-full-tag` | `{"messages":["8, 100, 2 has the following block data: {lit_total_time: 0, components: {}, cooking_time_spent: 0, speed_multiplier: 1.0f, x: 8, y: 100, cooking_total_time: 0, Items: [{count: 3, Slot: 0b, id: \"minecraft:dirt\"}, {count: 1, Slot: 1b, id: \"minecraft:coal\"}, {components: {\"minecraft:custom_name\": \"Hot\"}, count: 60, Slot: 2b, id: \"minecraft:iron_ingot\"}], z: 2, id: \"minecraft:furnace\", lit_time_remaining: 0, RecipesUsed: {}}"],"results":[[true,1]]}` | `{"messages":["8, 100, 2 has the following block data: {lit_total_time: 0, components: {}, cooking_time_spent: 0, speed_multiplier: 1.0f, x: 8, y: 100, Items: [{count: 3, Slot: 0b, id: \"minecraft:dirt\"}, {count: 1, Slot: 1b, id: \"minecraft:coal\"}, {components: {\"minecraft:custom_name\": \"Hot\"}, count: 60, Slot: 2b, id: \"minecraft:iron_ingot\"}], cooking_total_time: 0, z: 2, id: \"minecraft:furnace\", lit_time_remaining: 0, RecipesUsed: {}}"],"results":[[true,1]]}` | H191 presentation |
| `60-clock-raw` | `{"messages":["The game time is 381 tick(s)"],"results":[[true,381]]}` | `{"messages":["The game time is 60 tick(s)"],"results":[[true,60]]}` | Unequal raw startup clock; actual elapsed guard passes |
| `empty-60-full-tag` | `{"messages":["2, 100, 2 has the following block data: {lit_total_time: 1600, components: {}, cooking_time_spent: 0, speed_multiplier: 1.0f, x: 2, y: 100, cooking_total_time: 10, Items: [{components: {\"minecraft:custom_name\": \"Hot\"}, count: 6, Slot: 2b, id: \"minecraft:iron_ingot\"}], z: 2, id: \"minecraft:furnace\", lit_time_remaining: 1541, RecipesUsed: {\"h327:hot_iron\": 3}}"],"results":[[true,1]]}` | `{"messages":["2, 100, 2 has the following block data: {lit_total_time: 1600, components: {}, cooking_time_spent: 0, speed_multiplier: 1.0f, x: 2, y: 100, Items: [{components: {\"minecraft:custom_name\": \"Hot\"}, count: 6, Slot: 2b, id: \"minecraft:iron_ingot\"}], cooking_total_time: 10, z: 2, id: \"minecraft:furnace\", lit_time_remaining: 1541, RecipesUsed: {\"h327:hot_iron\": 3}}"],"results":[[true,1]]}` | H191 presentation |
| `plain-60-full-tag` | `{"messages":["4, 100, 2 has the following block data: {lit_total_time: 0, components: {}, cooking_time_spent: 0, speed_multiplier: 1.0f, x: 4, y: 100, cooking_total_time: 0, Items: [{count: 3, Slot: 0b, id: \"minecraft:dirt\"}, {count: 1, Slot: 1b, id: \"minecraft:coal\"}, {count: 1, Slot: 2b, id: \"minecraft:iron_ingot\"}], z: 2, id: \"minecraft:furnace\", lit_time_remaining: 0, RecipesUsed: {}}"],"results":[[true,1]]}` | `{"messages":["4, 100, 2 has the following block data: {lit_total_time: 0, components: {}, cooking_time_spent: 0, speed_multiplier: 1.0f, x: 4, y: 100, Items: [{count: 3, Slot: 0b, id: \"minecraft:dirt\"}, {count: 1, Slot: 1b, id: \"minecraft:coal\"}, {count: 1, Slot: 2b, id: \"minecraft:iron_ingot\"}], cooking_total_time: 10, z: 2, id: \"minecraft:furnace\", lit_time_remaining: 0, RecipesUsed: {}}"],"results":[[true,1]]}` | H191 presentation + H119 total |
| `plain-60-cooking_total_time` | `{"messages":["4, 100, 2 has the following block data: 0"],"results":[[true,0]]}` | `{"messages":["4, 100, 2 has the following block data: 10"],"results":[[true,10]]}` | H119 total retained |
| `capacity-60-full-tag` | `{"messages":["6, 100, 2 has the following block data: {lit_total_time: 0, components: {}, cooking_time_spent: 0, speed_multiplier: 1.0f, x: 6, y: 100, cooking_total_time: 0, Items: [{count: 3, Slot: 0b, id: \"minecraft:dirt\"}, {count: 1, Slot: 1b, id: \"minecraft:coal\"}, {components: {\"minecraft:custom_name\": \"Hot\"}, count: 63, Slot: 2b, id: \"minecraft:iron_ingot\"}], z: 2, id: \"minecraft:furnace\", lit_time_remaining: 0, RecipesUsed: {}}"],"results":[[true,1]]}` | `{"messages":["6, 100, 2 has the following block data: {lit_total_time: 0, components: {}, cooking_time_spent: 0, speed_multiplier: 1.0f, x: 6, y: 100, Items: [{count: 3, Slot: 0b, id: \"minecraft:dirt\"}, {count: 1, Slot: 1b, id: \"minecraft:coal\"}, {components: {\"minecraft:custom_name\": \"Hot\"}, count: 63, Slot: 2b, id: \"minecraft:iron_ingot\"}], cooking_total_time: 10, z: 2, id: \"minecraft:furnace\", lit_time_remaining: 0, RecipesUsed: {}}"],"results":[[true,1]]}` | H191 presentation + H119 total |
| `capacity-60-cooking_total_time` | `{"messages":["6, 100, 2 has the following block data: 0"],"results":[[true,0]]}` | `{"messages":["6, 100, 2 has the following block data: 10"],"results":[[true,10]]}` | H119 total retained |
| `merge-60-full-tag` | `{"messages":["8, 100, 2 has the following block data: {lit_total_time: 1600, components: {}, cooking_time_spent: 0, speed_multiplier: 1.0f, x: 8, y: 100, cooking_total_time: 10, Items: [{count: 1, Slot: 0b, id: \"minecraft:dirt\"}, {components: {\"minecraft:custom_name\": \"Hot\"}, count: 64, Slot: 2b, id: \"minecraft:iron_ingot\"}], z: 2, id: \"minecraft:furnace\", lit_time_remaining: 1541, RecipesUsed: {\"h327:hot_iron\": 2}}"],"results":[[true,1]]}` | `{"messages":["8, 100, 2 has the following block data: {lit_total_time: 1600, components: {}, cooking_time_spent: 0, speed_multiplier: 1.0f, x: 8, y: 100, Items: [{count: 1, Slot: 0b, id: \"minecraft:dirt\"}, {components: {\"minecraft:custom_name\": \"Hot\"}, count: 64, Slot: 2b, id: \"minecraft:iron_ingot\"}], cooking_total_time: 10, z: 2, id: \"minecraft:furnace\", lit_time_remaining: 1541, RecipesUsed: {\"h327:hot_iron\": 2}}"],"results":[[true,1]]}` | H191 presentation |

## Exact absolute clocks retained

Every absolute value below remains unchanged in raw records/traces. The separate
actual score operations require elapsed0/60. Full diagnostic compares its frozen
Java reference while newly admitted Java outputs remain separately intact.

| Capture | Actual Java start/end | Native start/end |
| --- | --- | --- |
| `java-canonical` | 309/369 | not run |
| `old-canonical` | 317/377 | 0/60 |
| `fixed-canonical` | 293/353 | 0/60 |
| `java-full` | 321/381 | not run |
| `old-full` | 309/369 | 0/60 |
| `fixed-full` | 305/365 | 0/60 |

## Local readiness

All six furnace tests pass fixed. `pr_ready.py --quick` passes formatting,
workspace/headless Clippy, doc and public-tree checks;1,880 Rust tests pass.
The single `drop_motion` one-bit Z failure at37.5/-21.25 reproduces identically
in the archived untouched binary. Python686 retains four unchanged packaging
failures/one skip (ARM mock installer release and Bash3.2 optional-archive arrays).
No related files are changed. Final evidence additions get repeated whitespace/
public-tree checks. Main is actually pulled through`5b4823c19`; reviewed furnace,
tick, component, recipe and codec sources remain unchanged from the paired base.
No later whole-tree or combined H119 runtime claim is made.
