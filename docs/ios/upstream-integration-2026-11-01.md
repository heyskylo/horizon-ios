# Mobile upstream integration: 1 November 2026

Upstream snapshot: `horizon-rust/horizon` at `dea6dea98`.
Previous shared upstream revision: `ed5565713`.
Mobile starting revision: `3e140d2d2`.

## Merge decisions

- Keep the native launcher, resource import, Microsoft authentication, iOS orientation handoff,
  keyboard/clipboard fixes, touch handoff and AirPlay support. The desktop launcher remains upstream's.
- Adopt upstream's generalized keyboard navigation and per-player splitscreen while preserving mobile
  input and AirPlay. UIKit controllers have stable per-device identities; native AirPlay and desktop
  compositors cannot simultaneously own the same renderer.
- Keep real engine teardown when returning to the launcher. Drain world/resource/shader workers and
  guest clients before dropping their owners. Use owned resource references instead of leaked ones.
- Adopt upstream's expanded shader interfaces, including compute/storage resources and geometry
  programs, while preserving asynchronous compilation, view-bobbing transforms, texture bounds and
  per-block material data. Sorting and compaction move geometry and its material data together.
- Keep fork-specific workflow restrictions. Upstream's private-runner jobs do not become requirements
  for the mobile fork. Android's separate workspace includes the new dependencies and vendor patches.

## Confirmed repairs during integration

- Preserve depth-only geometry programs; no color output is valid for those programs.
- Discover compute-only shadow composites before determining their resource requirements.
- Combine storage-image access across stages and reject conflicting image declarations.
- Bound shader archive reads, expanded textures and sprite-cache entries before allocation.
- Keep large renderer state on the heap; retain the existing mobile stack-size guard.
- Use the supported Mach memory query on iOS. Suspend memory-limit enforcement and park its watcher
  while the engine is stopped in the launcher, preserving the configured cap for the next Play.
- Keep new text editors eligible for the system keyboard, and avoid AirPlay/menu action-ID collisions.
- Apply TNT drag as Java does: widen the float returned by `Entity.getAirDrag()` to double. The pinned
  Java 26.3 `PrimedTnt.tick` bytecode and actual gravity/drag method values supplied the reference.
- Correct upstream armor-stand and TNT test fixtures to reflect Java's movement order and actual
  fluid intersection/bubble-column direction; do not change working physics to satisfy those fixtures.

## Validation and limits

Host and iOS client compilation, iOS Clippy, production input probes, focused world-loading and shutdown
checks, client library/binary tests, and Metal shader execution checks were exercised. Isolated LAN
checks cover player lists, chests, chat, damage feedback, eating on blocks and cauldron prediction.
Private Complementary Reimagined 5.9.3 and Solas 3.8 inputs build complete Metal pipelines in all three
dimensions. A focused depth readback verifies the depth-only geometry repair.

These checks are not an exhaustive Java-parity certification, an Android runtime pass, a physical
old-iPad test, or a full-world shader visual comparison. The reported old-iPad startup crash in mobile
issue #14 still needs the reporter's device/build details and diagnostic report; no fix is attributed
to that report without evidence. Private game files, shader archives and diagnostic outputs are not
part of this integration.

### Recorded checks

- Python: 600 tests, one skipped, no failures (Rust-backed dispatch probes enabled).
- Game binary: 589 passed, four ignored; the focused TNT trajectory tests also pass.
- Client library: 1,265 passed, 39 ignored, with one erroneous upstream armor-stand expectation
  subsequently corrected from Java bytecode and its three focused tests rerun successfully.
- Renderer: 959 regular tests passed; private-input/GPU tests are run separately. Two crash-symbolization
  tests require debug line tables and were excluded from the debug-info-free renderer test run.
- Shader crate: 133 tests passed; external-pack validation ran separately.
- Actual Metal: seven shader execution regressions and the depth-only geometry regression passed.
- Private real-pack inputs: six complete pipeline/target-allocation results, two packs by three dimensions.
- Isolated host/guest LAN: nine scenarios passed without skips, including item-use cancellation,
  bubble-column motion and team-controlled player passthrough.
- Current host and iOS Clippy checks pass with warnings denied; optimized host build succeeds.
- Android locked dependency resolution passes; no Android target/runtime verification is claimed.
- Formatting and public-tree checks pass. The private diagnostic working report remains untracked.
