#!/usr/bin/env python3
"""A flying projectile is pushed by the current of the water or lava it touches. Dedicated servers.

`Entity.baseTick` runs `updateFluidInteraction`, whose `EntityFluidInteraction.applyCurrentTo` adds the flow of the
water (scale 0.014) and of the lava (0.0023333, 0.007 where lava is fast) the box touches to the motion of every
entity `isPushedByFluid`. A thrown projectile (snowball) and a hurting one (wind charge) tick `baseTick` like any
entity, and nothing overrides `isPushedByFluid` for them, so a current carries them.

A vanilla dedicated server and Horizon's each load a copy of one flat world vanilla generated first. Ticking is
frozen. Each lane is a glass channel along x: a source block, then flowing water (level 1) or lava (level 2), whose
flow points +x, and a glass end. A projectile without motion (and, for the snowball and the arrow, without
gravity) waits in the flowing block. The arrow lane is the control (arrows already had the push). The servers are
stepped; after every step each projectile's exact Pos and Motion are read with `data get`. The transcripts must
match vanilla's (the oracle) line for line, and vanilla's must show every projectile moving +x.
"""
import re
import sys
import time

try:
    from . import test_locator_bar_parity as locator
    from .test_dedicated_server_parity import Server
except ImportError:
    import test_locator_bar_parity as locator
    from test_dedicated_server_parity import Server

NAME = 'projectile-fluid-current-parity'
TICKS = 3
STILL = 'Motion:[0.0d,0.0d,0.0d],'
# tag, the projectile, its data, its lane's z, the fluid
LANES = [('water_arrow', 'arrow', STILL + 'NoGravity:1b,pickup:0b,', 4, 'water'),
         ('water_snowball', 'snowball', STILL + 'NoGravity:1b,', 8, 'water'),
         ('water_wind_charge', 'wind_charge', STILL, 12, 'water'),
         ('lava_wind_charge', 'wind_charge', STILL, 16, 'lava')]
FORCELOAD = 'forceload add 0 0 31 31'
NUMBER = re.compile(r'(-?[0-9][0-9.E-]*)[df]')


def channel(z, fluid):
    """A channel on the floor at y -60 from x 2 to 6 along z: glass on both sides and at both ends, the fluid's
    source at x 3, its flowing block at x 4, and stone under both."""
    flowing = 'minecraft:water[level=1]' if fluid == 'water' else 'minecraft:lava[level=2]'
    return [f'fill 1 -61 {z - 1} 7 -59 {z + 1} minecraft:glass', f'fill 3 -61 {z} 5 -61 {z} minecraft:stone',
            f'fill 3 -60 {z} 5 -59 {z} minecraft:air', f'setblock 3 -60 {z} minecraft:{fluid}',
            f'setblock 4 -60 {z} {flowing}', f'setblock 5 -60 {z} minecraft:glass']


BLOCKS = ['time set 6000', *[line for *_rest, z, fluid in LANES for line in channel(z, fluid)]]
ENTITIES = [f'summon minecraft:{kind} 4.5 -59.8 {z}.5 {{{data}Tags:["{tag}"]}}' for tag, kind, data, z, _ in LANES]


def run(label, command, world, build):
    directory = build / label
    locator.server_dir(directory, world)
    server = Server(command, directory, label)
    lines = []
    try:
        server.wait_for(r'Done \(', timeout=600)
        server.console('tick freeze')
        server.console(FORCELOAD)
        server.console('say READY')
        server.wait_for(r'READY', timeout=120)
        time.sleep(3)
        for command_line in BLOCKS:
            server.console(command_line)
        time.sleep(1)
        for command_line in ENTITIES:
            server.console(command_line)
        time.sleep(1)
        for tick in range(TICKS + 1):
            if tick:
                server.console('tick step 1')
                time.sleep(1.0)
            start = len(server.lines)
            for tag, *_rest in LANES:
                for path in ('Pos', 'Motion'):
                    server.console(f'data get entity @e[tag={tag},limit=1] {path}')
            server.console(f'say END {tick}')
            server.wait_for(rf'END {tick}\b', timeout=120)
            answers = [l.split('INFO]: ', 1)[-1] for l in server.lines[start:]
                       if 'entity data:' in l or 'No entity was found' in l or 'Found no' in l]
            for i, (tag, *_rest) in enumerate(LANES):
                pos, motion = answers[2 * i:2 * i + 2] if len(answers) >= 2 * i + 2 else ('', '')
                if 'entity data:' not in pos:
                    lines.append(f'tick {tick} {tag} gone')
                else:
                    lines.append(f'tick {tick} {tag} pos {" ".join(NUMBER.findall(pos))}'
                                 f' motion {" ".join(NUMBER.findall(motion))}')
    finally:
        server.stop()
    return lines


def proves(vanilla):
    """Why vanilla's transcript does not show what this harness is for, or None."""
    for tag, *_rest in LANES:
        line = next(l for l in vanilla if l.startswith(f'tick {TICKS} {tag} '))
        if ' motion ' not in line:
            return f'{tag} is gone'
        if not float(line.split('motion ')[1].split()[0]) > 0.0:
            return f'the current did not push {tag}'
    return None


def main():
    lock, build, vanilla_server, horizon_server, _client = locator.sides(NAME)
    world = locator.generate_world(vanilla_server, build)
    vanilla = run('vanilla', vanilla_server, world, build)
    (build / 'vanilla.txt').write_text('\n'.join(vanilla) + '\n', encoding='utf-8')
    why = proves(vanilla)
    if why:
        raise AssertionError(f'vanilla is no proof: {why}:\n' + '\n'.join(vanilla))
    horizon = run('horizon', horizon_server, world, build)
    (build / 'horizon.txt').write_text('\n'.join(horizon) + '\n', encoding='utf-8')
    if vanilla != horizon:
        differ = [f'- {v}\n+ {h}' for v, h in zip(vanilla, horizon) if v != h]
        raise AssertionError(f'{len(differ)} lines differ from vanilla:\n' + '\n'.join(differ[:24]))
    print(f'projectile fluid current: {len(vanilla)} lines identical')


if __name__ == '__main__':
    try:
        main()
    except AssertionError as error:
        sys.exit(f'{NAME}: {error}')
