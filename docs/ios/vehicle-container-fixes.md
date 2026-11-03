# Vehicle container investigation

Baseline: e80f4673e, including upstream 66017a34c. Plain chest/hopper minecart
opening already works on this branch; the exact reported sound-only symptom is
not established for an unspecified desktop build. The expanded sweep reproduced
independent defects in the shared gameplay code:

- Opening a remote container at positive coordinates can overflow the client
  spawner scan's subtraction from the synthetic i32::MIN inventory key. The
  debug game panicked in tick_remote_spawners. Use abs_diff so synthetic keys
  remain outside the ticking radius without overflowing on any axis.
- Closing one viewer's vehicle menu removed the shared inventory stand-in and
  closed the other viewer's menu. Keep the stand-in until the last viewer in the
  level closes, reuse existing live slots on open, and restore the snapshot when
  replacing the same viewer's menu. Vehicle removal still explicitly releases it.
- Entity interactions used the latest movement sneak state rather than the click
  packet's secondary flag. Apply the click snapshot only within the temporary
  guest action, preserving subsequent movement input. A production-dispatch
  regression checks both directions of stale movement input.

Reference: inspected the pinned Java 26.3 bytecode for ContainerEntity's
interactWithContainerVehicle/isChestVehicleStillValid, MinecartChest's
createMenu/stopOpen, and ServerGamePacketListenerImpl.handleInteract. The menu
uses the vehicle's shared Container; closing a viewer does not delete it, and
interaction dispatch uses the packet's secondary-action flag.

Regression entry points:

- `cargo test -p horizon-client --lib client_tick_range_tests`
- `python3 -m unittest discover -s tests -p test_entity_interaction_secondary.py`
- `python3 tools/lan_e2e.py --check vehicle-containers`

The live check uses fresh worlds and actual client/server interactions. It covers
chest/hopper minecarts, oak chest boats and bamboo chest rafts for the host and LAN
guest; transfers, counts and reopening; riding inventory for boats/rafts; and two
viewers with independent close and command-edited cart contents. Audio is disabled
in the test environment. This is not a physical iPad or audible-sound verification.

Validation completed: all 12 live vehicle/container cases passed (including four
riding-inventory checks within the individual boat/raft cases), both client-range
unit tests passed, and the secondary-action dispatch regression failed before its
fix and passed afterward. Host build, scoped host Clippy and iOS Clippy passed.
The final iOS Clippy check includes all three fixes.
