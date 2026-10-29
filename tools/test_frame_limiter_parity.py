#!/usr/bin/env python3
"""Compare the frame limiter with the pinned client: the real
FramerateLimiter.limitDisplayFPS bytecode, its clock calls rewritten to a
scripted virtual clock, against the Rust limiter with the same script, frame
by frame: sleep requests, spin-waits, overshoot averaging and its reset on a
limit change, interrupted sleeps, oversleep and early wake-ups, the "below
260" condition of Minecraft.runTick, and the limits FramerateLimitTracker
hands it (option values, 60 in menus, 10 when minimised or idle)."""
import json
import random

try:
    from . import lab, oracle_harness as harness
except ImportError:
    import lab, oracle_harness as harness

LIMITS = [10, 30, 60, 60, 120, 144, 165, 240, 250, 259, 260, 1, 2, 7, 1000, 2**31 - 1]


def cases(rng):
    for limit in LIMITS:
        for overshoot in ([0], [50_000], [3_000_000], [30_000_000], [-200_000], [100_000, 2_500_000, 0, 24_999_999, 25_000_000]):
            for spin in (1_000, 50_000, 7_919):
                frames = [[rng.choice([0, 1_000_000, 4_000_000, 16_000_000]), limit] for _ in range(12)]
                yield dict(now=10**12, call=rng.choice([0, 25]), spin=spin, overshoot=overshoot, interrupts=[False],
                           last=10**12, average=0, last_limit=0, frames=frames)
    for n in range(5000):
        frames = []
        limit = rng.choice(LIMITS)
        for _ in range(rng.randrange(1, 40)):
            if rng.random() < 0.15:
                limit = rng.choice(LIMITS)
            work = rng.choice([0, 100_000, 2_000_000, 5_000_000, 16_666_666, 30_000_000, 120_000_000, rng.randrange(0, 50_000_000)])
            frames.append([work, limit])
        start = rng.choice([0, 10**9, 10**15, rng.randrange(0, 10**16)])
        yield dict(now=start, call=rng.choice([1, 30, 400, 20_000]), spin=rng.choice([100, 1_000, 70_000, 600_000]),
                   overshoot=[rng.choice([0, 30_000, 80_000, 1_200_000, 2_100_000, 24_000_000, 26_000_000, -300_000, rng.randrange(-1_000_000, 4_000_000)])
                              for _ in range(rng.randrange(1, 8))],
                   interrupts=[False] + [rng.random() < 0.3 for _ in range(rng.randrange(0, 5))],
                   last=start - rng.choice([0, 1, 5_000_000, 200_000_000]),
                   average=rng.choice([0, 0, 1, 500_000, 2_000_000]), last_limit=rng.choice([0, 60, limit]), frames=frames)


def make_cases(build):
    rng = random.Random(0x46505331)
    path = build / 'cases.jsonl'
    count = 0
    with path.open('w') as out:
        for case in cases(rng):
            out.write(json.dumps(case, separators=(',', ':')) + '\n')
            count += 1
    return path, count


def main():
    scope = ('The actual FramerateLimiter.limitDisplayFPS bytecode from the client jar, loaded through a ClassFile '
             'rewrite that redirects System.nanoTime, LockSupport.parkNanos, Thread.interrupted and Thread.onSpinWait '
             'to a scripted virtual clock (query cost, oversleep/early wake per park, spin cost, interrupt pattern), '
             'with its static state seeded per case and the Minecraft.runTick "limit < 260" gate. Compares every '
             'frame\'s park requests, spin count, clock queries, lastFrameTime, averageOvershootNs and '
             'lastFramerateLimit. The limits themselves (FramerateLimitTracker) are compared by the game loop harness; '
             'real OS sleep accuracy is outside the comparison.')
    harness.differential(
        'rust-frame-limiter-parity', 'VanillaFrameLimiterOracle', 'frame_limiter_probe', make_cases, scope,
        sources=[lab.ROOT / 'crates/horizon-client-input/src/window.rs'])


if __name__ == '__main__':
    main()
