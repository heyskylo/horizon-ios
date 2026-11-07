# Mob local sounds and shared capture

H069 concerns `Game::handle_mob_events`, which dispatched both `Sound` and
`LocalSound` through `Audio::play`. The fix uses the existing `play_unshared` for
`LocalSound`. It still follows the normal playback/native-hook path and draws
its audio seed; it temporarily removes and restores only the shared capture.
Producers and their random draws are unchanged.

Pinned Java 26.3 `Breeze.playAmbientSound` and `playWhirlSound` call
`Level.playLocalSound(Entity, SoundEvent, SoundSource, float, float)`. That server
method returns without broadcasting. This does not establish remote-client
Breeze animation or audible rendering parity.

## Frozen live proof

Base: `98af9399cd1c8cd39e153be686808d0cbc71353b`.
Driver: `tools/test_mob_local_sound_parity.py`, SHA-256
`45c5108697201522fedfd5492357bc29be0cbc3d38bff4b7cdb9220f296478c7`.
Fresh Java reference SHA-256:
`a813ce5330d41f41645bbb2ebf4ce3dedbef0fad892868eefb4092774f108196`.

The declared window is 240 actual world ticks, plus one tick for each of two
accepted cow hits (242 total). A stationary live Breeze has a fixed UUID,
Health30, NoAI1 and NoGravity1. Positive actor, player, geometry, loaded-chunk,
exact relative-clock and accepted-hit callback guards run on both sides.
Explicit shared harp sounds and real shared cow-hurt receipts bracket the
window on the same connection. All four shared controls retain their exact
name, category and position. The harp also retains fixed volume/pitch1.

| Complete-lifetime Breeze packet count | Java | Untouched Game | Fixed Game |
| --- | ---: | ---: | ---: |
| `entity.breeze.idle_ground` | 0 | 2 | 0 |
| `entity.breeze.idle_air` | 0 | 0 | 0 |
| `entity.breeze.whirl` | 0 | 6 | 0 |

These are observed counts, not a claim of deterministic random emission timing.
Full wire packets, seeds, pitch, receipt order, command feedback, actor bodies
and absolute clocks are retained locally. The compared contract is the three
local-event counts, four shared controls and 242-tick delta, together with all
positive admissions. Whole bodies, random seed/pitch, absolute startup clocks
and arbitrary packet-family equality are not asserted. Java clock67→309 and
untouched native68→310 are retained verbatim; both relative deltas are242. The
fixed run uses the identical script and immutable fresh Java reference bytes.

The actual Game regression injects a positive `LocalSound` between two shared
`Sound` events. It checks that local playback accounting remains positive and
that the surrounding shared captures retain their parameters and order.
Removing a producer cannot satisfy that test. The unchanged assertion fails on
the old adapter (shared count3 instead of2) and passes with the adapter fix.

Run with prepared local resources and an explicit binary:

```sh
python3 tools/test_mob_local_sound_parity.py --binary /path/to/horizon-client \
  --work-dir .local/local-sound-reference --reference-only
python3 tools/test_mob_local_sound_parity.py --binary /path/to/horizon-client \
  --work-dir .local/local-sound-native --reference-dir .local/local-sound-reference
```

Each destination must be fresh. The native run checks the unchanged script and
reference bytes. Complete captures remain executable through the existing
LocatorBarClient and official sound codec; no observer or packet implementation
was added.
