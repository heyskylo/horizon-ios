# Item integrity sweep

The broader gameplay audit is ongoing. This records verified findings separately
from hypotheses; it does not claim that duplication exploits have been eliminated.

## Vehicle slot commands (verified and repaired)

Baseline 48ec69857:

- `/item replace` on an open chest minecart changed its menu but left the backing
  entity's Items stale. `/data get entity ... Items` still reported the old item.
  A hopper minecart's ticking happened to reconcile its copies, masking the fault.
- Chest boats and chest rafts rejected valid container slot commands entirely.

Commands now address container vehicles consistently. Slot-name enumeration remains
side-effect-free, non-container boats have no slots, and successful writes reconcile
the live menu with the entity immediately. The pinned Java 26.3 AbstractChestBoat
getSlot delegates to ContainerEntity.getChestVehicleSlot.

`tools/lan_e2e.py --check vehicle-command-items` runs real command/menu interactions
and reads serialized entity data while the screen remains open. Three of four
baseline cases failed; all four repaired cases pass. The host build and iOS Clippy
pass. This is not a physical-device or crash/restart durability test.

## Remaining investigation

- Stale window IDs: Game::lan_inventory_click intercepts window-zero clicks before
  Hub::container_click. A suspected native-menu bypass in the Hub alone therefore
  is not proof of a reachable duplication exploit. Trace and exercise the full
  dispatch, including crafting/table/station menus, before changing authorization.
- Cursor and crafting-grid conservation during disconnect, death, menu replacement
  and vehicle destruction; include full inventories and overflow drops.
- Save/reload consistency across player inventories and world-owned containers.
- Continue beyond inventory into entity lifecycle, rendering/loading, input and
  other gameplay systems. The audit remains active.
