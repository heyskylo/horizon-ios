# Horizon's server against vanilla 26.3, Paper and Fabric + Lithium: mixed bots, equal work (2026-10-19)

This is the same load as `../server-equal-mixed-2026-10-18/`, now with two more servers: Paper, and vanilla on
Fabric with Lithium. It also uses a newer Horizon release (v2026.10.19). That release spawns mobs around every
player (#950) and ticks blocks around every player (#931), as vanilla does. The 2026-10-18 build did neither for
players away from spawn, so it had less work to do than vanilla, and its tick times there were lower than
they would have been with equal work. These numbers replace them.

## Summary

- **The work was equal** at every count, on all four servers. Chunks sent a second and first-time chunks a
  second were within 4 % of each other, and the walking bots got about the same chunks each (6050 to 6510).
  Every bot joined and none was dropped. Vanilla's 50-bot server is the exception: its watchdog stopped it.
- **Memory:** Horizon used 0.72 GB with one player and 1.11 GB with 50. The three Java servers used 3.4 to
  4.5 GB (a 4 GB heap each), so Horizon was **3.8 to 4.8 times smaller** at every count.
- **Start:** **8.2 to 8.5 s** to `Done` for Horizon, against 14.7 to 15.5 s for vanilla, 16.6 to 17.6 s for
  Fabric + Lithium and 19.0 to 22.4 s for Paper.
- **CPU** (100 % is one of the machine's 4 threads): from 10 players up, Horizon used the least: 143 % at 50,
  against Paper's 210 % and Fabric's 236 %. With one player Paper used less (32 % against 46 %).
- **Average tick (MSPT), where Horizon loses:**
  - With 1 player, Paper (10.3 ms) and Fabric + Lithium (12.5) were faster than Horizon (13.7). Vanilla was
    slowest (17.3).
  - With 10, Fabric (20.2) and Paper (20.6) were ahead of Horizon (22.9), which tied vanilla (22.9).
  - With 25, Paper (28.3) was ahead of Horizon (30.9), and Horizon was ahead of Fabric (31.4) and vanilla (37.4).
  - With 50, Paper (46.9) and Horizon (47.8) were close. Fabric + Lithium fell to 17.2 TPS (58.3 ms), and
    vanilla's watchdog stopped its server.
- **Slow ticks, where Horizon wins:** Horizon's P99 was the lowest from 10 players up (72.0 ms at 25 and 73.2
  at 50, against Paper's 90.4 and 109.7). Its worst 5-second P99 was the lowest at every count (133 ms at
  50, against 213 for Paper and 888 for Fabric).

So on this 4-thread laptop CPU, Horizon is not yet the fastest on average tick time. Paper is ahead at every
count: Horizon's average tick is a third longer than Paper's with one player and 2 % longer with 50. Fabric +
Lithium is ahead with 1 and 10 players.
Horizon is ahead on memory, start time, CPU from 10 players up, and the steadiness of its ticks (P99). Its
average tick is what perf-server-11 is working on.

## Results

MSPT is milliseconds per tick from each server's own `/tick query` (50 is the limit for 20 TPS). P95 and P99
are means over the 5-second queries of the 300 measured seconds. "Worst P99" is the highest P99 of a single
query. RSS and CPU cover the server's process tree. "First-time chunks/s" counts chunks a bot hadn't
received before (summed over the bots). "Pulled back" counts position packets that moved a bot back.

| Bots | Server | MSPT | P95 | P99 | Worst P99 | TPS | RSS (mean / max) | CPU | Start | Chunks/s | First-time chunks/s | Pulled back |
|---:|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 1 | Horizon | 13.70 | 19.06 | 27.38 | **43.4** | 20 | **0.72 / 0.72 GB** | 46 % | **8.3 s** | 9.94 | 0.01 | 0 |
| 1 | vanilla | 17.31 | 24.68 | 36.43 | 71.0 | 20 | 3.44 / 3.44 GB | 55 % | 14.9 s | 9.99 | 0.00 | 0 |
| 1 | Paper | **10.31** | 13.90 | 22.50 | 81.2 | 20 | 4.24 / 4.25 GB | **32 %** | 22.3 s | 9.81 | 0.02 | 0 |
| 1 | Fabric + Lithium | 12.46 | 17.69 | 32.74 | 70.1 | 20 | 3.43 / 3.43 GB | 46 % | 17.6 s | 9.98 | 0.00 | 0 |
| 10 | Horizon | 22.93 | 39.83 | 53.33 | **89.5** | 20 | **0.92 / 0.97 GB** | **67 %** | **8.2 s** | 61.21 | 1.78 | 3702 |
| 10 | vanilla | 22.94 | 36.92 | 56.48 | 196.6 | 20 | 3.71 / 3.76 GB | 101 % | 14.7 s | 61.63 | 1.84 | 3370 |
| 10 | Paper | 20.64 | 35.94 | 56.29 | 166.9 | 20 | 4.07 / 4.08 GB | 74 % | 20.5 s | 60.76 | 1.85 | 3974 |
| 10 | Fabric + Lithium | **20.18** | 37.04 | 56.08 | 217.9 | 20 | 3.73 / 3.78 GB | 94 % | 16.6 s | 61.65 | 1.84 | 4002 |
| 25 | Horizon | 30.92 | 48.25 | **72.00** | **121.1** | 20 | **1.01 / 1.12 GB** | **99 %** | **8.5 s** | 155.81 | 5.78 | 3008 |
| 25 | vanilla | 37.35 | 48.20 | 82.66 | 579.8 | 20 | 3.94 / 4.16 GB | 191 % | 15.5 s | 158.52 | 5.71 | 3762 |
| 25 | Paper | **28.30** | 61.68 | 90.40 | 124.3 | 20 | 4.16 / 4.25 GB | 124 % | 19.0 s | 156.35 | 5.72 | 4337 |
| 25 | Fabric + Lithium | 31.44 | 52.45 | 93.35 | 409.3 | 20 | 3.88 / 4.19 GB | 174 % | 17.5 s | 158.89 | 5.75 | 4194 |
| 50 | Horizon | 47.83 | 62.42 | **73.21** | **133.0** | 20 | **1.11 / 1.30 GB** | **143 %** | **8.3 s** | 302.68 | 11.02 | 8562 |
| 50 | vanilla | stopped by its watchdog | | | | | | | | | | |
| 50 | Paper | **46.86** | 61.16 | 109.72 | 213.4 | 20 | 4.35 / 4.52 GB | 210 % | 19.4 s | 310.51 | 11.16 | 7344 |
| 50 | Fabric + Lithium | 58.30 | 83.06 | 174.37 | 887.6 | 17.15 | 4.52 / 4.74 GB | 236 % | 17.5 s | 311.52 | 11.28 | 6350 |

Vanilla at 50: all 50 bots joined by 05:22:28 UTC. The server fell further and further behind ("Can't keep up!
… Running 16229ms or 324 ticks behind"), and at 05:28:17, during the warm-up, its watchdog stopped it: "A
single server tick took 60.00 seconds". That is the same as on 2026-10-18.

Chunks each bot received over the whole run, by behaviour (mean):

| Bots | Server | walk | explore | idle |
|---:|---|---:|---:|---:|
| 1 | Horizon | 6280 | | |
| 1 | vanilla | 6210 | | |
| 1 | Paper | 6049 | | |
| 1 | Fabric + Lithium | 6188 | | |
| 10 | Horizon | 6460 | 1195 | 473 |
| 10 | vanilla | 6272 | 1180 | 473 |
| 10 | Paper | 6177 | 1171 | 473 |
| 10 | Fabric + Lithium | 6291 | 1181 | 473 |
| 25 | Horizon | 6506 | 1262 | 473 |
| 25 | vanilla | 6307 | 1236 | 473 |
| 25 | Paper | 6280 | 1229 | 473 |
| 25 | Fabric + Lithium | 6357 | 1239 | 473 |
| 50 | Horizon | 6468 | 1251 | 473 |
| 50 | Paper | 6411 | 1224 | 473 |
| 50 | Fabric + Lithium | 6332 | 1228 | 473 |

## Equal work

`server_bench.py`'s work table compares, at each bot count, every server's chunks sent a second and
first-time chunks a second with the busiest server's. The work is equal when both are within 10 % (or within
a chunk a second when there are few).

| Bots | Chunks/s sent, first-time (Horizon; vanilla; Paper; Fabric + Lithium) | Equal |
|---:|---|---|
| 1 | 9.94, 0.01; 9.99, 0.00; 9.81, 0.02; 9.98, 0.00 | yes |
| 10 | 61.21, 1.78; 61.63, 1.84; 60.76, 1.85; 61.65, 1.84 | yes |
| 25 | 155.81, 5.78; 158.52, 5.71; 156.35, 5.72; 158.89, 5.75 | yes |
| 50 | 302.68, 11.02; (stopped); 310.51, 11.16; 311.52, 11.28 | yes |

Horizon pulled no bot back for moving too quickly (`moved too quickly`: 0 at every count). The Java servers
logged 0 to 46 such lines a run. All four sent position corrections ("Pulled back") at similar rates.

## Method

`tools/server_bench.py` ran each configuration on a fresh server:

- the same `server.properties` on all four: seed 12345, view and simulation distance 10, offline, creative,
  difficulty easy, `sync-chunk-writes=true`, white-list off;
- `gamerule respawn_radius 0`, and otherwise each server's default game rules;
- the server waits for `Done`, the bots join one a second, warm up for 300 seconds and are measured for 300,
  with `/tick query` every 5 seconds and RSS and CPU every second.

The bots are `horizon-bots --behaviour mixed`: 60 % walk, 20 % explore and 20 % idle, each choice deterministic
per bot. All are creative and fly 24 blocks above spawn. `walk` flies to random points within 48 blocks at
5.6 blocks a second, `explore` flies straight out at 0.8 blocks a second (`--explore-speed 0.8`), and `idle`
hovers.

**Machine.** The ThinkPad: Intel Core i7-7500U (2 cores, 4 threads), 32 GB, NixOS. The desktop was meant to
run it but didn't wake (`~/fleet/power wake desktop` timed out twice). No other session ran on the ThinkPad
(`load.txt`, a sample a minute: 0 other sessions throughout). The benchmark ran under `nice -n 19` and
`ionice -c3`. The bots ran on the Mac mini (M4 Pro, shared with other work at 51 to 98 % CPU, 82 % on
average) and reached the ThinkPad over the tailnet.

**What ran.**

- Horizon: release v2026.10.19 (07f2913d), the server archive `horizon-server-linux-x86_64.tar.gz` installed
  with `install.sh`. `horizon-server` ran its `horizon-engine` with game files prepared from the owner's own
  Minecraft.
- Vanilla: the pinned 26.3 server jar (the owner's own copy) on OpenJDK 25.0.4.1, `-Xms4g -Xmx4g`.
- Paper: 26.3 build 205 (`26.3-205-main@a90108f`), from PaperMC's download API, checked against its SHA-256.
  It ran with Paper's defaults except `connection-throttle: -1` in `bukkit.yml`. Every bot connects from one
  address, and the default throttle (4 s) refused all but the first. Paperclip patched the pinned server jar
  from its cache. Same Java and heap.
- Fabric + Lithium: the pinned server jar on Fabric loader 0.19.5 (`tools/benchmark-deps/fabric.lock.json`) with Lithium
  `mc26.3-0.26.2-fabric` (`tools/benchmark-deps/mods.lock.json`, checked against its SHA-512). Lithium needs no Fabric API.
  Same Java and heap.

The Paper and Fabric jars are dev-only downloads into `.local/server-bench/jars` and are never committed or
shipped.

**How it ran.** The first pass ran all four servers at 1 and 10 bots, but its Paper 10-bot run hit the
connection throttle (1 of 10 bots joined), and it was stopped after that. The 1-bot and 10-bot lines for
Horizon, vanilla and Fabric come from that pass, along with Paper's 1-bot line. A second pass with the
throttle off ran all four at 25 and 50 bots, then Paper at 10. All runs were on the same machine on the
same morning (02:57 to 06:02 UTC). `runs.jsonl` holds the 16 run lines used here.
`report-25-50.json` and `report-paper-10.json` are the second pass's reports. The first pass, stopped
early, wrote no report. Its lines come from its output.

## Reproduce

    curl -fsSL https://raw.githubusercontent.com/horizon-rust/horizon/main/packaging/server/install.sh | sh
    horizon-prepare --prepare --server-jar /path/to/server.jar      # your own copy
    cargo build --release -p horizon-bots                            # on the machine that runs the bots
    # in a checkout whose .local/minecraft holds the pinned vanilla jars (tools/lab.py prepare):
    HORIZON_BENCH_BIN=~/.local/share/horizon/current \
      python3 tools/server_bench.py --servers horizon,vanilla,paper,fabric --bots 1,10,25,50 --behaviour mixed \
        --explore-speed 0.8 --warmup 300 --nice 19 \
        --bots-via 'ssh <bots machine>' --bots-bin /path/to/horizon-bots --connect <this machine's address> \
        --out four-way.json

## Next

- One pass per configuration ran, on a 4-thread laptop CPU. A rerun on a bigger machine (the desktop's
  24 threads) would show which server uses more cores best, and give vanilla at 50 a second chance.
- Horizon's average tick: where its 1-player tick goes (13.7 ms against Paper's 10.3). That work is
  perf-server-11's.
- 100 bots.
