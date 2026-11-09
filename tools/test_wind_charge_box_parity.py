#!/usr/bin/env python3
"""A wind charge's box sits 0.15 below its position, and a piston pushes that box. Dedicated servers.

`AbstractWindCharge.makeBoundingBox` builds the type's 0.3125 cube from `y - 0.15F`, not from the feet like other
entities. A piston pushes what its moving block's swept box meets, by how far the entity's box reaches into it
(`PistonMovingBlockEntity.moveCollidedEntities`, `getMovement`): a stone lifted under a wind charge floating 0.1 over
it pushes it 0.51 on the first tick, where a box from the feet would get 0.41. A snowball (a box from its feet) in
the next lane is the control.

A vanilla dedicated server and Horizon's each load a copy of one flat world vanilla generated first. Ticking is
frozen. In two lanes an upward piston lifts a stone under a projectile with no motion, no gravity and (the charge)
no acceleration. The pistons are powered and the servers stepped; after every step each projectile's exact Pos and
Motion are read with `data get`. The transcripts must match vanilla's (the oracle) line for line, and vanilla's must
show the charge lifted more than the snowball on the first push.
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

NAME = 'wind-charge-box-parity'
TICKS = 5
TAGS = ['charge', 'snowball']
NUMBER = re.compile(r'(-?[0-9][0-9.E-]*)[df]')


def lane(x, entity, data, tag):
    """A piston in the floor at (x, -61, 4) under a stone, the projectile 0.1 over the stone."""
    return [
        f'setblock {x} -61 4 minecraft:piston[facing=up]',
        f'setblock {x} -60 4 minecraft:stone',
        f'summon minecraft:{entity} {x}.5 -58.9 4.5 {{Motion:[0.0d,0.0d,0.0d],{data}Tags:["{tag}"]}}',
    ]


SETUP = ['time set 6000', *lane(4, 'wind_charge', 'acceleration_power:0.0d,', 'charge'),
         *lane(10, 'snowball', 'NoGravity:1b,', 'snowball')]
POWER = ['setblock 3 -61 4 minecraft:redstone_block', 'setblock 9 -61 4 minecraft:redstone_block']


def run(label, command, world, build):
    directory = build / label
    locator.server_dir(directory, world)
    server = Server(command, directory, label)
    lines = []
    try:
        server.wait_for(r'Done \(', timeout=600)
        server.console('tick freeze')
        server.console('forceload add 0 0 31 31')
        server.console('say READY')
        server.wait_for(r'READY', timeout=120)
        time.sleep(3)
        for command_line in SETUP:
            server.console(command_line)
        time.sleep(1)
        for command_line in POWER:
            server.console(command_line)
        for tick in range(TICKS + 1):
            if tick:
                server.console('tick step 1')
                time.sleep(1.0)
            start = len(server.lines)
            for tag in TAGS:
                for path in ('Pos', 'Motion'):
                    server.console(f'data get entity @e[tag={tag},limit=1] {path}')
            server.console(f'say END {tick}')
            server.wait_for(rf'END {tick}\b', timeout=120)
            answers = [l.split('INFO]: ', 1)[-1] for l in server.lines[start:]
                       if 'entity data:' in l or 'No entity was found' in l or 'Found no' in l]
            for i, tag in enumerate(TAGS):
                pos, motion = answers[2 * i:2 * i + 2] if len(answers) >= 2 * i + 2 else ('', '')
                if 'entity data:' not in pos:
                    lines.append(f'tick {tick} {tag} gone')
                else:
                    lines.append(f'tick {tick} {tag} pos {" ".join(NUMBER.findall(pos))}'
                                 f' motion {" ".join(NUMBER.findall(motion))}')
    finally:
        server.stop()
    return lines


def height(lines, tick, tag):
    line = next(l for l in lines if l.startswith(f'tick {tick} {tag} '))
    return float(line.split('pos ')[1].split()[1]) if ' pos ' in line else None


def proves(vanilla):
    """Why vanilla's transcript does not show what this harness is for, or None."""
    heights = [[height(vanilla, t, tag) for t in (0, 1)] for tag in TAGS]
    if None in heights[0] + heights[1]:
        return 'a projectile is gone'
    (c0, c1), (s0, s1) = heights
    if not s1 - s0 > 0:
        return 'the snowball was not pushed'
    if not c1 - c0 > s1 - s0 + 0.1:
        return 'the charge was not lifted more than the snowball'
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
        raise AssertionError(f'{len(differ)} lines differ from vanilla:\n' + '\n'.join(differ[:20]))
    print(f'wind charge box: {len(vanilla)} lines identical')


if __name__ == '__main__':
    try:
        main()
    except AssertionError as error:
        sys.exit(f'{NAME}: {error}')
