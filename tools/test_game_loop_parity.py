#!/usr/bin/env python3
"""Compare the game loop and input handling with the pinned client: the
DeltaTracker.Timer, ServerTickRateManager (freeze, step, sprint), Window
scale computation, FramerateLimitTracker decisions, MouseHandler (relative
and absolute motion, sensitivity curve, cinematic smoothing, inversion,
scroll accumulation and discrete scrolling), Entity.turn and ToggleKeyMapping,
all driven with identical synthetic input and time sequences."""
import json
import random

try:
    from . import lab, oracle_harness as harness
except ImportError:
    import lab, oracle_harness as harness


def timer_cases(rng):
    for n in range(700):
        tps = rng.choice([20.0, 20.0, 10.0, 60.0, 1.0, 144.0])
        start = rng.choice([0, 0, 1234, 10**9])
        now = start
        ops = []
        for _ in range(rng.randrange(1, 40)):
            kind = rng.choice(['adv', 'adv', 'adv', 'real', 'pause', 'frozen'])
            if kind in ('adv', 'real'):
                now += rng.choice([0, 1, 2, 7, 16, 17, 33, 49, 50, 51, 99, 100, 101, 250, 999, 5000, 70000])
                if rng.random() < 0.03:
                    now -= rng.randrange(0, 30)
            if kind == 'adv':
                mspt = rng.choice([None, None, None, 50.0, 25.0, 100.0, 500.0, 1000.0, 0.5, 5.0, 0.1])
                ops.append(['adv', now, None if mspt is None else lab.fbits(mspt)])
            elif kind == 'real':
                ops.append(['real', now])
            else:
                ops.append([kind, rng.random() < 0.5])
        yield dict(t='timer', tps=lab.fbits(tps), start=start, ops=ops)


def rate_cases(rng):
    rates = [20.0, 1.0, 0.5, 0.0, -3.0, 100.0, 10000.0, 3.3, 1e-3, 33.333, 7.0, float('nan'), float('inf'), 59.99]
    for n in range(500):
        ops = []
        clock = 0
        for _ in range(rng.randrange(1, 40)):
            kind = rng.choice(['rate', 'freeze', 'step', 'stopstep', 'sprint', 'check', 'check', 'check', 'end', 'stopsprint',
                               'tick', 'tick', 'tick'])
            if kind == 'rate':
                ops.append(['rate', lab.fbits(rng.choice(rates))])
            elif kind == 'freeze':
                ops.append(['freeze', rng.random() < 0.5])
            elif kind == 'step':
                ops.append(['step', rng.choice([1, 2, 5, 20, 0, -1])])
            elif kind == 'sprint':
                ops.append(['sprint', rng.choice([1, 2, 3, 10, 100, 0])])
            elif kind in ('check', 'end'):
                clock += rng.choice([0, 1, 1000, 50_000_000, 123_456_789, 3_000_000_000])
                ops.append([kind, clock])
            else:
                ops.append([kind])
        yield dict(t='rate', ops=ops)


def scale_cases(rng):
    sizes = [(1, 1), (2, 2), (319, 239), (320, 240), (321, 241), (640, 480), (854, 480), (1280, 720), (1920, 1080),
             (2560, 1440), (3840, 2160), (7680, 4320), (0, 0), (1000, 100), (100, 1000), (641, 481), (333, 777)]
    for w, h in sizes:
        for maxs in [0, 1, 2, 3, 4, 5, 6, 8, 100, -1, 7]:
            for uni in (False, True):
                sw, sh = rng.choice([(w, h), (max(w // 2, 1), max(h // 2, 1)), (w + 1, h + 3)])
                yield dict(t='scale', w=w, h=h, max=maxs, uni=uni, sw=sw, sh=sh,
                           x=lab.dbits(rng.uniform(0, w)), y=lab.dbits(rng.uniform(0, h)))


def limit_cases(rng):
    for iconified, focused in [(True, False), (True, True), (False, True)]:
        for opt in (10, 30, 60, 120, 140, 260):
            for afk in (False, True):
                for ago in (0, 1000, 59000, 61000, 599000, 601000, 5_000_000):
                    for level in (False, True):
                        for screen in (False, True):
                            for overlay in (False, True):
                                yield dict(t='limit', iconified=iconified, focused=focused, opt=opt, afk=afk, ago=ago,
                                           level=level, screen=screen, overlay=overlay)


def mouse_cases(rng):
    for n in range(900):
        sens = rng.choice([0.0, 0.5, 1.0, 0.25, 0.1, 0.75, 0.3333, rng.random()])
        time = rng.choice([0.0, 10.0, 12345.678])
        ops = []
        for _ in range(rng.randrange(1, 40)):
            kind = rng.choice(['move', 'move', 'move', 'frame', 'frame', 'ignore'])
            if kind == 'move':
                x, y = rng.uniform(0, 1920), rng.uniform(0, 1080)
                big = rng.choice([1, 1, 1, 50, 5000])
                ops.append(['move', lab.dbits(x), lab.dbits(y), lab.dbits(rng.uniform(-30, 30) * big), lab.dbits(rng.uniform(-30, 30) * big)])
            elif kind == 'frame':
                time += rng.choice([0.0, 0.001, 0.0069, 0.016, 0.0167, 0.05, 0.5, 2.0])
                ops.append(['frame', lab.dbits(time)])
            else:
                ops.append(['ignore'])
        yield dict(t='mouse', sens=lab.dbits(sens), smooth=rng.random() < 0.4, scoping=rng.random() < 0.3,
                   ix=rng.random() < 0.3, iy=rng.random() < 0.3, grabbed=rng.random() < 0.85, active=rng.random() < 0.9, ops=ops)


def scroll_cases(rng):
    for n in range(700):
        ops = []
        for _ in range(rng.randrange(1, 40)):
            x = rng.choice([0.0, 0.0, 0.0, 1.0, -1.0, 0.25, -0.25, 0.4, 2.0, 5.5, -7.0, rng.uniform(-3, 3)])
            y = rng.choice([0.0, 1.0, -1.0, 1.0, -1.0, 0.1, -0.3, 0.5, -0.5, 2.0, -3.0, 120.0, rng.uniform(-3, 3)])
            ops.append([lab.dbits(x), lab.dbits(y)])
        case = dict(t='scroll', discrete=rng.random() < 0.4, sens=lab.dbits(rng.choice([1.0, 0.01, 10.0, 0.5, 2.0, 3.1622776601683795])),
                    slot=rng.randrange(9), ops=ops)
        yield case
        yield dict(case, spectator=True)
    # Bounds, recovery from each bound, fractional accumulation, and horizontal-only input.
    yield dict(t='scroll', discrete=False, sens=lab.dbits(1.0), slot=4, spectator=True,
               ops=[[lab.dbits(x), lab.dbits(y)] for x, y in
                    [(0, 100), (0, -1), (0, -100), (0, 1), (0, .4), (0, .7),
                     (0, -.4), (0, -.7), (5, 0), (-5, 0)]])


def key_cases(rng):
    for n in range(900):
        toggle = rng.random() < 0.8
        ops = []
        for _ in range(rng.randrange(1, 50)):
            ops.append(rng.choice([['option', rng.random() < 0.5], ['down', True], ['down', False], ['down', True], ['release'],
                                   ['restore'], ['click'], ['consume'], ['consume'], ['ingame']]))
        yield dict(t='key', toggle=toggle, keyboard=rng.random() < 0.7, restore=rng.random() < 0.6, ops=ops)


def make_cases(build):
    rng = random.Random(0x4c4f4f50)
    path = build / 'cases.jsonl'
    count = 0
    with path.open('w') as out:
        for generator in (timer_cases, rate_cases, scale_cases, limit_cases, mouse_cases, scroll_cases, key_cases):
            for case in generator(rng):
                out.write(json.dumps(case, separators=(',', ':')) + '\n')
                count += 1
    return path, count


def main():
    scope = ('Real DeltaTracker.Timer (float partial ticks from integer milliseconds, pause residual, frozen partial tick, '
             'real-time delta cap), ServerTickRateManager (set rate, freeze, step, sprint including the sprint report text), '
             'Window.calculateScale/setGuiScale and MouseHandler scaled positions, FramerateLimitTracker throttle reasons and '
             'limits (not the focus-lost exclusive-fullscreen branch, which reads SDL), MouseHandler.onMove/turnPlayer with '
             'sensitivity, cinematic smoothing, scoping and inversion, Entity.turn clamps, onScroll with ScrollWheelHandler '
             'accumulation, discrete scrolling, hotbar selection and spectator flight speed, and ToggleKeyMapping/KeyMapping down, click and '
             'screen-release state. The frame clock source (Blaze3D.getTime, System.nanoTime) is injected, so sleeping in '
             'FramerateLimiter and vsync present-mode selection are covered by Rust unit tests only.')
    harness.differential(
        'rust-game-loop-parity', 'VanillaGameLoopOracle', 'game_loop_probe', make_cases, scope,
        sources=[lab.ROOT / 'crates/horizon-client/src/clock.rs']
        + [lab.ROOT / f'crates/horizon-client-input/src/{name}.rs' for name in ('mouse', 'window', 'keymapping')])


if __name__ == '__main__':
    main()
