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

## Active menu ownership (verified and repaired)

A real bot opened a chest, then sent a window-zero inventory THROW packet.
The baseline server executed it and reduced the player's stone from 640 to 576.
The patched server retains all 640. This is an invalid inventory mutation;
a duplication exploit is not established by this result.

Game::lan_inventory_click intercepts window-zero clicks before Hub::container_click,
so checking only the Hub would miss this reachable path. Menu ownership is now
validated before inventory, crafting, recipe, station or native-hook dispatch.
The production-helper test covers inventory, hub containers/native menus, crafting
tables, stations, merchants, lecterns and mounts, with matching and stale IDs.

Java reference: pinned 26.3 ServerGamePacketListenerImpl.handleContainerClick,
handlePlaceRecipe, handleContainerButtonClick and handleContainerSlotStateChanged
all check the packet's menu ID against ServerPlayer.containerMenu.containerId.
The repair follows those checks rather than introducing a Horizon-only rule.
This is a window-ID check, not a new rejection rule for stale menu state IDs.
`tools/server_stale_inventory_e2e.py` exercises the actual dedicated-server path;
`tests/test_guest_menu_window.py` exercises the production ownership helper and gate.

Validation: the stale-window real-packet regression passes (640 items remain),
the ownership helper/gate matrix passes, and the iOS Clippy check passes. Live
containers, crafting, machines, stonecutter and lectern checks pass. The full
recipe-popup test also passes after waiting for the client to observe
its furnace block update. The diagnostic confirmed the client still saw air after
the command acknowledgement; a fixed frame wait was not a valid readiness check.
The fixture isolates positions and known recipes and now waits for actual block
state before interacting.

## Closing the current menu (verified and repaired)

Java 26.3 ServerGamePacketListenerImpl.handleContainerClose calls the current
menu's doCloseContainer without comparing the supplied ID. Horizon instead
matched the ID and could leave the old menu active. A real bot closed with ID 9
while chest ID 1 was open, then sent another click for ID 1: the baseline lost
64 inventory items (640 to 576). The patched server keeps 640 because the chest
is already closed. The cursor variant proves 64 move onto the cursor (576 remain
in inventory) and return exactly once on close (640), with the stale click ignored.

Closing now uses the common current-menu cleanup, preserving shared vehicle
viewers and returning cursor/station inputs; when the current menu is inventory
zero, its crafting grid is returned too. Reference is the pinned Java bytecode,
not a rule invented to reject every mismatched packet. `--case close` and
`--case cursor-close` extend the real-packet server regression. The test confirms
chest creation in a loaded chunk before launching its timed bot.

## Vehicle contents on /kill (verified and repaired)

The /kill path discarded chest minecarts, hopper minecarts, chest boats and chest
rafts without spilling their inventories. The baseline live test lost all 64
stored diamonds in each of eight cases (four vehicles, entity_drops on/off).
The final build preserves exactly 64 in all cases, counting dropped and picked-up
items together. Host compilation and the iOS Clippy check pass.

Java reference: Entity.kill calls remove(KILLED); AbstractMinecartContainer.remove
and AbstractChestBoat.remove call Containers.dropContents for destroying removal
reasons without an entity_drops gate. Containers.dropItemStack floors the entity
position and uses the same splitting/random-motion algorithm already implemented
in horizon-world::level::removal. The fix reuses that routine, preserves item
components, and spills chest-boat contents before leash removal. This scope does
not claim full parity of all attack, explosion or non-living kill-event paths.

`tools/lan_e2e.py --check vehicle-kill-contents` is the live regression. It uses
fresh worlds and validates actual entity drops and player inventory counts.

## Minecart contents after player damage (verified and repaired)

The attack/discard path also put minecart inventory drops inside entity_drops.
The live baseline lost 64 diamonds from both chest and hopper minecarts when a
creative player broke them with that rule disabled; boat/raft controls retained
their contents. The corrected path spills contents through the same container
removal routine and keeps the rule/creative checks on the vehicle item itself.

Reference: pinned Java VehicleEntity.hurtServer/discard/destroy and
AbstractMinecartContainer.remove. A destroying removal spills container contents
without the entity_drops gate. The host build and iOS Clippy pass. The real-client
`vehicle-damage-contents` and `vehicle-survival-contents` checks both pass: 16
cases across four vehicles, two rule values and creative/survival attacks.
These checks establish item conservation, not full explosion or RNG-stream parity.

## Vehicle death vibrations and TNT coverage

The live `vehicle-explosion-contents` check passes all eight cases: four container
vehicles with entity_drops on/off retain 64 stored diamonds through a TNT blast.
The explosion walk snapshots entities, so newly spilled contents are not hit by
the same blast. No additional explosion repair was needed for those cases.

A separate live sensor test found that /kill on plain/chest minecarts and boats
omitted ENTITY_DIE: all four left the sensor inactive. Java 26.3 Entity.kill
calls remove(KILLED), then gameEvent(ENTITY_DIE) at the entity position. The repair
posts that event after vehicle removal with a non-player source ID and non-living,
non-targetable context. All four cases now activate the sensor and report
last_vibration_frequency=15. Host build and iOS Clippy pass.

`tools/lan_e2e.py --check vehicle-kill-vibration` records the behavioural check.
This verifies vehicle command removal, not every non-living entity's override.

Renderer review: the earlier private-pack diagnostic still reports an optional
outline-program vaPosition translation failure. Owned renderer/translator source
already supplies the named attribute mapping; the cause has not been isolated.
No speculative shader compatibility change was made and no third-party pack
implementation source was inspected.

## Disconnect cleanup and dimension ownership (verified and repaired)

An abrupt disconnect in the Nether with a full inventory and one item in the
inventory crafting grid dropped that item in the Overworld. Cleanup switched
back to the host level before processing the leftover grid. Cleanup now finishes
in the departing player's level before that switch and before player saving.

The pinned Java InventoryMenu.removed / AbstractContainerMenu.clearContainer /
dropOrPlaceInInventory also drops held menu inputs when the player has disconnected,
even if inventory space is available. Horizon previously put them back into saved
inventory. Guest removal now marks actual disconnection, and cursor/station/grid
cleanup uses the drop path for that state while normal menu closes retain their
existing inventory-return path. New connections start with the flag cleared.

Four live abrupt-disconnect/reconnect cases pass: a full inventory with grid input,
a grid input with free inventory space, a carried cursor stack, and a crafting-table
input. Each leaves exactly one log in the Nether before reconnect and none in the
Overworld. The full case preserves all 2,304 saved stone. Normal crafting after
reconnect passes. Host compilation and iOS Clippy pass. The tests terminate only
the owned disposable client after verifying its PID and control socket.

The renderer source is unchanged since the merged snapshot; its current-source
cached test binary was rerun: 1,327 passed, 51 ignored. This is renderer component
coverage, not a claim of physical-device or complete shader-pack visual parity.

## Java fidelity requirement

Keep Java 26.3 as the behavioural reference for every gameplay repair. Verify the
relevant vanilla implementation and compare observable outcomes where practical.
Protocol compatibility alone is not a claim of matching gameplay or complete
cross-play compatibility. Tests should establish their specific scope, not imply
whole-game parity.

## Remaining investigation

- Other explosion variants and non-living entity overrides remain to audit;
  TNT vehicle conservation and vehicle /kill vibrations are verified above.

- Cursor and crafting-grid conservation during disconnect, death, menu replacement
  and vehicle destruction; include full inventories and overflow drops.
- Save/reload consistency across player inventories and world-owned containers.
- Continue beyond inventory into entity lifecycle, rendering/loading, input and
  other gameplay systems. The audit remains active.
