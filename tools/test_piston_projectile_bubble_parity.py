#!/usr/bin/env python3
"""A piston's push gives a projectile the push of the bubble column it moved it through. Dedicated servers.

`PistonMovingBlockEntity.moveEntityByPiston` moves an entity and then applies the effects of the blocks it went
through (`Entity.applyEffectsFromBlocks`), bubble columns among them: a projectile there gets
`Projectile.onInsideBubbleColumn` (0.06 up), as it does after its own move. A wind charge a piston lifts through a
bubble column climbs it this way (the Orbital Skybender's reading column).

A vanilla dedicated server and Horizon's each load a copy of one flat world vanilla generated first. Ticking is
frozen. An upward piston in the floor lifts a stone under a snowball with no gravity and no motion, floating over the
stone's east edge; the snowball's box reaches into a one-block bubble column (soul sand under it, glass around and
over it) in the next column. A second lane has no bubble column. The pistons are powered and the servers stepped;
after every step each snowball's exact Pos and Motion are read. The transcripts must match vanilla's (the oracle)
line for line, and vanilla's must show the snowball by the column rising faster than its own tick's push alone.
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

NAME = 'piston-projectile-bubble-parity'
TICKS = 3
TAGS = ['by_column', 'in_air']
NUMBER = re.compile(r'(-?[0-9][0-9.E-]*)d')


def lane(x, column, tag):
    """A piston in the floor at (x, -61, 4) under a stone, the snowball over its east edge, a column at x + 1."""
    walls = [
        f'fill {x + 1} -60 3 {x + 2} -58 5 minecraft:glass',
        f'setblock {x + 1} -60 4 minecraft:soul_sand',
        f'setblock {x + 1} -59 4 minecraft:bubble_column[drag=false]' if column else f'setblock {x + 1} -59 4 minecraft:air',
    ]
    return [
        f'setblock {x} -61 4 minecraft:piston[facing=up]',
        f'setblock {x} -60 4 minecraft:stone',
        *walls,
        f'summon minecraft:snowball {x}.95 -58.9 4.5 {{Motion:[0.0d,0.0d,0.0d],NoGravity:1b,Tags:["{tag}"]}}',
    ]


SETUP = ['time set 6000', *lane(4, True, 'by_column'), *lane(12, False, 'in_air')]
POWER = ['setblock 3 -61 4 minecraft:redstone_block', 'setblock 11 -61 4 minecraft:redstone_block']


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


def motion_y(lines, tick, tag):
    line = next(l for l in lines if l.startswith(f'tick {tick} {tag} '))
    return float(line.split('motion ')[1].split()[1]) if ' motion ' in line else None


def proves(vanilla):
    """Why vanilla's transcript does not show what this harness is for, or None."""
    column, air = motion_y(vanilla, 1, 'by_column'), motion_y(vanilla, 1, 'in_air')
    if column is None or air is None:
        return 'a snowball is gone'
    if air != 0:
        return 'the snowball with no column moved on its own'
    # Its own tick's push alone gives 0.06 (less the water's drag); the piston's push adds another.
    if not column > 0.07:
        return 'the snowball by the column got no push from the piston\'s move'
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
    print(f'piston projectile bubble: {len(vanilla)} lines identical')


if __name__ == '__main__':
    try:
        main()
    except AssertionError as error:
        sys.exit(f'{NAME}: {error}')
