# Connected player movement as a boat leash holder

The Hub Guest branch of `boat_holder` now reads existing `known_movement`.
After an accepted move, `ClientTickEnd` retains that movement. A later tick end
without another accepted move clears it, while position and previous position
remain unchanged. Holder bounds, eligibility, local fallback, other holders,
elastic pull, friction and packet policy remain unchanged.

The probe uses one survival player and one actual UUID-bound oak boat. The original
single lead is consumed by the attach input. It sends 30 position packets and 40
client tick ends over 43 world steps. Declared empty-mainhand UseItem markers copy
actual current rotation and wait for exact matching ACKs. Selected-slot inventory,
rotation, position and saved/live leash are admitted around each marker. Java's
empty-stack early return is source evidence; Java active use is not directly observed.
The Game marker test observes native model bookkeeping only.

With Python 3.12+, a sibling `paper-pumpkin-parity/framework`, prepared resources,
and a JDK for Java, run from the repository root:

```sh
python3.12 tools/probes/boat_holder_movement/fixture.py vanilla /path/to/server.jar /new/java-output
python3.12 tools/probes/boat_holder_movement/fixture.py horizon /path/to/horizon-client /new/native-output /path/to/horizon-server
```

The probe retains raw packets, bodies, clocks, full positions and strict checks.
All 1844 checks must pass. The five original target rows include four-decimal
projections; these do not claim an exact trajectory. Last-item interaction beyond
the admitted lead, riders, other-holder runtime behavior, rendering and restart
remain outside this proof. No native networking or shared solver change is made.
