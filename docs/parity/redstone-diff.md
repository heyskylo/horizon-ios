# Redstone machines, one tick at a time

`tools/redstone_diff.py` runs whole redstone machines through the real tick loop in vanilla 26.3 and in
Horizon and compares every block (with its properties) after every tick. The component harnesses
(`test_block_update_parity.py` and the others) call one block's code in a synthetic level; this finds what
only shows in the order things happen across blocks: shape updates against neighbour notices, scheduled
ticks against block events, pistons against the blocks they move.

```
python tools/redstone_diff.py --machines 48 --seed 1 --ticks 40     # hand-built circuits, then random soups
python tools/redstone_diff.py --only soup-s1-7 --keep scratch/rd     # one machine, keep the timelines
python tools/redstone_diff.py --only soup-s1-7 --near 6 1 2 3        # narrow a difference down to a box (DX DY DZ R)
```

Needs what `tools/perf_bench.py` needs: your own client jar, a built `horizon-client` and `horizon-ctl`,
prepared resources. No server and no EULA: vanilla is the client, driven by the bench mod.

## How it works

- A machine is a list of `/setblock` commands plus commands that change it at set ticks. The hand-built ones
  (`DEFAULT`) are an observer line, a slime train, a torch ring, a repeater pulse, a comparator clock, a
  piston bud and single pistons; the diagnostics (`FIXED`) are smaller variants of the same. A *soup* is a
  random cell of redstone components from `--seed` (each soup has its own generator, so `--machines` and
  `--only` do not change what a soup is).
- The world is frozen (`tick freeze`) and stepped one tick at a time (`tick step 1`). Vanilla: the bench mod
  (`RedstoneDiff`, with a mixin on `ServerLevel.tick`) snapshots the box before the first stepped tick and
  after each. Horizon: `horizon-ctl command` and the new `simulation blocks X1 Y1 Z1 X2 Y2 Z2`, which reads
  the **server** world, not the client's replica (the replica lags the server by design, and reading it gave
  false piston differences).
- Eight machines share a game session, 24 blocks apart. The first tick where the two differ is printed with
  the blocks that differ.

## What it found

`/setblock` notified neighbours before the shape updates, vanilla notifies after
([#12235](https://github.com/horizon-rust/horizon/pull/12235)). Two of 48 random soups differed; after the
fix 144 soups (seeds 1-3, 40 ticks) are identical.

## Limits

- **Only blocks are compared.** Both sides record the entities near the machine in the timelines (`--keep`),
  but nothing diffs them: vanilla's come from the server level, Horizon's from `entities`, which is this
  window's view. Items, TNT and falling blocks need a server-side `entities` before they can be compared.
- Random soups mostly find ordering at placement time; long-running clocks need `--ticks` of a few hundred.
- Both games run one machine set at a time, so a run of 48 machines takes about 15 minutes.
