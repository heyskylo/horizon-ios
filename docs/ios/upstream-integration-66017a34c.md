# Mobile integration through upstream 66017a34c

This merge advances the previous d2da9d64c snapshot by 711 upstream commits.
It preserves the native launcher, iOS input and AirPlay lifecycle, resource-pack
safety, custom NPC skins and the local ordered-passenger boat authority repair.

Conflict decisions:

- Combine the mobile Extras interface with the new pack browser and cosmetics.
  Match the browser search hit rectangle to the actual text field for touch input.
- Preserve asynchronous shader construction and mobile allocation checks while
  adding upstream outline rendering, storage-image discovery and ambient light.
  Adopt callback-based depth readbacks, retaining the land-frame depth regression.
- Keep controller-only boat prediction and packet-order seat assignment while
  applying upstream yaw interpolation to non-controlled boats.
- Retain cancelable login connections. Run the optional compatibility probe before
  opening a login socket so cancellation cannot leave that socket untracked.
- Combine item custom-data predicates with the native property hooks. Keep NPC
  skin helpers and partial entity visibility behavior.
- Initialize new entity join-order and native-config state. Only mark an attempted
  engine startup when Play actually starts it, not when opening the native launcher.
- Supply the new raw-key argument for synthetic touch input without applying
  physical-key remapping to touch controls.
- Generate the new native mod API in Android's separate workspace and synchronize
  its lockfile with the shared gameplay dependencies.

The same local update adds per-account Keychain storage and awaited Microsoft
session renewal in the native launcher; see docs/microsoft-account.md. Astra's
bounded account/launch review found two cancellation/selection edge cases, which
were corrected and regression-tested before scoped approval.

Validation includes 17 production Swift authentication/launch tests and a native
account-switching UI test on iOS 27, iOS Rust Clippy, 1,325 regular renderer tests,
eight Metal execution tests, and six Complementary/Solas dimension pipeline cases.
The optional Complementary outline fallback still reports an unsupported vaPosition
input; complete pipelines do not establish that optional path's visual correctness.
The iOS 18 test build succeeded, but its simulator test runner could not connect.
Live Microsoft login, a multi-day real account session, physical-device gameplay,
and Android cross-compilation have not been verified in this update.

Final local gates: 5,974 Rust workspace tests passed (116 ignored), 649 Python
tests passed (one skipped), full workspace host Clippy passed, desktop launcher
Clippy and 466 tests passed, and the public-tree audit passed. Android locked
metadata resolution passed; host checking its Android-specific manifest is not a
substitute for cross-compilation and stops on missing macOS-only dependencies.
