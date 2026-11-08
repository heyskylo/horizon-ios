#!/usr/bin/env python3
"""Redstone machines through the real tick loop in vanilla 26.3 and in Horizon, one tick at a time.

  redstone_diff.py [--machines N] [--seed S] [--ticks T] [--only NAME...] [--keep DIR]

The component harnesses (tools/test_block_update_parity.py and the others) call one block's code in a
synthetic level. This runs whole machines: each is a list of /setblock commands (a random "soup" of
redstone components from --seed, or a hand-built circuit) with commands that change it at set ticks,
in a frozen world that steps one tick at a time (`tick freeze`, `tick step 1`). After the setup and
after every tick both games report the machine's blocks (with their properties), and the first tick
where the blocks differ is printed with the ones that differ. The entities near the machine are recorded
too (see --keep) but not compared yet.

Vanilla is the client of tools/perf_bench.py's driver (mods/horizon/src/bench: RedstoneDiff snapshots
the box around every stepped overworld tick; no server, no EULA); Horizon is horizon-ctl on the same
commands, reading `simulation blocks`. Needs the same artifacts as perf_bench.py (your own client
jar, built horizon-client and horizon-ctl, prepared resources).
"""
import argparse
import json
import os
import random
import re
import subprocess
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / 'tools'))
import lab  # noqa: E402

ORIGIN = (200, 250, 200)
SPACING = 24
SIZE = (8, 3, 8)  # a soup's extent above its floor: x, y, z
WORLD = 'RedstoneDiff'
SEED = 1234
SIX = ['north', 'south', 'east', 'west', 'up', 'down']
FOUR = SIX[:4]

PRELUDE = [
    'gamerule send_command_feedback false', 'gamerule spawn_mobs false', 'gamerule advance_time false',
    'gamerule advance_weather false', 'gamerule random_tick_speed 0', 'time set 6000', 'weather clear',
]


def box_of(i, size=SIZE):
    ox, oy, oz = ORIGIN[0] + SPACING * i, ORIGIN[1], ORIGIN[2]
    return [ox - 1, oy - 1, oz - 1, ox + size[0], oy + size[1] + 1, oz + size[2]]


def component(rng):
    """One block state for a soup cell, as /setblock takes it."""
    r = rng.random()
    table = [
        (0.38, lambda: 'air'),
        (0.10, lambda: 'redstone_wire'),
        (0.05, lambda: f'repeater[facing={rng.choice(FOUR)},delay={rng.randint(1, 4)}]'),
        (0.04, lambda: f'comparator[facing={rng.choice(FOUR)},mode={rng.choice(["compare", "subtract"])}]'),
        (0.03, lambda: 'redstone_torch'),
        (0.03, lambda: f'redstone_wall_torch[facing={rng.choice(FOUR)}]'),
        (0.06, lambda: f'observer[facing={rng.choice(SIX)}]'),
        (0.04, lambda: f'piston[facing={rng.choice(SIX)}]'),
        (0.04, lambda: f'sticky_piston[facing={rng.choice(SIX)}]'),
        (0.07, lambda: 'stone'),
        (0.03, lambda: 'slime_block'),
        (0.02, lambda: 'honey_block'),
        (0.02, lambda: 'glass'),
        (0.02, lambda: 'redstone_block'),
        (0.02, lambda: 'redstone_lamp'),
        (0.02, lambda: f'lever[face={rng.choice(["floor", "wall", "ceiling"])},facing={rng.choice(FOUR)},powered={rng.choice(["true", "false"])}]'),
        (0.01, lambda: 'note_block'),
        (0.01, lambda: 'target'),
        (0.01, lambda: f'dispenser[facing={rng.choice(SIX)}]'),
        (0.01, lambda: f'dropper[facing={rng.choice(SIX)}]'),
        (0.01, lambda: f'hopper[facing={rng.choice(["down"] + FOUR)}]'),
        (0.01, lambda: 'daylight_detector'),
    ]
    acc = 0.0
    for p, make in table:
        acc += p
        if r < acc:
            return make()
    return 'air'


def soup(i, rng, ticks):
    ox, oy, oz = ORIGIN[0] + SPACING * i, ORIGIN[1], ORIGIN[2]
    sx, sy, sz = SIZE
    setup = [f'fill {ox - 1} {oy - 1} {oz - 1} {ox + sx} {oy - 1} {oz + sz} stone']
    for y in range(sy):
        for x in range(sx):
            for z in range(sz):
                state = component(rng)
                if state != 'air':
                    setup.append(f'setblock {ox + x} {oy + y} {oz + z} {state}')

    def cell():
        return rng.randrange(sx), rng.randrange(sy), rng.randrange(sz)

    events = {}
    for tick in (0, 3, 8, 14):
        x, y, z = cell()
        events.setdefault(tick, []).append(f'setblock {ox + x} {oy + y} {oz + z} redstone_block')
    for tick in (6, 11, 19):
        x, y, z = cell()
        events.setdefault(tick, []).append(f'setblock {ox + x} {oy + y} {oz + z} air')
    return {'name': f'soup-{i}', 'setup': setup, 'events': events, 'box': box_of(i), 'ticks': ticks}


def fixed(i, name, ticks):
    """A few circuits that update order shows in. (x, y, z) are offsets from the machine's origin."""
    ox, oy, oz = ORIGIN[0] + SPACING * i, ORIGIN[1], ORIGIN[2]
    at = lambda x, y, z: f'{ox + x} {oy + y} {oz + z}'
    floor = f'fill {ox - 1} {oy - 1} {oz - 1} {ox + 15} {oy - 1} {oz + 8} stone'
    slab = name.startswith('slab-')
    if slab:
        name = name[5:]
        floor = f'fill {ox - 1} {oy - 1} {oz - 1} {ox + 8} {oy - 1} {oz + 8} stone'
    setup, events = [floor], {}
    if name == 'observer-line':
        for x in range(1, 7):
            setup.append(f'setblock {at(x, 0, 2)} observer[facing=east]')
        events[1] = [f'setblock {at(7, 0, 2)} stone']
        events[9] = [f'setblock {at(7, 0, 2)} air']
    elif name == 'piston-slime-train':
        setup += [f'setblock {at(1, 0, 2)} sticky_piston[facing=east]', f'setblock {at(2, 0, 2)} slime_block',
                  f'setblock {at(3, 0, 2)} stone', f'setblock {at(4, 0, 2)} slime_block',
                  f'setblock {at(2, 1, 2)} piston[facing=east]', f'setblock {at(0, 0, 2)} lever[face=floor,facing=east,powered=false]']
        events[2] = [f'setblock {at(0, 0, 2)} redstone_block']
        events[12] = [f'setblock {at(0, 0, 2)} air']
    elif name == 'single-piston':
        setup += [f'setblock {at(2, 0, 2)} piston[facing=east]', f'setblock {at(3, 0, 2)} stone']
        events[2] = [f'setblock {at(2, 1, 2)} redstone_block']
        events[10] = [f'setblock {at(2, 1, 2)} air']
    elif name == 'single-sticky':
        setup += [f'setblock {at(2, 0, 2)} sticky_piston[facing=east]', f'setblock {at(3, 0, 2)} stone']
        events[2] = [f'setblock {at(2, 1, 2)} redstone_block']
        events[10] = [f'setblock {at(2, 1, 2)} air']
    elif name.startswith('fl-'):
        # train-a on floors of different shapes (z is the row the machine is in: 2)
        kind = name[3:]
        rows = {'row': (2, 2), 'three': (1, 3), 'five': (0, 4), 'slab': (-1, 8)}[kind]
        setup = [f'fill {at(-1, -1, -1)} {at(15, -1, 8)} air', f'fill {at(-1, -1, rows[0])} {at(8, -1, rows[1])} stone',
                 f'setblock {at(1, 0, 2)} sticky_piston[facing=east]', f'setblock {at(2, 0, 2)} slime_block']
        events[2] = [f'setblock {at(0, 0, 2)} redstone_block']
        events[12] = [f'setblock {at(0, 0, 2)} air']
    elif name.startswith('drag-'):
        # A sticky piston pushes a slime block that drags a row of N stone blocks under it, the row
        # pushing the blocks ahead of it: N + 1 blocks move.
        n = int(name[5:])
        setup = [f'fill {at(-1, -1, -1)} {at(15, -1, 8)} air', f'fill {at(2, -1, 2)} {at(1 + n, -1, 2)} stone',
                 f'setblock {at(1, 0, 2)} sticky_piston[facing=east]', f'setblock {at(2, 0, 2)} slime_block']
        events[2] = [f'setblock {at(0, 0, 2)} redstone_block']
        events[12] = [f'setblock {at(0, 0, 2)} air']
    elif name.startswith('e-'):
        kind = name[2:]
        if kind == 'back-power':
            setup += [f'setblock {at(1, 0, 2)} sticky_piston[facing=east]', f'setblock {at(2, 0, 2)} stone']
        elif kind == 'slime-floor':
            setup += [f'setblock {at(1, 0, 2)} sticky_piston[facing=east]', f'setblock {at(2, 0, 2)} slime_block']
        elif kind == 'slime-air':
            setup += [f'fill {at(0, -1, 0)} {at(8, -1, 4)} air', f'setblock {at(1, 0, 2)} sticky_piston[facing=east]',
                      f'setblock {at(2, 0, 2)} slime_block']
        elif kind == 'slime-air-above':
            setup += [f'fill {at(0, -1, 0)} {at(8, -1, 4)} air', f'setblock {at(1, 0, 2)} sticky_piston[facing=east]',
                      f'setblock {at(2, 0, 2)} stone', f'setblock {at(2, 1, 2)} slime_block']
        events[2] = [f'setblock {at(0, 0, 2)} redstone_block']
        events[12] = [f'setblock {at(0, 0, 2)} air']
    elif name.startswith('push-'):
        # A piston pushes a chain of N stone blocks east; 12 is vanilla's limit, 13 blocks it.
        n = int(name[5:])
        setup.append(f'setblock {at(0, 0, 2)} piston[facing=east]')
        setup += [f'setblock {at(1 + k, 0, 2)} stone' for k in range(n)]
        events[2] = [f'setblock {at(0, 1, 2)} redstone_block']
        events[10] = [f'setblock {at(0, 1, 2)} air']
    elif name.startswith('train-'):
        # A sticky piston pushes a line of blocks east; the variants add to the line.
        kind = name[6:]
        setup += [f'setblock {at(1, 0, 2)} sticky_piston[facing=east]', f'setblock {at(2, 0, 2)} slime_block']
        if kind in ('b', 'c', 'd'):
            setup += [f'setblock {at(3, 0, 2)} stone', f'setblock {at(4, 0, 2)} slime_block']
        if kind in ('c', 'd'):
            setup.append(f'setblock {at(2, 1, 2)} piston[facing=east]')
        if kind == 'd':
            setup.append(f'setblock {at(5, 0, 2)} stone')
        events[2] = [f'setblock {at(0, 0, 2)} redstone_block']
        events[12] = [f'setblock {at(0, 0, 2)} air']
    elif name == 'torch-ring':
        # Three wall torches on a column of stone, each powering the block the next one is on.
        for k, (x, z) in enumerate([(2, 2), (4, 2), (3, 4)]):
            setup.append(f'setblock {at(x, 0, z)} stone')
        setup += [f'setblock {at(3, 0, 2)} redstone_wire', f'setblock {at(4, 1, 2)} redstone_wire',
                  f'setblock {at(2, 1, 2)} redstone_torch', f'setblock {at(4, 0, 3)} redstone_wire',
                  f'setblock {at(3, 1, 4)} redstone_torch']
        events[0] = [f'setblock {at(2, 2, 2)} redstone_block']
        events[5] = [f'setblock {at(2, 2, 2)} air']
    elif name == 'repeater-pulse':
        setup += [f'setblock {at(1, 0, 2)} lever[face=floor,facing=east,powered=false]']
        for x in range(2, 7):
            setup.append(f'setblock {at(x, 0, 2)} repeater[facing=east,delay={1 + x % 4}]')
        setup.append(f'setblock {at(7, 0, 2)} redstone_lamp')
        events[0] = [f'setblock {at(1, 0, 2)} redstone_block']
        events[1] = [f'setblock {at(1, 0, 2)} air']
        events[10] = [f'setblock {at(1, 0, 2)} redstone_block']
    elif name == 'comparator-clock':
        setup += [f'setblock {at(2, 0, 2)} comparator[facing=east,mode=subtract]',
                  f'setblock {at(3, 0, 2)} redstone_wire', f'setblock {at(3, 0, 3)} repeater[facing=west,delay=1]',
                  f'setblock {at(2, 0, 3)} redstone_wire', f'setblock {at(1, 0, 2)} redstone_block']
        events[4] = [f'setblock {at(1, 0, 2)} air']
    elif name == 'observer-piston-bud':
        setup += [f'setblock {at(2, 0, 2)} observer[facing=east]', f'setblock {at(3, 0, 2)} stone',
                  f'setblock {at(1, 0, 2)} sticky_piston[facing=west]', f'setblock {at(1, 1, 2)} redstone_wire']
        events[2] = [f'setblock {at(3, 0, 2)} dirt']
        events[7] = [f'setblock {at(3, 0, 2)} air']
    else:
        raise ValueError(name)
    return {'name': ('slab-' if slab else '') + name, 'setup': setup, 'events': events,
            'box': box_of(i, (8, 3, 8) if slab else (15, 3, 8)), 'ticks': ticks}


# What a plain run builds before the soups, and the rest, which --only can name: the diagnostics that
# narrowed a difference down.
DEFAULT = ['observer-line', 'piston-slime-train', 'torch-ring', 'repeater-pulse', 'comparator-clock', 'observer-piston-bud', 'single-piston', 'single-sticky']
FIXED = DEFAULT + ['fl-row', 'fl-three', 'fl-five', 'fl-slab', 'slab-train-a', 'slab-train-b', 'slab-piston-slime-train', 'drag-1', 'drag-2', 'drag-3', 'drag-5', 'drag-7', 'drag-9', 'drag-10', 'drag-11', 'e-back-power', 'e-slime-floor', 'e-slime-air', 'e-slime-air-above', 'push-1', 'push-2', 'push-3', 'push-4', 'push-6', 'push-8', 'push-10', 'push-12', 'push-13', 'train-a', 'train-b', 'train-c', 'train-d']


def build(count, seed, ticks, only):
    """The machines to run, each with the batch of the game session it is in: eight to a session, at
    slots of the position they have among those selected, so the same selection is the same world."""
    machines = [('fixed', name) for name in (FIXED if only else DEFAULT)]
    machines += [('soup', i) for i in range(max(0, count - len(DEFAULT)))]
    names = {}
    for kind, key in machines:
        names[(kind, key)] = key if kind == 'fixed' else f'soup-s{seed}-{key}'
    selected = [mk for mk in machines if not only or names[mk] in only]
    out = []
    for index, (kind, key) in enumerate(selected):
        slot = index % 8
        # A soup is a function of its seed and number alone, whatever else is selected.
        m = fixed(slot, key, ticks) if kind == 'fixed' else soup(slot, random.Random(seed * 100003 + key), ticks)
        m['name'] = names[(kind, key)]
        out.append((index // 8, m))
    return out


def narrow(m, near, drop):
    """The machine with only the setblock commands near a point (offsets from its origin), in order."""
    dx, dy, dz, r = near
    ox, oy, oz = m['box'][0] + 1, m['box'][1] + 1, m['box'][2] + 1
    keep, n = [], 0
    for line in m['setup']:
        w = line.split()
        if w[0] == 'setblock':
            x, y, z = int(w[1]) - ox, int(w[2]) - oy, int(w[3]) - oz
            if max(abs(x - dx), abs(y - dy), abs(z - dz)) <= r:
                if n not in drop:
                    keep.append(line)
                print('  [%d] %s' % (n, line))
                n += 1
        else:
            keep.append(line)
    out = dict(m)
    out['setup'] = keep
    out['events'] = {}
    return out


def norm(state):
    """'id[b=1,a=2]' with its properties sorted."""
    m = re.fullmatch(r'([^\[]+)(?:\[(.*)\])?', state)
    name, props = m.group(1), m.group(2)
    if not props:
        return name
    return name + '[' + ','.join(sorted(props.split(','))) + ']'


def clean_blocks(lines):
    out = {}
    for line in lines:
        x, y, z, state = line.split(' ', 3)
        out[(int(x), int(y), int(z))] = norm(state)
    return out


# ---- vanilla -----------------------------------------------------------------------------------

def run_vanilla(batch, args, work):
    import perf_bench as pb
    manifest = pb.vanilla_manifest()
    jars = pb.vanilla_classpath(manifest)
    fabric = pb.runtime.fabric_libraries()
    mod = pb.build_bench_mod(jars + fabric)
    game_dir = work / 'vanilla'
    import shutil
    shutil.rmtree(game_dir, ignore_errors=True)
    pb.write_options(game_dir, 12)
    (game_dir / 'mods').mkdir(exist_ok=True)
    shutil.copy(mod, game_dir / 'mods')
    natives = work / 'natives'
    pb.extract_natives(jars, natives)
    ox, oy, oz = ORIGIN
    centre = (ox + SPACING * 3.5, oy + 6.0, oz + 4.0)
    machines = []
    for _, m in batch:
        entry = dict(m)
        entry['events'] = {str(k): v for k, v in m['events'].items()}
        machines.append(entry)
    machines[0]['setup'] = PRELUDE + machines[0]['setup']
    script = work / 'script.json'
    script.write_text(json.dumps({
        'world': WORLD, 'seed': SEED, 'settle': 0, 'seconds': 1, 'timeout': 1800,
        'scenes': [{'name': 'redstone', 'from': list(centre), 'to': list(centre), 'pitch': 0}],
        'redstone': machines,
    }))
    events = work / 'vanilla.events.jsonl'
    events.unlink(missing_ok=True)
    assets = work / 'assets'
    (assets / 'indexes').mkdir(parents=True, exist_ok=True)
    java = Path(os.environ['JAVA_HOME']) / 'bin/java' if os.environ.get('JAVA_HOME') else 'java'
    command = [str(java), '-Xmx2G', *pb.LAUNCHER_JVM, *pb.runtime.conditional_client_jvm_options(manifest),
               *pb.manifest_jvm_flags(manifest), '--enable-native-access=ALL-UNNAMED',
               f'-Djava.library.path={natives}', f'-Dorg.lwjgl.system.SharedLibraryExtractPath={natives}',
               f'-Dhorizon.bench.script={script}', f'-Dhorizon.bench.out={events}', '-Dfabric.log.disableAnsi=true',
               '-cp', os.pathsep.join(map(str, jars + fabric)), 'net.fabricmc.loader.impl.launch.knot.KnotClient',
               '--username', pb.BENCH_PLAYER, '--uuid', '00000000000000000000000000000000', '--accessToken', '0',
               '--offlineDeveloperMode', '--version', pb.lab.VERSION, '--gameDir', str(game_dir),
               '--assetsDir', str(assets), '--assetIndex', manifest['assetIndex']['id'],
               '--width', '854', '--height', '480']
    env = dict(os.environ, ALSOFT_DRIVERS='null')
    log = (work / 'vanilla.log').open('w')
    proc = subprocess.Popen(command, cwd=game_dir, env=env, stdout=log, stderr=subprocess.STDOUT)
    try:
        proc.wait(timeout=1800)
    except subprocess.TimeoutExpired:
        proc.kill()
        raise SystemExit('vanilla timed out; see ' + str(work / 'vanilla.log'))
    result = {}
    for line in events.read_text().splitlines():
        e = json.loads(line)
        if e.get('event') == 'redstone':
            result[e['machine']] = [
                {'tick': s['tick'], 'blocks': clean_blocks(s['blocks']), 'entities': s['entities']}
                for s in e['snapshots']]
        if e.get('event') == 'timeout':
            raise SystemExit('vanilla driver timed out in ' + e.get('phase', '?'))
    return result


# ---- Horizon -----------------------------------------------------------------------------------

class Ctl:
    def __init__(self, ctl, sock):
        self.ctl, self.sock = ctl, str(sock)

    def run(self, *args, check=True):
        p = subprocess.run([self.ctl, '--socket', self.sock, *map(str, args)], capture_output=True, text=True)
        if check and p.returncode:
            raise RuntimeError(f'{args}: {p.stdout} {p.stderr}')
        return p.stdout

    def json(self, *args):
        return json.loads(self.run(*args))

    def command(self, line):
        return self.run('command', line, check=False)

    def game_time(self):
        return self.json('simulation')['game_time']


def run_horizon(batch, args, work):
    ctl_path = ROOT / 'target' / args.profile / ('horizon-ctl.exe' if os.name == 'nt' else 'horizon-ctl')
    sock = work / 'horizon.sock'
    game_dir = work / 'horizon-game'
    import shutil
    shutil.rmtree(game_dir, ignore_errors=True)
    game_dir.mkdir(parents=True)
    env = lab.game_env(HORIZON_HOME=str(game_dir), HORIZON_TELEMETRY_URL='off')
    ctl = Ctl(ctl_path, sock)
    launch = subprocess.run([str(ctl_path), '--socket', str(sock), 'launch', '--world', WORLD, '--seed', str(SEED),
                             '--creative', '--wait', '--offscreen', '--resources', str(args.resources),
                             '--game-dir', str(game_dir), '--render-distance', '8', '--width', '640', '--height', '360'],
                            env=env, capture_output=True, text=True, timeout=900)
    if launch.returncode:
        raise SystemExit('horizon launch failed: ' + launch.stdout + launch.stderr)
    ox, oy, oz = ORIGIN
    ctl.command('gamemode spectator')
    ctl.run('tp', ox + SPACING * 3.5, oy + 6.0, oz + 4.0)
    time.sleep(8)
    result = {}
    try:
        first = True
        for _, m in batch:
            if first:
                for line in PRELUDE:
                    ctl.command(line)
                first = False
            ctl.command('tick freeze')
            for line in m['setup']:
                ctl.command(line)
            time.sleep(0.3)
            box = m['box']
            frames = []

            def snapshot(tick):
                # The server's own blocks: `blocks` is this window's copy, which the server's changes reach
                # in the next tick's broadcast, as vanilla's clients get them.
                reply = ctl.json('simulation', 'blocks', *box)
                near = ctl.json('entities', 16)
                frames.append({'tick': tick, 'blocks': clean_blocks([' '.join(map(str, b[:3])) + ' ' + b[3] for b in reply['blocks']]),
                               'entities': near})

            snapshot(0)
            for tick in range(m['ticks']):
                for line in m['events'].get(tick, []):
                    ctl.command(line)
                before = ctl.game_time()
                ctl.command('tick step 1')
                for _ in range(200):
                    if ctl.game_time() != before:
                        break
                    time.sleep(0.01)
                snapshot(tick + 1)
            result[m['name']] = frames
    finally:
        ctl.run('quit', check=False)
    return result


# ---- compare -----------------------------------------------------------------------------------

def diff_frames(a, b):
    """The first tick where two timelines differ: (tick, [(pos, a state, b state)]), or None."""
    for fa, fb in zip(a, b):
        ba, bb = fa['blocks'], fb['blocks']
        if ba != bb:
            rows = []
            for pos in sorted(set(ba) | set(bb)):
                if ba.get(pos) != bb.get(pos):
                    rows.append((pos, ba.get(pos, 'air'), bb.get(pos, 'air')))
            return fa['tick'], rows
    return None


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument('--machines', type=int, default=24, help='machines to run: the hand-built ones, then soups')
    p.add_argument('--seed', type=int, default=1)
    p.add_argument('--ticks', type=int, default=40)
    p.add_argument('--only', nargs='*', help='machine names')
    p.add_argument('--profile', default='fast', help='the cargo profile of the built horizon-client and horizon-ctl')
    p.add_argument('--resources', default=str(ROOT / '.local/horizon-client/26.3'))
    p.add_argument('--near', nargs=4, type=int, metavar=('DX', 'DY', 'DZ', 'R'),
                   help='keep only the setup commands within R blocks of this offset from the machine\'s origin')
    p.add_argument('--drop', nargs='*', type=int, default=[], help='with --near: leave out these setblock commands (by index)')
    p.add_argument('--keep', help='write the machines and both timelines here')
    p.add_argument('--skip-vanilla', help='reuse this vanilla timelines file (from --keep)')
    args = p.parse_args()
    machines = build(args.machines, args.seed, args.ticks, args.only)
    if args.near:
        machines = [(b, narrow(m, args.near, args.drop)) for b, m in machines]
    work = ROOT / '.local' / 'redstone-diff'
    work.mkdir(parents=True, exist_ok=True)
    batches = {}
    for batch_no, m in machines:
        batches.setdefault(batch_no, []).append((batch_no, m))
    vanilla, horizon = {}, {}
    for batch_no, batch in sorted(batches.items()):
        print(f'batch {batch_no}: {len(batch)} machines', flush=True)
        if args.skip_vanilla:
            vanilla = json.loads(Path(args.skip_vanilla).read_text())['vanilla']
            vanilla = {k: [{'tick': f['tick'], 'blocks': {tuple(map(int, q.split(','))): s for q, s in f['blocks'].items()}, 'entities': f['entities']} for f in v] for k, v in vanilla.items()}
        else:
            vanilla.update(run_vanilla(batch, args, work))
        horizon.update(run_horizon(batch, args, work))
    different = 0
    for _, m in machines:
        name = m['name']
        a, b = vanilla.get(name), horizon.get(name)
        if not a or not b:
            print(f'{name}: missing ({"vanilla" if not a else "horizon"})')
            different += 1
            continue
        d = diff_frames(a, b)
        if d is None:
            print(f'{name}: identical for {len(a) - 1} ticks')
            continue
        different += 1
        tick, rows = d
        print(f'{name}: DIFFERS from tick {tick}: {len(rows)} blocks')
        for pos, x, y in rows[:6]:
            print(f'    {pos}: vanilla {x}  horizon {y}')
    if args.keep:
        keep = Path(args.keep)
        keep.mkdir(parents=True, exist_ok=True)
        dump = lambda t: {k: [{'tick': f['tick'], 'blocks': {','.join(map(str, q)): s for q, s in f['blocks'].items()}, 'entities': f['entities']} for f in v] for k, v in t.items()}
        (keep / 'timelines.json').write_text(json.dumps({'vanilla': dump(vanilla), 'horizon': dump(horizon), 'machines': [m for _, m in machines]}))
    print(f'{different} of {len(machines)} machines differ')
    return 1 if different else 0


if __name__ == '__main__':
    sys.exit(main())
