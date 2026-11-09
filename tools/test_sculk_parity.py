#!/usr/bin/env python3
"""Differential sculk harness: sensors, calibrated sensors, shriekers, catalysts and wardens.

The Java oracle (VanillaSculkOracle) runs each scenario in a real, headless, flat vanilla server
level: it builds the blocks, places the player (with a warning level) and entities, and ticks the
level in ServerLevel.tick's order while scripted game events and block changes happen at given
ticks. Horizon runs the same scenarios through its game code (`HORIZON_SCULK_FIXTURE`, the
same server phases, with synthetic player and entity state). Both print, per tick, what changed at the watched blocks (state,
vibration, selector, last frequency, warning level, charge cursors), sounds, level events, the
player's warden warning level and darkness, new entities, and the level random. The harness
reports each scenario's first differing tick.

  python3 tools/test_sculk_parity.py [--only NAME ...] [--count N] [--rust-only] [--java-only] [--report-only]
"""
import argparse
import os
import random
import subprocess

try:
    from . import lab, oracle_harness, test_rust_parity as base, test_model_baking_parity as baking
except ImportError:
    import lab
    import oracle_harness
    import test_rust_parity as base
    import test_model_baking_parity as baking

FLOOR = -60  # top of the flat preset's grass layer + 1
SENSOR = 'minecraft:sculk_sensor'
CALIBRATED = 'minecraft:calibrated_sculk_sensor'
SHRIEKER = 'minecraft:sculk_shrieker'
CATALYST = 'minecraft:sculk_catalyst'

# Game events that sculk listens to (and some that it does not), by frequency.
EVENTS = ['step', 'swim', 'flap', 'projectile_land', 'hit_ground', 'splash', 'item_interact_finish', 'projectile_shoot',
          'instrument_play', 'entity_action', 'elytra_glide', 'unequip', 'entity_dismount', 'equip', 'entity_interact',
          'shear', 'entity_mount', 'entity_damage', 'drink', 'eat', 'container_close', 'block_close', 'block_deactivate',
          'block_detach', 'container_open', 'block_open', 'block_activate', 'block_attach', 'prime_fuse', 'note_block_play',
          'block_change', 'block_destroy', 'fluid_pickup', 'block_place', 'fluid_place', 'entity_place', 'lightning_strike',
          'teleport', 'entity_die', 'explode', 'item_interact_start', 'jukebox_play', 'shriek', 'resonate_3', 'resonate_11']


def handwritten():
    s = []

    def add(name, ticks, lines, opts=''):
        s.append((name, ticks, opts, lines))

    sensor = f'block 0 {FLOOR} 0 {SENSOR}'
    watch = f'watch 0 {FLOOR} 0'
    add('sensor_step', 60, [sensor, watch, 'player 3.5 -60 0.5', 'at 2 event minecraft:step 3.5 -60 0.5 player'])
    add('sensor_out_of_range', 30, [sensor, watch, 'player 9.5 -60 0.5', 'at 2 event minecraft:step 9.5 -60 0.5 player'])
    add('sensor_edge_of_range', 40, [sensor, watch, 'player 8.5 -60 0.5', 'at 2 event minecraft:step 8.5 -60 0.5 player'])
    add('sensor_wool', 40, [sensor, watch, f'fill 2 {FLOOR} -3 2 {FLOOR + 3} 3 minecraft:white_wool',
                            'player 4.5 -60 0.5', 'at 2 event minecraft:block_place 4.5 -60 0.5 player'])
    add('sensor_sneak', 60, [sensor, watch, 'player 3.5 -60 0.5 sneak', 'at 2 event minecraft:step 3.5 -60 0.5 player',
                             'at 3 event minecraft:entity_damage 3.5 -60 0.5 player'])
    add('sensor_spectator', 40, [sensor, watch, 'player 3.5 -60 0.5 spectator', 'at 2 event minecraft:step 3.5 -60 0.5 player'])
    add('sensor_wool_dampens', 40, [sensor, watch, 'player 3.5 -60 0.5',
                                    'at 2 event minecraft:step 3.5 -60 0.5 player minecraft:white_wool'])
    add('sensor_own_place', 40, [sensor, watch, 'at 2 event minecraft:block_place 0.5 -59.5 0.5 none',
                                 'at 6 event minecraft:block_change 0.5 -59.5 0.5 none'])
    add('sensor_selector', 60, [sensor, watch, 'player 3.5 -60 0.5',
                                'at 2 event minecraft:step 5.5 -60 0.5 player',
                                'at 2 event minecraft:explode 5.5 -60 0.5 none',
                                'at 2 event minecraft:eat 2.5 -60 0.5 player',
                                'at 2 event minecraft:block_place 6.5 -60 0.5 none',
                                'at 3 event minecraft:block_destroy 1.5 -60 0.5 none'])
    add('sensor_cooldown', 120, [sensor, watch, 'player 3.5 -60 0.5'] +
        [f'at {t} event minecraft:block_change 4.5 -60 {0.5 + (t % 5)} player' for t in range(2, 100, 3)])
    add('sensor_redstone', 60, [sensor, watch, f'block 1 {FLOOR} 0 minecraft:redstone_wire', f'block 2 {FLOOR} 0 minecraft:redstone_wire',
                                f'block 0 {FLOOR} 1 minecraft:comparator[facing=north]', f'block 0 {FLOOR} 2 minecraft:redstone_wire',
                                f'watch 1 {FLOOR} 0', f'watch 2 {FLOOR} 0', f'watch 0 {FLOOR} 1', f'watch 0 {FLOOR} 2',
                                'player 5.5 -60 3.5', 'at 2 event minecraft:container_open 5.5 -60 3.5 player'])
    add('sensor_resonance', 80, [sensor, watch, f'block 1 {FLOOR} 0 minecraft:amethyst_block',
                                 f'block 4 {FLOOR} 0 {CALIBRATED}[facing=north]', f'watch 4 {FLOOR} 0',
                                 f'block 6 {FLOOR} 0 {SENSOR}', f'watch 6 {FLOOR} 0',
                                 'player -3.5 -60 0.5', 'at 2 event minecraft:block_place -3.5 -60 0.5 player'])
    for power in (0, 3, 7, 13):
        cal = f'block 0 {FLOOR} 0 {CALIBRATED}[facing=north]'
        lines = [cal, watch, 'player 3.5 -60 0.5']
        if power:
            # A redstone wire behind (south of a north-facing sensor) carries the input power.
            lines += [f'block 0 {FLOOR} 1 minecraft:redstone_wire[power={power},north=side,south=side]', f'watch 0 {FLOOR} 1']
        lines += [f'at {2 + 4 * i} event minecraft:{e} 3.5 -60 {0.5 + i} player' for i, e in
                  enumerate(['block_change', 'eat', 'step', 'block_destroy', 'container_open', 'entity_die', 'drink', 'block_place'])]
        add(f'calibrated_power_{power}', 90, lines)
    shrieker = f'block 0 {FLOOR} 0 {SHRIEKER}'
    # Shriekers hear only a sensor's tendrils clicking: a sensor beside it hears the player.
    near = [f'block 2 {FLOOR} 0 {SENSOR}', f'watch 2 {FLOOR} 0']
    for warn in (0, 1, 2, 3):
        add(f'shrieker_warn_{warn}', 120, [f'block 0 {FLOOR} 0 {SHRIEKER}[can_summon=true]', watch, *near, f'player 3.5 -60 0.5 warn={warn}',
                                           'at 2 event minecraft:step 3.5 -60 0.5 player'])
    add('shrieker_no_summon', 120, [shrieker, watch, *near, 'player 3.5 -60 0.5 warn=2', 'at 2 event minecraft:step 3.5 -60 0.5 player'])
    add('shrieker_peaceful', 120, [f'block 0 {FLOOR} 0 {SHRIEKER}[can_summon=true]', watch, *near, 'player 3.5 -60 0.5 warn=1',
                                   'at 2 event minecraft:step 3.5 -60 0.5 player'], 'difficulty=peaceful')
    add('shrieker_rule_off', 120, [f'block 0 {FLOOR} 0 {SHRIEKER}[can_summon=true]', watch, *near, 'player 3.5 -60 0.5 warn=1',
                                   'at 2 event minecraft:step 3.5 -60 0.5 player'], 'nowardens')
    add('shrieker_unheard', 60, [f'block 0 {FLOOR} 0 {SHRIEKER}[can_summon=true]', watch, *near, 'player 3.5 -60 0.5',
                                 'at 2 event minecraft:step 3.5 -60 0.5 none', 'at 4 event minecraft:block_change 3.5 -60 0.5 player',
                                 'at 6 event minecraft:eat 3.5 -60 0.5 player'])
    add('shrieker_cooldown', 600, [f'block 0 {FLOOR} 0 {SHRIEKER}[can_summon=true]', watch, *near, 'player 3.5 -60 0.5'] +
        [f'at {t} event minecraft:step 3.5 -60 0.5 player' for t in range(2, 600, 37)])
    add('shrieker_removed_while_shrieking', 60, [f'block 0 {FLOOR} 0 {SHRIEKER}[can_summon=true]', watch, *near, 'player 3.5 -60 0.5 warn=2',
                                                 'at 2 event minecraft:step 3.5 -60 0.5 player', f'at 20 set 0 {FLOOR} 0 minecraft:air'])
    add('shrieker_summons', 200, [f'block 0 {FLOOR} 0 {SHRIEKER}[can_summon=true]', watch, *near, 'player 3.5 -60 0.5 warn=3',
                                  'at 2 event minecraft:step 3.5 -60 0.5 player'])
    add('shrieker_creative', 60, [f'block 0 {FLOOR} 0 {SHRIEKER}[can_summon=true]', watch, *near, 'player 3.5 -60 0.5 creative warn=2',
                                  'at 2 event minecraft:step 3.5 -60 0.5 player'])
    # Catalysts: a mob killed by the player nearby charges the catalyst, which blooms and spreads sculk.
    region = f'region -9 {FLOOR - 3} -9 9 {FLOOR + 3} 9'
    # Mobs without loot (items are another harness's work).
    for name, mob, floor in [('catalyst_silverfish', 'silverfish', 'minecraft:sculk'), ('catalyst_endermite', 'endermite', 'minecraft:stone'),
                             ('catalyst_axolotl_grass', 'axolotl', None), ('catalyst_blaze', 'blaze', 'minecraft:deepslate')]:
        lines = [f'block 0 {FLOOR} 0 {CATALYST}', watch, region, 'player 5.5 -60 5.5', f'mob minecraft:{mob} 2.5 -60 0.5 0 7 noai',
                 'at 3 kill m0']
        if floor:
            lines.insert(0, f'fill -6 {FLOOR - 1} -6 6 {FLOOR - 1} 6 {floor}')
        add(name, 200, lines)
    add('catalyst_far', 60, [f'block 0 {FLOOR} 0 {CATALYST}', watch, region, 'player 5.5 -60 5.5',
                             'mob minecraft:silverfish 9.5 -60 0.5 0 7 noai', 'at 3 kill m0'])
    add('catalyst_two', 200, [f'block 0 {FLOOR} 0 {CATALYST}', watch, f'block 4 {FLOOR} 0 {CATALYST}', f'watch 4 {FLOOR} 0', region,
                              f'fill -6 {FLOOR - 1} -6 6 {FLOOR - 1} 6 minecraft:sculk', 'player 5.5 -60 5.5',
                              'mob minecraft:endermite 3.5 -60 0.5 0 7 noai', 'mob minecraft:silverfish -1.5 -60 2.5 0 8 noai',
                              'at 3 kill m0', 'at 4 kill m1'])
    add('catalyst_unkilled', 60, [f'block 0 {FLOOR} 0 {CATALYST}', watch, region, 'player 5.5 -60 5.5',
                                  'mob minecraft:silverfish 2.5 -60 0.5 0 7 noai', 'at 3 kill m0 3'])
    return s


def random_scenarios(count, seed):
    rng = random.Random(seed)
    out = []
    sculk = [SENSOR, SENSOR, SENSOR, CALIBRATED, CALIBRATED, SHRIEKER, SHRIEKER]
    others = ['minecraft:white_wool', 'minecraft:red_wool', 'minecraft:amethyst_block', 'minecraft:stone', 'minecraft:redstone_wire',
              'minecraft:redstone_block', 'minecraft:white_carpet', 'minecraft:glass']
    facings = ['north', 'south', 'east', 'west']
    for n in range(count):
        lines = []
        cells = {}
        for _ in range(rng.randrange(1, 5)):
            x, y, z = rng.randrange(-4, 5), FLOOR + rng.randrange(0, 3), rng.randrange(-4, 5)
            kind = rng.choice(sculk)
            if kind == CALIBRATED:
                kind += f'[facing={rng.choice(facings)}]'
            elif kind == SHRIEKER and rng.random() < 0.6:
                kind += '[can_summon=true]'
            cells[(x, y, z)] = kind
        for _ in range(rng.randrange(0, 10)):
            x, y, z = rng.randrange(-5, 6), FLOOR + rng.randrange(0, 3), rng.randrange(-5, 6)
            cells.setdefault((x, y, z), rng.choice(others))
        for (x, y, z), state in sorted(cells.items()):
            # Nothing floats: blocks above the floor stand on stone.
            for below in range(FLOOR, y):
                cells.setdefault((x, below, z), 'minecraft:stone')
        # Wire and carpet stand on a full block, or they would pop (items are another harness's work).
        solid = {'minecraft:' + n for n in ('stone', 'white_wool', 'red_wool', 'amethyst_block', 'redstone_block', 'glass')}
        for (x, y, z), state in sorted(cells.items(), key=lambda c: c[0][1]):
            if ('redstone_wire' in state or 'carpet' in state) and y > FLOOR and cells[(x, y - 1, z)] not in solid:
                cells[(x, y, z)] = 'minecraft:stone'
        for (x, y, z), state in sorted(cells.items(), key=lambda c: (c[0][1], c[0][0], c[0][2])):
            lines.append(f'block {x} {y} {z} {state}')
            if any(k in state for k in ('sculk', 'redstone_wire')):
                lines.append(f'watch {x} {y} {z}')
        px, pz = rng.randrange(-7, 8) + 0.5, rng.randrange(-7, 8) + 0.5
        mode = rng.choice(['', '', '', '', ' creative', ' sneak'])
        warn = rng.choice([0, 0, 1, 2, 3, 3])
        lines.append(f'player {px} {FLOOR + 3} {pz}{mode} warn={warn}')
        ticks = rng.choice([60, 100, 160])
        for _ in range(rng.randrange(1, 12)):
            t = rng.randrange(1, ticks - 20)
            ev = rng.choice(EVENTS)
            x, y, z = rng.uniform(-9, 9), FLOOR + rng.uniform(-1, 4), rng.uniform(-9, 9)
            source = rng.choice(['player', 'player', 'player', 'none'])
            if source == 'player':
                x, y, z = px, FLOOR + 3, pz
            lines.append(f'at {t} event minecraft:{ev} {x!r} {y!r} {z!r} {source}')
        # A block nothing stands on is replaced.
        free = [c for c in cells if (c[0], c[1] + 1, c[2]) not in cells]
        if free and rng.random() < 0.3:
            (x, y, z) = rng.choice(free)
            lines.append(f'at {rng.randrange(1, ticks - 5)} set {x} {y} {z} {rng.choice(["minecraft:air", "minecraft:stone"])}')
        opts = rng.choice(['', '', '', 'difficulty=peaceful', 'nowardens', f'levelseed={rng.randrange(1 << 30)}'])
        out.append((f'random_{n}', ticks, opts, lines))
    return out


def catalyst_scenarios(count, seed):
    """Catalysts over random terrain charged by kills: blooms, cursors, veins, growths."""
    rng = random.Random(seed * 7919 + 13)
    out = []
    ground = ['minecraft:stone', 'minecraft:dirt', 'minecraft:deepslate', 'minecraft:sculk', 'minecraft:grass_block',
              'minecraft:moss_block', 'minecraft:tuff', 'minecraft:cobblestone', 'minecraft:sand']
    extras = ['minecraft:stone', 'minecraft:glass', 'minecraft:oak_planks', 'minecraft:water', 'minecraft:sculk_vein[down=true]',
              'minecraft:sculk', 'minecraft:sculk_catalyst', 'minecraft:white_wool', 'minecraft:oak_slab']
    # Mobs without loot (item drops are another harness's work).
    mobs = ['silverfish', 'endermite', 'axolotl', 'silverfish', 'vex']
    for n in range(count):
        lines = []
        for x in range(-7, 8):
            for z in range(-7, 8):
                if rng.random() < 0.7:
                    lines.append(f'block {x} {FLOOR - 1} {z} {rng.choice(ground)}')
        cells = {}
        for _ in range(rng.randrange(0, 14)):
            x, y, z = rng.randrange(-6, 7), FLOOR + rng.randrange(0, 2), rng.randrange(-6, 7)
            cells[(x, y, z)] = rng.choice(extras)
        for (x, y, z), state in sorted(cells.items(), key=lambda c: c[0][1]):
            if y > FLOOR and (x, FLOOR, z) not in cells:
                lines.append(f'block {x} {FLOOR} {z} minecraft:stone')
            lines.append(f'block {x} {y} {z} {state}')
        catalysts = []
        for _ in range(rng.choice([1, 1, 1, 2, 3])):
            x, z = rng.randrange(-4, 5), rng.randrange(-4, 5)
            catalysts.append((x, z))
            lines.append(f'block {x} {FLOOR} {z} {CATALYST}')
            lines.append(f'watch {x} {FLOOR} {z}')
        lines.append(f'region -9 {FLOOR - 3} -9 9 {FLOOR + 3} 9')
        lines.append(f'player {rng.randrange(-8, 9) + 0.5} {FLOOR + 4} {rng.randrange(-8, 9) + 0.5}')
        ticks = rng.choice([120, 200, 300])
        kills = rng.randrange(1, 12)
        for i in range(kills):
            cx, cz = rng.choice(catalysts)
            x, z = cx + rng.randrange(-6, 7) + 0.5, cz + rng.randrange(-6, 7) + 0.5
            lines.append(f'mob minecraft:{rng.choice(mobs)} {x} {FLOOR + 2} {z} 0 {rng.randrange(1000)} noai')
            lines.append(f'at {rng.randrange(1, ticks // 2)} kill m{i}')
        opts = f'levelseed={rng.randrange(1 << 30)}'
        out.append((f'catalyst_random_{n}', ticks, opts, lines))
    return out


def mob_scenarios():
    """A mob's own game events: a creeper's fuse (Creeper.tick's PRIME_FUSE once ignited), a mob
    eating (Mob.ate's EAT), in range of a sensor or not; the fuse ends after the scenario. These run
    last: their mobs take entity ids from vanilla's global counter, and experience orbs merge by id
    (ExperienceOrb.canMerge), so mobs added before the catalyst scenarios would change their orbs."""
    s = []

    def add(name, ticks, lines, opts=''):
        s.append((name, ticks, opts, lines))

    sensor = f'block 0 {FLOOR} 0 {SENSOR}'
    watch = f'watch 0 {FLOOR} 0'
    add('mob_creeper_fuse', 25, [sensor, watch, 'player 6.5 -60 6.5', 'mob minecraft:creeper 2.5 -60 0.5 0 7 noai',
                                 'at 2 ignite m0'])
    add('mob_creeper_fuse_far', 25, [sensor, watch, 'player 6.5 -60 6.5', 'mob minecraft:creeper 9.5 -60 0.5 0 7 noai',
                                     'at 2 ignite m0'])
    add('mob_sheep_eat', 40, [sensor, watch, 'player 6.5 -60 6.5', 'mob minecraft:sheep 3.5 -60 0.5 90 11 noai',
                              'at 2 ate m0'])
    add('mob_calibrated_eat', 40, [f'block 0 {FLOOR} 0 {CALIBRATED}[facing=east]', watch, f'block 1 {FLOOR} 0 minecraft:redstone_block',
                                   'player 6.5 -60 6.5', 'mob minecraft:sheep 3.5 -60 0.5 90 11 noai', 'at 2 ate m0'])
    return s


def scenarios(count, seed):
    return handwritten() + random_scenarios(count, seed) + catalyst_scenarios(count // 3, seed) + mob_scenarios()


def write_scenarios(path, chosen):
    lines = []
    for i, (name, ticks, opts, body) in chosen:
        # One origin for all: the oracle resets the area between scenarios.
        ox, oz = 512, 0
        lines.append(f'scenario {name} ticks={ticks} {opts}'.rstrip())
        lines.append(f'origin {ox} {oz}')
        for line in body:
            f = line.split()
            if f[0] == 'player':
                f[1] = repr(float(f[1]) + ox)
                f[3] = repr(float(f[3]) + oz)
            elif f[0] == 'mob':
                f[2] = repr(float(f[2]) + ox)
                f[4] = repr(float(f[4]) + oz)
            elif f[0] == 'region':
                for k in (1, 4):
                    f[k] = str(int(f[k]) + ox)
                for k in (3, 6):
                    f[k] = str(int(f[k]) + oz)
            elif f[0] in ('block', 'watch'):
                f[1] = str(int(f[1]) + ox)
                f[3] = str(int(f[3]) + oz)
            elif f[0] == 'fill':
                for k in (1, 4):
                    f[k] = str(int(f[k]) + ox)
                for k in (3, 6):
                    f[k] = str(int(f[k]) + oz)
            elif f[0] == 'at' and f[2] == 'event':
                f[4] = repr(float(f[4]) + ox)
                f[6] = repr(float(f[6]) + oz)
            elif f[0] == 'at' and f[2] == 'set':
                f[3] = str(int(f[3]) + ox)
                f[5] = str(int(f[5]) + oz)
            lines.append(' '.join(f))
        lines.append('end')
    path.write_text('\n'.join(lines) + '\n')


def parse(path):
    """{scenario: {tick: [lines]}}"""
    result, cur = {}, None
    for line in path.read_text().splitlines():
        if line.startswith('== '):
            cur = result.setdefault(line[3:], {})
            continue
        t, _, rest = line.partition(' ')
        if rest.startswith('trace '):
            # SCULK_TRACE=1: the level random's callers, for reading only.
            continue
        cur.setdefault(int(t), []).append(rest)
    return result


def compare(vanilla, rust):
    report, clean = [], 0
    for name, v in vanilla.items():
        r = rust.get(name)
        if r is None:
            report.append(f'{name}: missing from Horizon')
            continue
        bad = None
        for t in sorted(set(v) | set(r)):
            a, b = v.get(t, []), r.get(t, [])
            if sorted(a) != sorted(b):
                bad = t
                report.append(f'{name}: first difference at tick {t}')
                for line in a:
                    if line not in b:
                        report.append(f'    vanilla {line}')
                for line in b:
                    if line not in a:
                        report.append(f'    horizon {line}')
                break
        if bad is None:
            clean += 1
            report.append(f'{name}: all {len(v)} ticks match')
    return report, clean


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--only', nargs='*')
    parser.add_argument('--count', type=int, default=300, help='random scenarios besides the handwritten ones')
    parser.add_argument('--seed', type=int, default=1)
    parser.add_argument('--java-only', action='store_true')
    parser.add_argument('--rust-only', action='store_true', help='reuse the last vanilla output')
    parser.add_argument('--report-only', action='store_true', help='print differences without failing')
    args = parser.parse_args()
    build = lab.LOCAL / 'sculk-parity'
    build.mkdir(parents=True, exist_ok=True)
    chosen = [(i, s) for i, s in enumerate(scenarios(args.count, args.seed)) if not args.only or any(o in s[0] for o in args.only)]
    inputs = build / 'scenarios.txt'
    write_scenarios(inputs, chosen)
    lock, jars, _ = base.verify_artifacts()
    resources = lab.LOCAL / 'horizon-client' / lock['minecraft']['version']
    if not args.rust_only:
        classes, dependencies, _ = oracle_harness.compile_oracle(build, 'VanillaSculkOracle', jars, baking.client_libraries(),
                                                                lock['minecraft']['java_major'])
        oracle_harness.run_logged(['java', '-Xmx3g', '-Djava.awt.headless=true', '-cp', str(classes) + os.pathsep + dependencies,
                                   'dev.horizonlab.parity.VanillaSculkOracle', inputs, build / 'vanilla.txt'],
                                  build, 'vanilla', timeout=3600)
    if not args.java_only:
        oracle_harness.run_logged(['cargo', 'build', '--locked', '--profile', 'fast', '-p', 'horizon-client', '--bin', 'horizon-client'],
                                  build, 'compile-rust')
        oracle_harness.run_logged([lab.cargo_target() / 'fast/horizon-client', '--resources', resources], build, 'horizon',
                                  env=lab.game_env(HORIZON_SCULK_FIXTURE=f'{inputs},{build / "horizon.txt"}'))
    if args.java_only:
        return
    names = {s[0] for _, s in chosen}
    vanilla = {k: v for k, v in parse(build / 'vanilla.txt').items() if k in names}
    report, clean = compare(vanilla, parse(build / 'horizon.txt'))
    (build / 'report.txt').write_text('\n'.join(report) + '\n')
    print('\n'.join(line for line in report if not line.endswith('ticks match')))
    print(f'{clean}/{len(vanilla)} scenarios match on every tick')
    if not args.report_only and clean != len(vanilla):
        raise SystemExit(1)


if __name__ == '__main__':
    main()
