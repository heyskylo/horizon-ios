# Mobile integration through upstream f40d4df78

This local merge incorporates 707 commits after 66017a34c. It preserves native
iOS launcher lifecycle, input/clipboard/orientation, Microsoft account handling,
server-pack/NPC rendering and the verified item-integrity repairs.

Integration decisions:

- Keep packed terrain compact while moving tint and exact block/AO metadata in
  the same sort. Rebuild geometry through the existing layout transition when
  legacy shader packs need the unpacked representation.
- Retain asynchronous shader compilation and per-page shader bindings; carry the
  new frame format into owned background requests. Integrate atlas memory,
  blending/default-buffer and one-dimensional image changes.
- Use upstream entity join-order ticking; remove the old orb batch to avoid
  ticking or consuming random values twice. Keep TNT float drag/attribution and
  overflow-safe spawner selection.
- Preserve current-menu checks and close semantics, vehicle live-inventory and
  removal fixes, and disconnected-player cleanup in the player's dimension.
- Preserve launcher memory suspension and the iOS Mach sampler while adopting
  upstream memory-pressure policy changes.
- Own and cancel both culling workers, wake idle receivers, poll completion during
  launcher shutdown, and join before releasing their game state. Targeted tests
  cover idle shutdown/idempotence and canceled work.
- Preserve fork-disabled upstream deployment workflows and upstream-only automation
  guards. The optional pack benchmark falls back to hosted runners on this fork.
  Desktop mod-tool publication remains upstream-only. Keep safe fetch argument
  handling and quote the CPU-baseline check's binary paths.
- Update the Android workspace lock for the new shared networking dependencies.

The pending mobile README draft stays in the original working tree for user review;
upstream desktop README edits were not applied to that file. No push is authorized
by this merge step. Runtime and build validation follow the conflict-resolution
checkpoint; formatting, workflow lint and public-tree checks pass at that point.
