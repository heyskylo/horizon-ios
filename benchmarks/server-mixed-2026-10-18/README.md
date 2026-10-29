# Horizon's server against vanilla 26.3: mixed bots (2026-10-18)

The first benchmark from docs/server.md section 10 (item 4): the same bots on Horizon's dedicated server, as
`install.sh` installs it today, and on vanilla 26.3's, one server at a time on the same quiet machine.
There were 1, 10, 25 and 50 bots, all with the `mixed` behaviour.

## Summary

- **Both servers held 20 TPS in every run.**
- **Memory:** Horizon's RSS was 3.7 to 5.2 times smaller: 0.65 GB against 3.4 GB with one player, and
  1.0 GB against 4.0 GB with 50. Vanilla runs with the usual fixed heap (`-Xms4g -Xmx4g`), so its RSS
  is mostly that heap.
- **Tick time (MSPT):**
  - with 1, 10 and 25 players, Horizon's average tick was shorter (2.4 against 17.9 ms, 12.4 against 34.5,
    24.0 against 31.6);
  - with 50, it was longer (35.2 against 29.7 ms);
  - Horizon's P99 was lower at every count, and so were its worst 5-second windows (241 ms at most,
    against 424 to 2320 ms for vanilla).
- **The work was not equal, and the difference counts against Horizon.** In `mixed`, 20 % of the bots
  fly straight out at 22 blocks a second, as fast as each server sends them chunks. Horizon kept up:
  - with 10 or more players it sent 2.9 to 5.9 times as many chunks a second, and generated 1.9 to 2.9
    times as much new terrain a second;
  - its walking bots each received 3300 to 4000 chunks, against 640 to 1100 on vanilla once 10 or more
    were online.

  So at 50 players Horizon's tick is 18 % longer while it generates twice the new terrain. The
  equal-rate explore pass of `../server-equal-work-2026-10-17/` is the fair comparison for exploring.
- **Start (fresh world, to `Done`):** 10 to 20 s for Horizon, 14.5 to 28.6 s for vanilla.

## Results

MSPT is milliseconds per tick from each server's own `/tick query` (50 is the limit for 20 TPS). P95 and
P99 are over the whole run. "Worst P99" is the highest P99 of a single 5-second query. RSS and CPU cover
the server's whole process tree (100 % is one thread; the machine has 4). "New chunks/s" counts chunks
no bot had before. "Pulled back" counts position packets that moved a bot back.

| Bots | Server | MSPT | P95 | P99 | Worst P99 | TPS | RSS (mean / max) | CPU | Start | Chunks/s | New chunks/s | Pulled back |
|---:|---|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 1 | Horizon | **2.4** | 5.2 | 18.2 | 45.6 | 20 | **0.65 / 0.66 GB** | 25 % | 10.9 s | 9.9 | 0.3 | 0 |
| 1 | vanilla | 17.9 | 27.6 | 47.3 | 423.8 | 20 | 3.38 / 3.39 GB | 64 % | 14.8 s | 9.9 | 0.3 | 0 |
| 10 | Horizon | **12.4** | 36.0 | 65.9 | 117.1 | 20 | **0.94 / 1.01 GB** | 210 % | 10.2 s | 98.2 | 36.5 | 6267 |
| 10 | vanilla | 34.5 | 63.5 | 138.6 | 884.7 | 20 | 3.59 / 3.66 GB | 217 % | 14.5 s | 24.3 | 19.1 | 5500 |
| 25 | Horizon | **24.0** | 49.7 | 80.6 | 168.8 | 20 | **1.01 / 1.18 GB** | 192 % | 14.7 s | 178.2 | 31.9 | 5532 |
| 25 | vanilla | 31.6 | 70.0 | 222.7 | 2319.6 | 20 | 3.74 / 4.06 GB | 218 % | 28.6 s | 30.4 | 10.8 | 4524 |
| 50 | Horizon | 35.2 | 69.0 | 96.6 | 240.8 | 20 | **1.01 / 1.25 GB** | 218 % | 19.8 s | 273.2 | 34.6 | 8369 |
| 50 | vanilla | **29.7** | 62.3 | 137.4 | 489.3 | 20 | 4.00 / 4.60 GB | 294 % | 20.6 s | 94.4 | 17.8 | 12034 |

Chunks each bot received, by behaviour (mean):

| Bots | Server | walk | explore | idle |
|---:|---|---:|---:|---:|
| 10 | Horizon | 3992 | 7360 | 473 |
| 10 | vanilla | 680 | 2824 | 358 |
| 25 | Horizon | 3928 | 2475 | 473 |
| 25 | vanilla | 640 | 738 | 180 |
| 50 | Horizon | 3300 | 1192 | 460 |
| 50 | vanilla | 1105 | 629 | 220 |

No bot failed to join or was dropped, and no chunk was sent twice (`resent` 0), on either server. Vanilla
also logged "moved too quickly" 12, 324 and 2193 times at 10, 25 and 50 bots.

## Method

`tools/server_bench.py`, unchanged, runs each configuration on a fresh server:

- the same `server.properties` on both: seed 12345, view and simulation distance 10, offline, creative,
  difficulty easy, `sync-chunk-writes=true`, white-list off;
- `gamerule respawn_radius 0`, and otherwise vanilla's default game rules on both. The locator bar is on
  for both: Horizon's server has run it since v2026.10.18.13. The 2026-10-17 runs turned it off on vanilla
  because Horizon had none then;
- the server waits for `Done`, then the bots join one a second, warm up for 60 seconds, and are measured
  for 300 seconds, with `/tick query` every 5 seconds and RSS and CPU every second.

The bots are `horizon-bots --behaviour mixed`: 60 % walk, 20 % explore and 20 % idle, each choice
deterministic per bot. All are creative and fly 24 blocks above their spawn:

- `walk`: random points within 48 blocks at 5.6 blocks a second;
- `explore`: straight out at up to 22 blocks a second, as fast as chunks come;
- `idle`: hovers.

A bot doesn't move while the chunk it stands in hasn't arrived, and it acknowledges chunk batches as
vanilla's client does.

**Machine.** The ThinkPad: Intel Core i7-7500U (2 cores, 4 threads), 32 GB, NixOS (Linux 6.18). No other
session ran on it (`load.txt`: the fleet monitor shows 0 sessions throughout). The busy periods are the
servers themselves, which used 2 to 3 of its 4 threads from 10 bots up, so both servers were CPU-bound
there. The servers ran under `nice -n 19`. The desktop was meant to run this (24 threads) but didn't
wake. The bots ran on the Mac mini (M4 Pro) and reached the ThinkPad over the tailnet, so they took none
of the servers' CPU. The Mac mini was shared with other work (70 to 90 % busy), which only affects the
bots' timing.

**What ran.**

- Horizon: the published v2026.10.18.15 release, `horizon-client` (the engine) at 6c078b28, as
  `install.sh` installs it. Its game files were prepared from the owner's own Minecraft by the release's
  `horizon-prepare`. `horizon-server` (the supervisor, which takes no part in the tick) came from #898
  (4ea2243e), because the release's copy can't find game files prepared without sounds (#893).
  `horizon-bots` came from main.
- Vanilla: the pinned 26.3 server jar (the owner's own copy) on OpenJDK 25.0.4.1, `-Xms4g -Xmx4g`.

On Linux the release's engine needs `libasound.so.2` and `libudev.so.1`. The ThinkPad has them only in
the Nix store, so the run set `LD_LIBRARY_PATH` to them (see #898).

## Reproduce

    curl -fsSL https://raw.githubusercontent.com/horizon-rust/horizon/main/packaging/server/install.sh | sh
    horizon-prepare --prepare --server-jar /path/to/server.jar      # your own copy
    cargo build --release -p horizon-bots                            # on the machine that runs the bots
    # in a checkout whose .local/minecraft holds the pinned vanilla jars (tools/lab.py prepare):
    HORIZON_BENCH_BIN=<a folder with horizon-server and horizon-client> \
      python3 tools/server_bench.py --servers horizon,vanilla --bots 1,10,25,50 --behaviour mixed --nice 19 \
        --bots-via 'ssh <bots machine>' --bots-bin /path/to/horizon-bots --connect <this machine's address> \
        --out mixed.json

`mixed.json` is the report (`horizon-server-benchmark`: machine, versions, settings and every run), and
`load.txt` is the ThinkPad's load before and during the run.

## Next

- An equal-work `mixed` pass (`--explore-speed` at a rate both servers keep up with), then 100 bots.
- The same run on a bigger machine, so neither server is CPU-bound at 25 to 50 bots.
- Paper and Fabric + Lithium (docs/server.md section 9).
- At 50 players Horizon's tick is longer than vanilla's while it generates and sends far more. The
  profile of the 2026-10-17 run (streaming and block entity ticks growing with players) is
  perf-server's to take on.
