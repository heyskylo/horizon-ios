# Pillager and WanderingTrader saved Inventory

Pinned Java 26.3 Pillager and AbstractVillager read/write Inventory through
InventoryCarrier. Their SimpleContainers have five and eight slots respectively.
The saved list contains occupied ItemStacks and is written even when empty.

Horizon now adopts these two species' input into the existing `extra.inventory`
model through the existing item codec and projects that model on save. It does
not substitute a constant empty list. Piglin code is unchanged; Villager keeps
its separate model. Existing AI/drop consumers of this field are Piglin-gated.
No pickup, raid, loot or trade path is changed.

## Actual Game proof

Frozen base: `41f822b659789780f58d32c63ddfb6887ea1b8bf`. Both own executables
use this base; the old build includes only the new cfg(test) regression before
any gameplay edit. One persistent checkout and fresh isolated game homes are used.

```sh
python3 tools/test_carrier_inventory_parity.py \
  --binary /absolute/path/to/horizon-client --work-dir /fresh/output
```

The predeclared 131 complete-record program has independent absent, explicit-empty
and valid single-stack cases for each species, plus pig absence and Piglin
empty/nonempty controls. Actual UUID/type/Health/Pos/Motion/NoAI/NoGravity,
loaded floor/air and exact relative clock 0 are positive admissions. Inventory
reads use fresh result/success sentinels and exact list/presence/item types.
Stone 3 with custom_data.audit Int 137 is preserved. Trader positive Offers and
Int 48000 DespawnDelay, and Pillager PatrolLeader1/CanJoinRaid0 remain strict.
Fresh pinned Java runs with `_JAVA_OPTIONS=-Xmx1G` and owned prepared resources.

Untouched Game: **123/131**, all admissions passing. Eight differences are the
read/success/presence/type observations of the two missing empty defaults, not
eight independent defects. Fixed: **131/131**. Script and fresh reference bytes
must be identical across the pair.

- Program SHA256: `a376c401c2ce2ceb07055217698f135879a9eb15b37778fce18c7033f1fd7317`
- Reference SHA256: `017c5db07aae44a17735f39a4978032f6ca32e1e7a7ba4ed1ec84d4b35ba0fab`

The resource-free real decode/save regression
`entity_nbt::carrier_inventory_tests::carrier_inventory_is_adopted_and_saved_from_the_model`
fails old on absent Pillager Inventory. It checks actual model adoption, raw
absent/present, no-raw and stale-raw saves, single-stack components, changed count4
and cleared model output. Offers/DespawnDelay and Pillager flags are retained;
Piglin and pig controls protect existing behavior. The identical fixed regression passes in the required Rust suite.

## Diagnostics and scope

Nine whole bodies and two absolute clock reads are predeclared raw traces,
retained in full in java.log/rust.log separately from the 131-record contract.
Existing Pillager CanPickUpLoot is Java1/native 0; that load-flag gap is retained,
not repaired. The fixed raw residuals are:

| Raw records | Java | Native (unchanged apart from corrected Inventory defaults) |
|---|---|---|
| 3 Pillager bodies | CanPickUpLoot1, attributes[] | CanPickUpLoot0, movement_speed base0.3499999940395355 |
| 3 trader bodies | attributes[] | movement_speed base0.7 |
| pig body | attributes[] | movement_speed base0.25 |
| 2 Piglin bodies | CannotHunt0, CanPickUpLoot1 | CannotHunt absent, CanPickUpLoot0 |
| start/end absolute clocks | old Java305 / fixed Java269 | native 0 |

Both old and fixed retain all 11 raw diagnostic differences. Before the fix the
two absent-input bodies additionally lack Inventory; the fixed bodies contain
it. Complete styled records/callbacks are retained without normalization. No full-body or
absolute-clock equality claim; no masks/tolerances in the complete canonical records.

The first Java draft expected empty Offers.Recipes to remain present; Java saves
empty Offers as {}. Its script/reference/raws remain preserved as a rejected
setup expectation. The revised positive-offer fixture is independently Java-admitted.

Runtime coverage is these vanilla species and single valid stacks below capacity.
The existing occupied-stack representation and item codec are reused, with source
bounds 5/8. SimpleContainer.fromItemList aggregation (H138), multiple-stack merging,
max-stack/capacity redesign, numeric slot access, other species and lifecycle/reopen
behavior are outside this proof. The ledger retains these limits.
