#!/usr/bin/env python3
"""An arrow that bounces off a wind charge keeps its place in the tick order. Dedicated servers.

`ServerLevel` ticks its entities in the order they joined (`EntityTickList`); an arrow that hits an entity it can't
hurt bounces off (`AbstractArrow.onHitEntity`, `deflect(REVERSE)`) without ever leaving the level, so it still
ticks before every entity that joined after it. Whether a falling arrow meets a rising projectile in a tick depends
on that order: ticking first, it moves into where the other one still is. The Orbital Skybender's reading arrow
falls into each wind charge the cannon lifts this way.

A vanilla dedicated server and Horizon's each load a copy of one flat world vanilla generated first. Ticking is
frozen. Three projectiles are summoned in this order, in open air: an arrow falling at 0.2 a tick; wind charge C1
(no acceleration) sliding east, whose box top the arrow reaches on the first tick, so the arrow bounces off it and C1
slides away; and breeze wind charge C2 (no acceleration) rising at 0.5 a tick, whose box top lies in the arrow's path on
the third tick, but only before C2's own move. After every step the arrow's and C2's exact Pos and Motion are read.
The transcripts must match vanilla's (the oracle) line for line, and vanilla's must show the arrow bouncing off C2
on the third tick.
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

NAME = 'bounced-projectile-tick-order-parity'
TICKS = 5
TAGS = ['arrow', 'c2']
SETUP = [
    'time set 6000',
    'summon minecraft:arrow 4.5 -50.0 4.5 {Motion:[0.0d,-0.2d,0.0d],pickup:0b,Tags:["arrow"]}',
    'summon minecraft:breeze_wind_charge 4.5 -50.2625 4.5 {Motion:[0.6d,0.0d,0.0d],acceleration_power:0.0d,Tags:["c1"]}',
    'summon minecraft:breeze_wind_charge 4.5 -51.33 4.5 {Motion:[0.0d,0.5d,0.0d],acceleration_power:0.0d,Tags:["c2"]}',
]
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
    ys = [motion_y(vanilla, t, 'arrow') for t in range(TICKS + 1)]
    if None in ys:
        return 'the arrow is gone'
    if not ys[2] < 0:
        return 'the arrow was not falling again after its first bounce'
    # A bounce turns the fall to a tenth of it, upwards, before inertia and gravity.
    if not ys[3] > ys[2]:
        return 'the arrow did not bounce off the second wind charge on the third tick'
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
    print(f'bounced projectile tick order: {len(vanilla)} lines identical')


if __name__ == '__main__':
    try:
        main()
    except AssertionError as error:
        sys.exit(f'{NAME}: {error}')
