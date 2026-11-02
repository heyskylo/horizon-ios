# Mobile integration through upstream 3db53df3b

This sync follows upstream `3a264943c`, preserving the mobile launcher, Microsoft
sign-in, touch/keyboard behavior, AirPlay, asynchronous shader loading and complete
engine shutdown.

## Server-address keyboard regression (mobile issue #17)

The prior title-screen event path removed printable text before the title field
received it, while Backspace still traveled through physical-key handling.
Upstream `85aba0256` corrects the text routing. The mobile integration retains
that fix and preserves character-cancellation hooks and editing-modifier guards.

`tests/test_mobile_multiplayer_keyboard.py` exercises production dispatch and
UIKit conversion. The old revision fails address insertion; the repaired path
passes typing/backspace, Unicode, hook cancellation, modifier and canceled-key
character tests. The ordinary title-field tests additionally cover Direct
Connection and Add Server cursor editing. This is automated input-path evidence,
not a claim of testing the reporter's physical device.

## Integration decisions

- Adopt server-thread-only simulation; remove obsolete in-frame paths rather
  than retain a second implementation. Drain starting/running integrated servers,
  loaders and the new prepared-data writer before returning to the launcher.
- Preserve active prepared-file snapshots while invalidating cached lookups after
  resource replacement. Validate cache indices, offsets, names and lengths before
  allocation, and stream bounded cache writes. Oversized cache members retain
  ordinary-file fallback. Desktop caches remain optional after mobile import.
- Keep resource reloads gated during rest loading and integrated world handoff.
- Preserve per-item and terrain shader metadata, independent AO, bounded texture
  loading and deep-stack pipeline creation alongside upstream renderer changes.
- Consolidate setup/begin programs into upstream's ordered representation; reject
  unsafe game-resource URIs before generated-resource shortcuts or filesystem reads.
- Record original touches as user input and replay them as agent input. Do not
  record their translated pointer events twice; clear unfinished touch state at
  replay teardown.
- Retain mobile publication restrictions, including explicit rejection of the new
  `horizon-data.bin` prepared-data bundle in source and IPA publication checks.

Focused checks cover renderer/shader tests, actual Metal execution and all six
Complementary/Solas dimension pipeline combinations. Full readiness results are
reported in the PR. Quick readiness does not run every affected Java-parity
harness. Physical older-device behavior and full-world shader visual parity
remain separate validation scopes. No private game data, archives or diagnostic
reports are included.
