# Guest respawn: world information and personal landing

Pinned Java26.3 `PlayerList.respawn` sends `ServerLevel.getRespawnData` in the
first default-point packet; `sendLevelInfo` sends that level data again. Both
are world information, independent of the guest's personal landing destination.
Current `Game::guest_respawn` instead puts the resolved personal destination in
`TickCtx.spawn`, which both existing Hub resend sites consume.

## Frozen actual Game/Java fixture

Base: `764df32826a7f9997c4661f7a8df564f750a8986`. The existing connected
`LocatorBarClient`, real dedicated Game and `net_capture.py --full` proxy are
used. No production observer, new protocol implementation or fake guest event.

```
python3 tools/test_respawn_world_point_parity.py --side java --label java --binary /path/to/old-game
python3 tools/test_respawn_world_point_parity.py --side native --label old --binary /path/to/old-game --reference .local/respawn-world-point-parity/java-facts.json
python3 tools/test_respawn_world_point_parity.py --side native --label fixed --binary /path/to/fixed-game --reference .local/respawn-world-point-parity/java-facts.json
```

World W=[0,100,0], forced personal P=[5,101,5]. A fresh flat world gets a stone
floor at99, a support stone under P, loaded four chunks, no mob spawning and
respawn radius0. The actual survival Alice joins AFTER W is saved; initial99
is W. Set P by the real command, capture the actual player body, disconnect,
flush/copy the actual saved personal file, then reconnect the same UUID. The
saved forced configuration, actual typed UUID/type/Health/mode/Pos, water-free
air/support/load controls, Health0 after kill, Health20 after ClientCommand12
with action0, landing blockP and unchanged saved W are independently admitted.

Two measured world ticks after EACH join settle packet delivery; one further
world tick delivers the respawn request. Exactly five explicit setup/delivery
ticks total, no open-ended retry. Relative clock deltas from configured are
0/2/2/3 for configured/before/dead/after. Raw absolute clocks stay retained:
Java5/7/7/8, final old native6/8/8/9 and fixed native8/10/10/11;
they are not claimed equal. The earlier valid old capture7/9/9/10 is also retained.

Before native pairing, the 54 complete fact labels and program were declared.
Driver SHA256: `6dec605b016c21968e79d5b9d55a0829dd41ac0a49079e307152d915edfbbd52`.
Fresh Java reference SHA256: `d5925b3259a2e0c7d8a65e6a0bd4ec45061629b088cd0198dadd6e1d3067b15b`.
The final CLI also runs both sides without required flags for the normal parity
runner. That routine CLI correction leaves the declared program unchanged; a new
fresh Java reference is byte-identical and both native builds are rerun against it.
All full wire bytes in both directions, packet counts/order, raw player bodies,
logs and saved files remain under the owner's `.local`. No Mojang files published.

Old current: **53/54**, solely `network/1/after-points`, which strictly compares
BOTH complete99 packets (dimension, signed block coordinates, yaw/pitch/float
bits, complete payload and order after Respawn84). Java sends W twice; native
sends P twice. Both initial connections send W once; exactly one respawn and
one real action0 are observed. All other53 facts pass, including death/revival
and saved-personal admission.

## Explicit diagnostic boundaries

No full raw packet-family/body/absolute-clock equality claim. Exact post-respawn
Pos is retained separately: Java[5.5,101.1,5.5], old native[5.5,101.0,5.5]. The
strict physical guard is the landing blockP, declared before native capture;
the physical resolver is out of scope and must remain unchanged. Other complete
packets and their interleavings remain raw diagnostics, not silently normalized.

The original no-step draft was rejected: kill feedback alone did not establish
death. A two-join-step draft reached Health0 but its request had not delivered.
A subsequent draft had the wrong expected exact Java landing Y. All drafts,
partial traces and raw outputs remain executable/retained, with no defect counts
from failed prerequisites. The final fresh Java program independently passes.

## Narrow change and regression

Only separate the world-information context from the already resolved personal
destination. Keep both Hub resend sites and their order, the physical resolver,
world-spawn changes/angles, H258 sea-level handling and other protocol fields.
Same-level proof does not establish every dimension or unavailable-world case.

The resource-backed regression fails old at the two P-versus-W packet fields,
after all live-state/order assertions, and passes fixed. It invokes actual
`Game::guest_respawn` on a real Game/Hub with distinct W/P. Its channel decodes actual emitted packets and checks
84/99/99, both complete point fields W, unchanged personal landing, same UUID,
Health20 and alive state. It tests the caller context, not just Hub with a manually
corrected TickCtx.

## Fixed paired result

Final driver/program and fresh Java reference bytes are unchanged across the
paired reruns: **53/54 old → 54/54 fixed**. The only changed fact is the two
complete post-respawn99 packets, now W/W. The target packet subsequence remains84/99/99.
The raw native post-respawn position is byte-for-byte the same typed value in
both captures: [5.5,101.0,5.5], and the live landing-blockP guard remains strict.
No physical resolver or other packet-field repair is inferred.

All emitted capture records (both directions, every payload and order) number
468 for final Java, 546 old native and 548 fixed native. These totals are
diagnostic traffic counts, not a packet-family comparison. The complete sequence
and per-field whole-player-body expected/actual diagnostic files are retained.
Other differing raw player-body field names: `Brain`, `HurtTime`,
`LastDeathLocation`, `Motion`, `OnGround`, `Pos`, `XpSeed`, `attributes`,
`current_impulse_context_reset_grace_time`, `spawn_extra_particles_on_fall`,
`warden_spawn_tracker`.
No repair or parity claim for those unrelated saved/default fields.

## Readiness and later source checkpoint

Final `pr_ready.py --quick --base 764df32826a7f9997c4661f7a8df564f750a8986`:
1032 Rust tests pass; rustfmt, clippy with warnings denied, doc tests and public
source checks pass. The ignored resource-backed actual Game regression was
explicitly run: old fails at P/P versus W/W, fixed passes. The earlier incorrectly
qualified exact-filter invocation ran zero tests and remains a rejected test
invocation, not passing regression evidence.

Python runs696 tests with four existing macOS packaging failures and one skip:
the installer signature fixture offers x86_64 assets on this ARM64 host, and
three optional-archive combinations hit Bash3.2 unset arrays. A narrow installer
rerun confirms the prerequisite. Required checks therefore exit nonzero; no
installer/release repair or fully green check claim. Broad parity is skipped by
`--quick`; the explicit actual Game/Java pair ran separately. The new driver is
also discovered by `tools/parity.py respawn_world_point --list`.

Later actual main pull: `e5875b0b192e64c2de121e30940d39bdd4c7728b`, changing only
launcher native-mod diagnostics. No relevant gameplay/protocol/harness delta,
so proof remains qualified to frozen764df3282 without a combined-latest claim.
Raw stage logs and available file timestamps are retained; unobserved event stage
times remain unknown. The final evidence-only documentation append is followed
by whitespace/public-tree checks, without repeating settled gameplay checks.

After the required run, a test-only guard prevents an output path from overwriting
the supplied reference. The program/54 labels are unchanged; fresh Java, old and
fixed are all rerun with the newly declared driver hash above, giving54/54,53/54
and54/54. Fresh reference bytes are identical. No production or Rust-test change.

Latest raw capture diagnostics (configured/before/dead/after absolute clocks):

| Capture | Actual raw clocks | All emitted capture records |
|---|---|---|
| java-publish | `[7, 9, 9, 10]` | 466 |
| old-publish | `[7, 9, 9, 10]` | 547 |
| fixed-publish | `[5, 7, 7, 8]` | 544 |

Earlier full captures and every declared script/reference remain retained.
