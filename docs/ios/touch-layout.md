# Mobile touch layout

In Horizon's app Settings, choose **Touch controls → Edit button layout**. This is independent of Minecraft's in-game Options screen and works before the engine is started.

The editor opens in landscape with a black canvas. Drag the movement, sneak, jump, perspective, chat, pause and inventory buttons. The canvas fills the display with the game’s pixel dimensions and GUI Scale. Drag the floating toolbar by its handle to reach any position underneath it. The toolbar's button selector brings an obscured control to the front for editing. The hotbar slots are a fixed reference belonging to the game HUD.

**Save** persists the draft on this device. **Cancel** discards it. **Reset** restores the built-in button positions in the draft; choose Save to keep the reset. Closing the editor returns the phone to portrait and the tablet to its previous orientation.

Positions use normalized centres and are clamped to the actual button size after a resize or GUI-scale change. Rendering and input read the same rectangles. When controls overlap, the last painted control owns that touch, including a control placed over the hotbar.

Controls are visible by default. **Show touch controls** changes drawing only: touch regions continue to work when hidden. It can be switched off manually before a system screenshot.

## Capture limitation

Controls currently remain visible both on-device and in system recordings. There is no automatic recording-time hiding: making them disappear from the player's screen is not a substitute for excluding them only from the video.

Apple documents a secondary-window technique for recordings made by an app's own ReplayKit recorder in [Streaming an AR experience](https://developer.apple.com/documentation/arkit/streaming-an-ar-experience). That is not a guarantee for Control Center recording, system screenshots or every OS version. Capture-only exclusion and an in-app recorder are not implemented in this change. Screenshot notifications arrive after capture; they cannot remove controls from an already-taken system screenshot.

## Implementation and compatibility

This is original Horizon code; no Amethyst or Pojav source was read or incorporated. Amethyst's GPLv3 license was checked before choosing an original implementation.

The Swift configuration uses `touchControls.v1` in this app's UserDefaults, separately from account data and world settings. Invalid data falls back to the built-in layout. Ten button centres cross the versioned C ABI as twenty floats, with NaN meaning default placement. Flag bit zero hides painting. No pointers are retained across the call. The engine resolves the bundled export once and falls back to the original controls if running with an older launcher framework. Desktop layouts are unchanged.

Validation covers production Rust layout and finger dispatch tests, isolated Swift persistence and ABI tests, iOS 18 phone and iPadOS 27 simulator editor interactions, an arm64 device framework build, and the iOS engine Clippy check. Simulator UI and compilation checks do not establish physical-device gameplay or recording behavior.
