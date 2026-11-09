#!/usr/bin/env python3
"""A body a piston pushes stops at a boat in its way. Dedicated servers.

`Entity.move` collides with the blocks and also with the entities the mover can collide with
(`Entity.collide` → `getEntityCollisions`, `canCollideWith`): for every mover but a vehicle, the ones that are
`canBeCollidedWith` — a boat, a living shulker, a happy ghast held still. A piston's push (`MoverType.PISTON`) is
such a move, so a projectile or item it carries into a boat stops at the boat's side. The Orbital Skybender parks
its wind charges this way, against boats riding minecarts.

A vanilla dedicated server and Horizon's each load a copy of one flat world vanilla generated first. Ticking is
frozen. In two lanes a piston facing east extends its head through a snowball with no gravity and no motion; in one
lane an oak boat sits on the ground just past where the head's second push would carry the snowball. After every
step each snowball's exact Pos is read. The transcripts must match vanilla's (the oracle) line for line, and
vanilla's must show the snowball by the boat stopped short of the other one.
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

NAME = 'piston-push-boat-parity'
TICKS = 3
TAGS = ['by_boat', 'clear']


def lane(z, boat, tag):
    """A piston at (3, -60, z) facing east, the snowball in the block its head moves into, a boat beyond it."""
    return [
        f'setblock 3 -60 {z} minecraft:piston[facing=east]',
        f'summon minecraft:snowball 4.5 -59.8 {z}.5 {{Motion:[0.0d,0.0d,0.0d],NoGravity:1b,Tags:["{tag}"]}}',
        *([f'summon minecraft:oak_boat 5.9 -60.0 {z}.5 {{Rotation:[90.0f,0.0f]}}'] if boat else []),
    ]


SETUP = ['time set 6000', *lane(4, True, 'by_boat'), *lane(10, False, 'clear')]
POWER = ['setblock 2 -60 4 minecraft:redstone_block', 'setblock 2 -60 10 minecraft:redstone_block']
NUMBER = re.compile(r'(-?[0-9][0-9.E-]*)d')


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
                server.console(f'data get entity @e[tag={tag},limit=1] Pos')
            server.console(f'say END {tick}')
            server.wait_for(rf'END {tick}\b', timeout=120)
            answers = [l.split('INFO]: ', 1)[-1] for l in server.lines[start:]
                       if 'entity data:' in l or 'No entity was found' in l or 'Found no' in l]
            for i, tag in enumerate(TAGS):
                pos = answers[i] if len(answers) > i else ''
                lines.append(f'tick {tick} {tag} ' + (f'pos {" ".join(NUMBER.findall(pos))}'
                                                     if 'entity data:' in pos else 'gone'))
    finally:
        server.stop()
    return lines


def x_of(lines, tick, tag):
    line = next(l for l in lines if l.startswith(f'tick {tick} {tag} '))
    return float(line.split('pos ')[1].split()[0]) if ' pos ' in line else None


def proves(vanilla):
    """Why vanilla's transcript does not show what this harness is for, or None."""
    by_boat, clear = x_of(vanilla, TICKS, 'by_boat'), x_of(vanilla, TICKS, 'clear')
    if by_boat is None or clear is None:
        return 'a snowball is gone'
    if not clear > 5.1:
        return 'the piston did not push the snowball with no boat far enough'
    # The boat's side, 0.6875 from its centre, against the snowball's, 0.125 from its own.
    if not by_boat < clear - 0.03:
        return 'the boat did not stop the snowball'
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
    print(f'piston push boat: {len(vanilla)} lines identical')


if __name__ == '__main__':
    try:
        main()
    except AssertionError as error:
        sys.exit(f'{NAME}: {error}')
