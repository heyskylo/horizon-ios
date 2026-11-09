# Horizon Rust rewrite status

The behaviour target is the pinned **Minecraft Java Edition 26.3** (`crates/horizon-core/src/version.rs`). One-to-one
parity with vanilla is the acceptance criterion; it has not been reached everywhere, but the game is playable: single
player on its server thread, LAN, dedicated servers, survival and creative, vanilla terrain and saves.

This page doesn't list what is ported, because such lists drift. The live sources are:

- **The harnesses.** Each `tools/test_<name>_parity.py` runs a Java oracle on the real 26.3 jars and a Rust probe on
  the same inputs and compares their output. `python3 tools/parity.py --list` names them and `--status` shows their
  last results; [parity-workflow.md](parity-workflow.md) says how to run and add one. A harness is the spec for its
  area: never delete one.
- **The port ledger.** `python3 tools/port_ledger.py where Class.method` maps a vanilla method to the Rust that ports
  it, and `issue N` / `check` list the gaps ([port-ledger.md](port-ledger.md)).
- **What is missing.** Open issues, the `[server-thread]` issues for single player's server thread, and the
  player-facing list in [known-issues.md](known-issues.md).

Area notes that explain how one subsystem is compared: [block behaviours](parity/blocks.md),
[meshing](parity/meshing.md), [redstone machines](parity/redstone-diff.md), [whole worlds](parity/world-compare.md),
[world generation](world-generation.md), [world data](world-data.md).

## Rules every port keeps

- Mirror vanilla's `Math` with `horizon_core::jmath` (HotSpot-exact) and its randoms with `horizon_core::random`:
  draws happen in vanilla's order, with Java's signed arithmetic and rejection rules.
- Floor negative coordinates, never truncate. Keep vanilla's iteration orders: ticks, entity lists, feature
  placement and NBT key order where it is observable.
- Return typed errors where Java throws, and keep that difference out of observable behaviour.
