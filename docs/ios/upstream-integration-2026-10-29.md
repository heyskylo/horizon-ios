# Upstream integration for the mobile fork

Local integration of the mobile fork at `7f3e7d33f` with upstream first at `c7065027b`, then the additional 29 commits through `7ebf7d26c`. This is a local merge; no branch has been pushed and no pull request has been opened.

## Conflict decisions

- Adopt upstream's `horizon-client-render`, `horizon-client-input` and `horizon-items` split. Move the mobile renderer helpers into the render crate and retain gameplay-level resource import and touch services. Gate desktop clipboard dependencies so Wayland is not compiled for iOS/Android.
- Retain the native SwiftUI launcher, deferred Play initialization, owned resources, Quit-to-launcher shutdown, AirPlay idle message, worker draining and immutable-registry reuse. Preserve the local account-icon removal and Settings cleanup from the last iPad build.
- Combine upstream's shader panic containment, new HDR/TAA behavior and maximum-target-size checks with the mobile deep-stack constructors, asynchronous workers, sparse projection and stable camera history. Fold the new camera spin into model-view. Flat drawing now requires indirect-execution support before allocating indirect buffers, including in the simulator.
- Preserve local asynchronous pack staging and network cancellation while adding upstream's pack-cache, ZIP-entry, metadata-precedence, streaming and rendering improvements.
- Retain NPC texture lifetime/cache recovery and invisible armor-stand equipment behavior while adopting upstream's custom-head profile formats, skin/cape layers, player arm choices, item lighting and scoreboard changes. Rendered equipment keeps its actual item IDs for shader spans.
- Follow upstream's native-only game-mod transition. Remove obsolete Wasm game hooks but preserve mobile built-in zoom, minimap, FPS and keystrokes alongside native frame/input/HUD hooks. The desktop launcher's mod-building workflow is included as source; this integration does not add on-device native compilation to the Swift launcher.
- Retain the fork's intentionally removed desktop release/parity workflows and its existing iOS/Android workflows. Update Android's direct shared-code dependencies and standalone lockfile. iOS Microsoft client IDs and configuration are unchanged.
- Preserve mobile imports when prepared files are absent while retaining upstream's desktop missing-resource diagnostics. Upstream's updated preparer/output-version metadata is included.

## Validation

- iOS audio-only Clippy with warnings denied passed for the final snapshot.
- Repository quick self-check passed: 430 Python tests, 2,692 workspace tests, 276 desktop-launcher tests, two doc tests, five content-root tests, formatting, Clippy and token generation checks.
- Native launcher preview tests passed, including the reduced Settings page.
- Android's locked metadata resolution and host compilation of its shared-game library passed. This is not an Android NDK build or Android device test.
- Source reviews covered renderer, gameplay, networking/input and the root mobile lifecycle. A review finding about official-server initialization being skipped before Play was corrected without starting engine/heartbeat work in the idle launcher.
- The first snapshot was exercised in the full iOS simulator: launcher idle, Play/title, fresh creative world generation and visible rendering, save/title, then Quit/launcher with the game endpoint removed. The final snapshot repeated title/world/save/title/Quit successfully against a clean validation-resource copy. The simulator container path was resolved again after reinstalling before supplying command-line paths.
- No simulator flat-draw workaround was needed after the capability fix. Runs used isolated test homes and prepared validation resources; no real player worlds or accounts were used.
- Public-tree/history checks and added-line sensitive-pattern checks passed.

The 353 affected vanilla parity harnesses were listed rather than run. Physical iPad/AirPlay, multiplayer authentication, Hypixel resource packs and shader-pack performance need a device regression pass before a release claim.
