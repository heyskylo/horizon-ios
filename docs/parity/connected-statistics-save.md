# Connected-player statistics at save-all

The dedicated server must write a connected player's live statistics before
`save-all` or `save-all flush` reports success. Saving only the player body does
not satisfy this boundary. The repair adds a statistics-only call after the
existing body enqueue loop; it reuses the existing UUID mapping and JSON writer.
No dirty-state predicate, body/advancement rewrite or file/error policy changes.

The actual input fixture uses one fixed UUID-bound survival player and two
independently admitted living pigs. Real sword attacks take Health 10 to 3 and
advance the live used-sword criterion to 1, then 2. Following same-connection
use-item markers receive exact sequence ACKs 0 and 2. This establishes action
processing independently of expected damage/statistics. Initial login/teleport,
chunk/load, player body, equipment and actor admissions remain separate.

Each scene reads the actual stats JSON while the player stays connected, after
normal/flush/repeated save completion. Separate disconnect positives establish
the same path; they do not substitute for connected reads. There are eight exact
stepped world ticks, with every absolute clock and phase timestamp retained.

The frozen current-base capture on `d5504cd328` passes Java 87/87 and native 74/87.
The same frozen program/reference passes fixed 87/87, closing all 13 target
rows; the portable public version independently reproduces 87/74/87.
All 13 old differences are missing/stale connected-file reads. The two original
actual Game regressions fail old after successful save feedback, including with
network dirty entries consumed; both unchanged tests now pass. Added source
controls preserve nonplaced/released/no-play and dedicated/nonclient/host gates.
Full raw body/field diagnostics are 28/35 exact;
the other seven are retained, outside the statistics claim.

Run against prepared 26.3 resources and an existing compatible
[framework checkout](https://github.com/TheMrZZ/paper-pumpkin-parity):

```sh
export HORIZON_STATISTIC_FRAMEWORK=/path/to/paper-pumpkin-parity
python3 tools/test_connected_statistics_save_parity.py vanilla /path/to/server.jar NEW_JAVA_OUT
python3 tools/test_connected_statistics_save_parity.py horizon /path/to/horizon-client NEW_NATIVE_OUT /path/to/horizon-server
```

The public fixture differs from the frozen local fixture only in portable path
selection and the adapter filename; its Java/pristine-native records were
independently recaptured and match the original 87 records exactly. Every output
folder must be new. Keep complete files/receipts; never interpret a missing file
as zero. The wrapper reports all failures and exits nonzero if any remain.

Scope is connected dedicated-player statistics persistence. Existing autosave
and pending metadata callers share this source boundary; their timing, restart,
durability, other player data and broad lifecycle parity are unproved. Earlier
invalid duplicate-actor captures and the resolved disconnect-saved statistic
slice remain separate evidence.
