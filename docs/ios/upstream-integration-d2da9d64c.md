# Mobile integration through upstream d2da9d64c

This snapshot follows `3db53df3b`. It retains native iOS launcher/input/AirPlay
behavior, the multiplayer title-input repair, resource-cache safety and complete
engine shutdown while integrating upstream GPU culling, cosmetics (off by default),
renderer changes and mod API updates.

Merge decisions:

- Move GPU-culling records together with the fork's terrain metadata, preserving
  exact block IDs, AO and item metadata. Keep fail-closed shader resizing and
  deep-stack construction when building uncached pipelines.
- Preserve named-dimension/conditional shader settings and feature-macro setup.
  Intentional cloud disabling is not an incomplete shader pack.
- Preserve Java-compatible world-link validation and allowlists when showing the
  new upstream symlink warning. Rejection must leave the target and lock link intact.
- Suspend both soft and hard memory enforcement in the iOS launcher. Keep configured
  caps and live-allocation accounting so Play resumes the same limits. Android
  declares the shared entry point's allocator diagnostic features explicitly.
- Retain the player-only piston destruction repair, Java wrapping relative moves,
  NPC skin handling and attachment cleanup across upstream refactoring.

Focused validation includes shader tests, renderer/Metal GPU-culling and depth
checks, and complete private Complementary/Solas pipelines in all three dimensions.
GPU floating-point positions retain a small rounding tolerance while packed
attributes are checked exactly. Full-world visual and physical-device compatibility
are not inferred from these tests.

The same PR adds audited, unsigned iOS prereleases after successful trusted `main`
builds. See `packaging/ios/README.md` for release tags, build numbers, checksums,
publication guards and retry behavior. PR/fork builds cannot publish releases.
