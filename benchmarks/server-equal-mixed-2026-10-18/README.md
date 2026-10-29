# Horizon's server against vanilla 26.3: mixed bots, equal work (2026-10-18)

This reruns `../server-mixed-2026-10-18/` with the bots' load made the same on both servers. In that run the
`explore` bots flew as fast as each server sent them chunks. Horizon kept up and vanilla didn't, so Horizon
sent 2.9 to 5.9 times as many chunks a second, and vanilla's walking bots got a fifth of the chunks Horizon's
got. The comparison of tick times wasn't fair.

## Summary

- **The work was equal** at 1 and 10 bots. At 25 bots it was equal in chunks sent a second (158.6 against 157.6);
  vanilla was still catching up on chunks its bots hadn't had yet, see "Equal work" below. Every bot joined, none
  was dropped, no chunk was sent twice, and no bot ever waited for the chunk it stood in.
- **Tick time (MSPT), same work:**
  - 1 bot: 1.8 ms on Horizon against 11.3 on vanilla;
  - 10 bots: 4.7 against 34.3 ms;
  - 25 bots: 11.7 against 50.5 ms. Vanilla dropped just under 20 TPS (19.8) and its worst 5-second P99 was
    1388 ms, against Horizon's 130;
  - 50 bots: Horizon ran at 23.8 ms (20 TPS). Vanilla's watchdog stopped its server 30 s after the last bot
    joined ("A single server tick took 60.01 seconds").
- **Memory:** Horizon's RSS was 4.1 to 4.9 times smaller: 0.69 against 3.40 GB with one bot, 0.95 against
  3.87 GB with 25, and 1.03 GB with 50. Vanilla's heap is fixed at 4 GB, so its RSS is mostly that heap.
- **CPU** (100 % is one of the machine's 4 threads): 30 against 128 % at 10 bots, 58 against 192 % at 25.
- **What changed since the first run:** the first run's vanilla held 50 bots at 29.7 ms because its walking
  bots got 640 to 1100 chunks each. With the explore bots paced, vanilla serves the walking bots in full
  (5400 to 6100 chunks each, against Horizon's 6100 to 6600), and that is the load it can't carry at 50.

## Results

MSPT is milliseconds per tick from each server's own `/tick query` (50 is the limit for 20 TPS). P95 and P99
are over the 300 measured seconds. "Worst P99" is the highest P99 of a single 5-second query. RSS and CPU
cover the server's process tree. "First-time chunks/s" counts chunks a bot hadn't received before (summed
over the bots). "Waited" is ticks a second that bots stood still waiting for their own chunk. "Pulled back"
counts position packets that moved a bot back.

| Bots | Server | MSPT | P95 | P99 | Worst P99 | TPS | RSS (mean / max) | CPU | Start | Chunks/s | First-time chunks/s | Waited | Pulled back |
|---:|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 1 | Horizon | **1.77** | 4.21 | 13.04 | 47.5 | 20 | **0.69 / 0.69 GB** | 28 % | 19.7 s | 9.94 | 0.01 | 0 | 0 |
| 1 | vanilla | 11.34 | 17.12 | 33.22 | 119.1 | 20 | 3.40 / 3.42 GB | 36 % | 20.4 s | 9.98 | 0.01 | 0 | 0 |
| 10 | Horizon | **4.74** | 10.48 | 19.69 | 120.1 | 20 | **0.86 / 0.88 GB** | 30 % | 19.5 s | 61.35 | 1.79 | 0 | 4159 |
| 10 | vanilla | 34.30 | 53.72 | 89.08 | 375.8 | 20 | 3.71 / 3.75 GB | 128 % | 19.9 s | 61.66 | 1.93 | 0 | 3976 |
| 25 | Horizon | **11.72** | 24.45 | 43.79 | 129.8 | 20 | **0.95 / 1.09 GB** | 58 % | 9.6 s | 158.57 | 5.78 | 0 | 3496 |
| 25 | vanilla | 50.45 | 74.09 | 191.21 | 1388.1 | 19.82 | 3.87 / 4.75 GB | 192 % | 37.8 s | 157.63 | 8.25 | 0 | 3629 |
| 50 | Horizon | **23.82** | 50.98 | 74.64 | 195.9 | 20 | **1.03 / 1.20 GB** | 98 % | 14.9 s | 302.81 | 10.96 | 0 | 7565 |
| 50 | vanilla | stopped by its watchdog | | | | | | | 11.2 s | | | | |

Chunks each bot received over the whole run, by behaviour (mean):

| Bots | Server | walk | explore | idle |
|---:|---|---:|---:|---:|
| 1 | Horizon | 6092 | | |
| 1 | vanilla | 6084 | | |
| 10 | Horizon | 6248 | 1165 | 473 |
| 10 | vanilla | 5729 | 1124 | 473 |
| 25 | Horizon | 6629 | 1267 | 473 |
| 25 | vanilla | 5387 | 1119 | 473 |
| 50 | Horizon | 6497 | 1256 | 473 |

Both servers pulled bots back about as often (`moved too quickly` 20 and 296 times on vanilla at 10 and 25
bots).

## Equal work

`tools/server_bench.py` now prints a work table after the runs. At each bot count it compares every server's
chunks sent a second and first-time chunks a second with the busiest server's. Work counts as equal when
both are within 10 % (or within a chunk a second when there are few). The report keeps the table as `work`.

| Bots | Chunks/s (Horizon, vanilla) | First-time chunks/s (Horizon, vanilla) | Equal |
|---:|---|---|---|
| 1 | 9.94, 9.98 | 0.01, 0.01 | yes |
| 10 | 61.35, 61.66 | 1.79, 1.93 | yes |
| 25 | 158.57, 157.63 | 5.78, 8.25 | no: vanilla sent 2.5 more first-time chunks a second |

Two settings made the load the same:

- **Explore bots paced at 0.8 blocks a second** (`--explore-speed 0.8`), the rate the 2026-10-17 run showed
  both servers keep up with. Every bot then follows the same path on both servers. The path is deterministic
  per bot, and a bot only stops while the chunk it stands in hasn't arrived, which never happened here
  (`Waited` 0). So both servers had the same chunks to send.
- **A 300-second warm-up** before the 300 measured seconds. A first pass with the old 60-second warm-up
  (aborted after 10 bots) sent the same chunks a second on both (63.1 against 61.4). But vanilla was still
  delivering the bots' first views during the measurement (12.9 first-time chunks a second against Horizon's
  4.1), so its window held more work than Horizon's. With 300 seconds both were in steady state at 1 and 10
  bots. At 25, vanilla still sent 2.5 more first-time chunks a second than Horizon: 1.6 % of all it sent, and
  its walking bots ended the run with 19 % fewer chunks. Read the 25-bot tick times with that in mind: the
  extra first-time chunks are work vanilla had still to do because it fell behind.

## Method

`tools/server_bench.py` ran each configuration on a fresh server:

- the same `server.properties` on both: seed 12345, view and simulation distance 10, offline, creative,
  difficulty easy, `sync-chunk-writes=true`, white-list off;
- `gamerule respawn_radius 0`, and otherwise vanilla's default game rules (the locator bar on for both);
- the server waits for `Done`, the bots join one a second, warm up for 300 seconds and are measured for 300,
  with `/tick query` every 5 seconds and RSS and CPU every second.

The bots are `horizon-bots --behaviour mixed`: 60 % walk, 20 % explore and 20 % idle, each choice deterministic
per bot. All are creative and fly 24 blocks above spawn. `walk` flies to random points within 48 blocks at
5.6 blocks a second, `explore` flies straight out at 0.8 blocks a second, and `idle` hovers.

**Machine.** The ThinkPad: Intel Core i7-7500U (2 cores, 4 threads), 32 GB, NixOS. The desktop (24 threads)
was meant to run it, but it didn't wake (`~/fleet/power wake desktop` timed out, as it did for the first run).
No other session ran on the ThinkPad during the runs (`load.txt`, a sample a minute). The servers ran under
`nice -n 19`. The bots ran on the Mac mini (M4 Pro, shared with other work at 70 to 90 % CPU) and reached the
ThinkPad over the tailnet.

**What ran.**

- Horizon: the server build from #916 (`horizon-client --no-default-features`, no sound or controllers) at
  519ec121, packaged with `xtask package-server` and installed with `install.sh --archive`, as Linux owners
  will get it. `horizon-server` ran it as `horizon-engine`. The game files were prepared from the owner's own
  Minecraft. `horizon-bots` and `server_bench.py` came from this PR.
- Vanilla: the pinned 26.3 server jar (the owner's own copy) on OpenJDK 25.0.4.1, `-Xms4g -Xmx4g`.

`server_bench.py` stopped after vanilla's 50-bot server crashed and before it wrote its report.
`mixed-equal.json` holds the run lines it printed. The vanilla 50 entry and its error come from that server's
log, and the `work` table was computed with `server_bench.work`. `load.txt` is the fleet monitor's view of the
machines during the runs.

## Reproduce

    curl -fsSL https://raw.githubusercontent.com/horizon-rust/horizon/main/packaging/server/install.sh | sh
    horizon-prepare --prepare --server-jar /path/to/server.jar      # your own copy
    cargo build --release -p horizon-bots                            # on the machine that runs the bots
    # in a checkout whose .local/minecraft holds the pinned vanilla jars (tools/lab.py prepare):
    HORIZON_BENCH_BIN=~/.local/share/horizon/current \
      python3 tools/server_bench.py --servers horizon,vanilla --bots 1,10,25,50 --behaviour mixed \
        --explore-speed 0.8 --warmup 300 --nice 19 \
        --bots-via 'ssh <bots machine>' --bots-bin /path/to/horizon-bots --connect <this machine's address> \
        --out mixed-equal.json

## Next

- One pass per configuration ran. Vanilla at 50 bots, in particular, needs a second pass, ideally on the
  desktop (24 threads), where neither server is CPU-bound.
- 100 bots, and Paper and Fabric + Lithium (docs/server.md section 9).
- A longer warm-up, or measuring from when the first-time chunk rate settles, so the 25-bot work is equal too.
