# Item-on-block use cooldown

Measured scope: a connected survival player places oak planks from a five-item main-hand stack carrying `minecraft:use_cooldown={seconds:30.0f,cooldown_group:"probe:group"}`. First placement consumes one; the second use in that group leaves air and count four. A failed stick use in another group starts no entry; planks in that group still place.

Fresh pinned Java 26.3 passed67/67. Pristine `a5d17ed8369b5a8b47fae322bcc9fa602768b28e` passed64/67: the second cell's air/complement/count were the only assigned differences. Actual registered Game/Guest packet regressions failed at absent group and an accepted second placement; the failed-stick control passed. All original assertions and complete raw replies remain retained.

The local v4 program sends the four original UseItemOn payloads in order, with exact sequence ACK1–4. It independently admits player UUID/type/Health20/survival/Pos, typed held item/count/full component/group, loaded stone support and initial air. Four explicit world ticks run at20Hz. Player cooldowns can advance while the world is frozen: the predeclared first-to-second elapsed bound is10seconds against a30second component. Actual Java/native elapsed values and absolute clocks are retained, not normalized. Added causal packet checks require exactly one600tick group start after each successful placement and none after the rejected/failed use.

`ServerPlayerGameMode.useItemOn` performs block interactions before its item cooldown gate. `ItemStack.useOn` applies component effects only after Success/wasItemInteraction, from the pre-use stack copy. The change reuses `Game::apply_use_cooldown` once after final successful placement, consumption and solid-bucket replacement. All earlier block interaction returns, failure returns and the existing gate remain in place. The helper, native hook, group resolution and packet recipient logic are unchanged; native-hook cancellation is not measured here.

Source reach includes every caller of this final BlockItem placement boundary, including fluid/bucket paths that reach it. Runtime is survival main-hand oak planks count5 only. Last-item, offhand, creative, water/bucket behavior, exact expiry, successful cross-item sharing, prediction, other item-on-block success paths and complete item mechanics are unclaimed.

The retained v1/v2 captures failed readiness because repeated command barriers triggered Java spam protection. The local adapter uses the existing Java ping and native connected-client list readiness path; action processing uses exact matching input ACKs. Native v3 stopped before input because integer seconds were retained as Int rather than Java's Float projection. V4 independently readmitted explicit Float input on both engines; no component codec repair is included. Full body/layout/recipe/physics/absolute-clock differences remain diagnostics.

## Reproduce

The fixture uses the existing `paper-pumpkin-parity/framework` (default sibling checkout; override `HORIZON_STATISTIC_FRAMEWORK`). It requires prepared26.3 resources and JDK on PATH. Each run creates isolated server/world and application homes; no real saves are used.

```sh
python3 tools/probes/item_on_block_cooldown/fixture.py vanilla /path/to/26.3/server.jar /new/java-output
python3 tools/probes/item_on_block_cooldown/fixture.py horizon /path/to/horizon-client /new/native-output /path/to/horizon-server
python3 tools/probes/item_on_block_cooldown/verify.py /new/java-output /new/native-output
```

The public fixture differs from frozen local v4 only in its repository-root path depth. Original inputs, invalid attempts, script histories, references, source hashes, runtime and unit executables remain protected under `.local/item-on-block-use-cooldown-start/`. No restart, durability or full protocol claim is made.

Fixed SAMEv4 passes67/67; the same original Game tests pass3/3 and two existing placement/swing preservation controls pass. Added causal packet controls pass4/4. Public portable-fixture Java/old/fixed results independently reproduce67/64/67 with identical strict rows. Complete fixed replies match61/85, with24 full-body/layout/epoch diagnostics retained unmasked; this is not full-body parity. Native first-to-second elapsed was3.900762458seconds old and3.898887042seconds fixed, both within the unchanged10second predeclared bound.
