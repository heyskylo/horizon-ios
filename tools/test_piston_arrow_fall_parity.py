#!/usr/bin/env python3
"""A stuck arrow a piston pushes away from what holds it starts falling right after the push. Dedicated servers.

`AbstractArrow.move`: after any move but its own (`MoverType.PISTON`, `SHULKER_BOX`), an arrow in the ground with
nothing to collide with within 0.06 of its position starts falling (`shouldFall`, `startFalling`), whether or not
the move went anywhere. Its own tick only releases it once the block at its position changes, a tick later. A
machine that lifts or slides an arrow and lets it drop onto something on the next tick (the Orbital Skybender's
switch arrow) depends on the push's release.

A vanilla dedicated server and Horizon's each load a copy of one flat world vanilla generated first. Ticking is
frozen. An arrow with no owner is dropped onto a stone and sticks on its top. A piston beside it then extends its
head through the arrow's block, pushing the arrow east off the stone, over air. After every step the arrow's exact
Pos, Motion and inGround are read. The transcripts must match vanilla's (the oracle) line for line, and vanilla's
must show the arrow out of the ground on the tick of the push that carried it off the stone.
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

NAME = 'piston-arrow-fall-parity'
LAND = 2
TICKS = 5
SETUP = [
    'time set 6000',
    'setblock 4 -60 4 minecraft:stone',
    'setblock 3 -59 4 minecraft:piston[facing=east]',
    'summon minecraft:arrow 4.5 -58.4 4.5 {Motion:[0.0d,-1.0d,0.0d],pickup:0b,Tags:["shot"]}',
]
POWER = ['setblock 2 -59 4 minecraft:redstone_block']
NUMBER = re.compile(r'(-?[0-9][0-9.E-]*)[db]')


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
        for _ in range(LAND):
            server.console('tick step 1')
            time.sleep(1.0)
        for command_line in POWER:
            server.console(command_line)
        for tick in range(TICKS + 1):
            if tick:
                server.console('tick step 1')
                time.sleep(1.0)
            start = len(server.lines)
            for path in ('Pos', 'Motion', 'inGround'):
                server.console(f'data get entity @e[tag=shot,limit=1] {path}')
            server.console(f'say END {tick}')
            server.wait_for(rf'END {tick}\b', timeout=120)
            answers = [l.split('INFO]: ', 1)[-1] for l in server.lines[start:]
                       if 'entity data:' in l or 'No entity was found' in l or 'Found no' in l]
            if len(answers) < 3 or any('entity data:' not in a for a in answers[:3]):
                lines.append(f'tick {tick} arrow gone')
                continue
            pos, motion, ground = (NUMBER.findall(a) for a in answers[:3])
            lines.append(f'tick {tick} arrow pos {" ".join(pos)} motion {" ".join(motion)} in_ground {ground[0]}')
    finally:
        server.stop()
    return lines


def proves(vanilla):
    """Why vanilla's transcript does not show what this harness is for, or None."""
    if any('gone' in line for line in vanilla):
        return 'the arrow is gone'
    if not vanilla[0].endswith('in_ground 1'):
        return 'the arrow did not stick on the stone'
    xs = [float(l.split('pos ')[1].split()[0]) for l in vanilla]
    off = next((t for t, x in enumerate(xs) if x - 0.25 > 5.0), None)
    if off is None:
        return 'the piston did not push the arrow off the stone'
    if not vanilla[off].endswith('in_ground 0'):
        return 'the arrow was still in the ground after the push that carried it off'
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
    print(f'piston arrow fall: {len(vanilla)} lines identical')


if __name__ == '__main__':
    try:
        main()
    except AssertionError as error:
        sys.exit(f'{NAME}: {error}')
