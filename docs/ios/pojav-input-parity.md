# Pojav iOS input comparison

## Source and provenance

The user explicitly requested inspection of Pojav's actual input implementation and a newly written Horizon adapter with matching behavior. This is an explicit exception to this checkout's CONTRIBUTING.md prohibition on reading GPL source; it must be disclosed in any future review or PR. This work is source-informed, not a clean-room implementation. No Pojav or Amethyst implementation files or extracted method bodies are bundled in the repository or app.

References, both GPL-3.0:

- [PojavLauncher iOS, 1eeab5c1ee6f80991e09cc411054419374852cff](https://github.com/PojavLauncherTeam/PojavLauncher_iOS/tree/1eeab5c1ee6f80991e09cc411054419374852cff)
- [Amethyst iOS, f98c4133c72d39e06ceac42b57cc4477c52105fc](https://github.com/AngelAuraMC/Amethyst-iOS/tree/f98c4133c72d39e06ceac42b57cc4477c52105fc)

The inspected input paths are `Natives/customcontrols/ControlJoystick.m`, `Natives/SurfaceViewController.m`, `Natives/input/ControllerInput.m`, the control-button types, input bridge and key-code declarations. LICENSE was inspected at each pin; the GLFW key-code header has its own zlib notice. Commit IDs and SHA-256 digests of every source file compiled by the development oracle are enforced by the harness.

## Matching behavior

The game calls the same Rust `Joystick`, `Pointer` and `MotionClock` routines that the probe executes:

- Joystick thumb width is one quarter of its smaller dimension; travel is three eighths. The grabbed-mode dead zone is 35% of travel. Moving back inside that zone retains the prior movement until release.
- Directions use float32 atan2 and eight 45-degree sectors. Every sector change sends W, A, S, D states in that order. The source's exact-axis quirk is retained: an exact horizontal or vertical normalized axis produces the neutral state. Tests cover both sides of sector/dead-zone boundaries and exact axes explicitly.
- Menu cursor motion accumulates relative touch deltas, applies mouse speed, clamps to the surface and scales only at the callback boundary. One surface finger owns movement; handoff establishes a new origin without jumping. Left/right click fingers remain independent.
- An ungrabbed joystick supplies cursor velocity. Each rendered frame applies magnitude-squared acceleration and elapsed-time scaling relative to 60 Hz, followed by the same pointer clamp/transform.
- Default buttons capture their initial press until release, including outside the button. A second contact on the same ordinary button is ignored, as for UIKit's default single-touch UIControl. Sneak emits a held Shift press/release on iOS; in-game toggle-crouch behavior remains the game's responsibility. The Android legacy toggle/slide behavior is unchanged.
- Only one world surface finger drives the camera. A second finger can take over after the first ends; it cannot add a second camera delta while the primary remains active.
- Releasing the joystick clears movement. Horizon additionally clears holds and pending clicks on cancellation, screen changes and input takeover to avoid stuck inputs.

Movement emits physical WASD inputs through the player’s in-game bindings. Virtual keys can drive rebound mouse actions, and mouse buttons can drive rebound key actions; every mapping bound to an input receives it. This does not change Java-compatible world physics or network packets. Pojav's GLFW/JVM transport and Horizon's Rust action dispatch are different backends.

## Differential harness

This follows the engine's Java-versus-Rust parity method: feed identical deterministic inputs to an independent reference implementation and the production Rust routines, then compare outputs at a defined boundary. Here the independent references are the actual pinned Objective-C methods, compiled unchanged inside a private development cache with Foundation geometry/UI stand-ins. This is not a comparison to another handwritten reference algorithm.

Run from a macOS developer environment with Clang, Rust and cached serde_json 1.0.151:

```sh
python3 tools/pojav_parity/run.py \
  --reference-dir /absolute/path/to/private-reference-cache \
  --report /absolute/path/outside-repo/input-parity.json
```

The cache must contain the pinned Pojav `LICENSE`, joystick and surface files at their repository paths, plus `Natives/input/ControllerInput.m`. Under `amethyst/`, provide the corresponding pinned Amethyst files, with its ControllerInput saved as `Natives/ControllerInput.m`. The harness refuses changed hashes. It neither downloads nor vendors the sources. Generated GPL oracle source/binaries stay in the explicitly supplied cache. Rust probe builds use a fresh temporary directory. This opt-in tool deliberately lives outside `tools/test_*_parity.py` automatic discovery: regular CI must not fetch or read GPL reference files.

The verified run compared **1,262 traces / 15,796 samples**, with **zero mismatches** between Pojav, Amethyst and Horizon:

| Group | Traces | Coverage |
| --- | ---: | --- |
| Joystick | 716 | Square/non-square bounds, axes, sector/dead-zone edges, out-of-bounds motion, retained center state, release, shift modifier |
| Pointer | 36 | Grabbed/ungrabbed, direct/virtual, speed, display scale, resolution scale, absolute/relative motion, bounds |
| Joystick clock | 18 | 30/60/120 Hz with variable cadence, zero/nonzero velocity, acceleration, clamping, speed |
| Mouse buttons | 4 | Left/right down/up ordering and simultaneous holds |
| World buttons | 64 | Production Touches → Game::touch_act, all current key/mouse buttons, repeated holds, outside release, simultaneous presses |
| Menu buttons | 64 | Production Cursor::event_controls, matching key/button events, captured fingers |
| World joystick | 100 | Production Touches → Game::touch_act over random multi-direction gestures |
| World camera | 120 | Actual Pojav touch methods versus Touches → Game::touch_act, one primary finger, handoff, cancellation, display scales 1–4 |
| Gestures | 80 | Actual tap/long-press/hotbar-swap/scroll/keyboard handlers versus production Recognized input translation, including requested release delays and enabled/disabled mouse gestures |
| Menu surface | 60 | Actual Pojav touch methods versus production Cursor, direct and relative modes, two-finger ownership, handoff, lift and out-of-bounds motion |

Comparisons check event count/order, key/button state and pointer coordinates; numeric tolerance is 1e-12 relative / 1e-9 absolute. Additional production dispatcher tests cover quick click frame separation, screen transitions, cancellation, repeated ownership changes, finger handoff and simultaneous joystick/click use.

## Limits and deliberate adaptations

The differential harness establishes parity at the native-control-to-callback boundary for the supported default modes. It does not run UIKit hit testing, the JVM, Minecraft, a rendered scene or a physical device. Separate native simulator interaction tests exercise the production UIKit recognizers and C bridge. The source oracle uses one valid contact per button and disables forward lock. Importing Pojav JSON layouts, assigning arbitrary four-key control macros, forward lock, custom swipe/toggle controls, and configurable middle-click/scroll buttons are outside this change. Minecraft’s in-game rebinding is supported and separately regression-tested. No general claim of full Pojav feature parity is made.

Horizon retains safety behavior for cancellation, shared held-button ownership and deferred menu clicks because its Rust menus consume pointer state on rendered frames. Those are documented platform adaptations rather than byte-for-byte GLFW event transport. Touch sensitivity is the existing Horizon setting; the current touch pointer uses speed 1.0. Non-default pointer speeds are tested at the reusable transform boundary, not exposed as a new app preference.

The prior build 96 predates this source-backed correction. Source, differential and compile passes do not establish physical-device behavior; this change needs a fresh package and device check before a release claim.

## Native integration and regression validation

The shipping Swift framework attaches UIKit recognizers to the game’s own view. The game frame consumes the capacity-checked `horizon_touch_gestures_v2` queue: eleven doubles per input, the six of version one followed by the click context the gesture was made against, its hardware modifiers and its touch identity. The game names the current context through `horizon_touch_set_context_v1`. Version one stays exported for older bundled readers. Both menu modes share the same control dispatcher; only pointer positioning differs. A tap releases after the source’s requested 33 ms, with time already spent in the native queue subtracted. Menu edges additionally wait for rendered frames so clicks use the currently drawn target. A long press uses UIKit’s 400 ms recognition threshold. Two-finger scrolling keeps both velocity axes; the keyboard gesture toggles the existing system keyboard. Hotbar taps, double taps and holds emit the number, F and Q inputs through normal game bindings.

Queue records carry the current screen context. A changed screen, cancellation or overflow clears obsolete input and releases delivered holds. Menu hardware takeover discards stale queued gestures without disabling recognition of the next touch. World keyboard movement can coexist with a native held mouse gesture. Source-informed cancellation is adapted to avoid clearing an independent held Shift when a two-finger pan cancels its surface touch. Physical Shift from a virtual button also participates in inventory shift-click queries; a crouch toggle does not stand in for the physical modifier.

The native recognizer bridge uses the game’s actual GUI scale and the view’s display scale, rather than stretching rounded GUI dimensions across the view. Hotbar hit testing uses the reference bridge’s physical integer centering and 20-GUI-pixel height. The existing 20/26-float position reader remains compatible. Older frameworks without the new gesture export retain the direct-touch fallback; the source-backed gesture path requires a matching current framework.

Validation includes production Rust touch state, production Game button/gesture dispatchers, the real KeyBinds implementation under rebound options, modifier state, timed releases, scene/screen cancellation, and native queue capacity/context handling. Native XCTest interaction on iPadOS 27 and an iOS 18 phone simulator exercises real taps, long presses, hotbar double taps and hotbar holds through the production recognizers and C ABI. The native callback handlers for two-finger scrolling and keyboard input are source-compared; the C queue’s keyboard callback is also exercised by a recognized-input fixture.

The final key/mouse consumers in the Rust differential probes are isolated endpoints. These checks do not run Minecraft’s world simulation, the JVM, a rendered Minecraft menu, or physical-device gameplay. This is control/input parity evidence rather than a claim of whole-game or full-launcher feature parity. A new IPA and physical-device check are separate release validation; build 96 predates these corrections.

The Release shipping `HorizonLauncherKit` target also builds successfully. A separate simulator C executable loads that framework using `dlopen`, resolves the gesture export, and exercises its capacity/null/empty-queue ABI. This checks the shipped framework export independently of the Swift preview target. The small linkage checker is `tools/pojav_parity/check_native_bridge.c`. It now covers `horizon_touch_gestures_v2` (the export the game calls), `horizon_touch_gestures_v1` and `horizon_touch_set_context_v1`. The earlier recorded simulator runs on iPadOS 27 and iOS 18 were of the version-one check only. The extended checker passes against a Release simulator build of the framework on iPadOS 27.2; it has not been run on iOS 18.

Final verification: 1,262 differential traces / 15,796 samples with zero mismatches; 26 Rust touch tests; 873 Python tests with 7 skips; 10 Swift touch-control logic tests; native gesture interaction tests on both simulators; arm64 iOS client Clippy with warnings denied. Python’s parser/state cache was redirected to internal scratch storage after the external workspace refused a cache write; no test algorithm or expected result was changed for that workaround.
