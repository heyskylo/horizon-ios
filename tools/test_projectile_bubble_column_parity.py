#!/usr/bin/env python3
"""A bubble column pushes the projectiles in it, unclamped. Dedicated servers.

`BubbleColumnBlock.entityInside` calls `Projectile.onInsideBubbleColumn` (0.06 up, or 0.03 down over magma) and,
at the column's top, `onAboveBubbleColumn` (0.1 up); a projectile's push is not clamped like other entities'. The
throwables (`ThrowableProjectile.tick`), the fireballs and wind charges (`AbstractHurtingProjectile.tick`) and
shulker bullets get it through the `applyEffectsFromBlocks` of their tick, a throwable also on its first tick
before gravity (`handleFirstTickBubbleColumn`).

A vanilla dedicated server and Horizon's each load a copy of one flat world vanilla generated first. Soul sand
under two pools of water four deep makes two upward bubble columns, left to form with ticking on. Ticking is then
frozen and a snowball with no gravity and a small fireball with no acceleration, both still, are summoned at the bottom
of one column each; the servers are stepped and after every step each projectile's exact Pos and Motion are read with
`data get`. The transcripts must match vanilla's (the oracle) line for line, and vanilla's must show both rising.
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

NAME = 'projectile-bubble-column-parity'
TICKS = 8
TAGS = ['snowball', 'fireball']
NUMBER = re.compile(r'(-?[0-9][0-9.E-]*)[df]')


def pool(x):
    """Soul sand at (x, -61, 4) under water up to -57, walled in glass."""
    return [
        f'fill {x - 1} -61 3 {x + 1} -56 5 minecraft:glass',
        f'setblock {x} -61 4 minecraft:soul_sand',
        f'fill {x} -60 4 {x} -57 4 minecraft:water',
    ]


SETUP = ['time set 6000', *pool(4), *pool(10)]
SUMMON = ['summon minecraft:snowball 4.5 -59.9 4.5 {Motion:[0.0d,0.0d,0.0d],NoGravity:1b,Tags:["snowball"]}',
          'summon minecraft:small_fireball 10.5 -59.9 4.5 '
          '{Motion:[0.0d,0.0d,0.0d],acceleration_power:0.0d,Tags:["fireball"]}']


def run(label, command, world, build):
    directory = build / label
    locator.server_dir(directory, world)
    server = Server(command, directory, label)
    lines = []
    try:
        server.wait_for(r'Done \(', timeout=600)
        server.console('forceload add 0 0 31 31')
        server.console('say READY')
        server.wait_for(r'READY', timeout=120)
        time.sleep(3)
        for command_line in SETUP:
            server.console(command_line)
        time.sleep(4)
        server.console('tick freeze')
        time.sleep(1)
        for command_line in SUMMON:
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
    for tag in TAGS:
        first, last = height(vanilla, 0, tag), height(vanilla, TICKS, tag)
        if first is None or last is None:
            return f'the {tag} is gone'
        if not last > first + 0.5:
            return f'the {tag} did not rise'
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
    print(f'projectile bubble column: {len(vanilla)} lines identical')


if __name__ == '__main__':
    try:
        main()
    except AssertionError as error:
        sys.exit(f'{NAME}: {error}')
