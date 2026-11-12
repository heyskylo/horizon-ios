#!/usr/bin/env python3
"""A player's wind charge can't be turned in its first five ticks. Dedicated servers.

`WindCharge.deflect` refuses while `noDeflectTicks` (5, one less after each tick, not saved) lasts, so a projectile
that hits a young wind charge doesn't turn it (`Projectile.onRedirectProjectile`): it keeps its owner, its movement
and its acceleration power. An older one turns, and `AbstractHurtingProjectile.onDeflection` by an attack gives it
0.1 acceleration power. A breeze's wind charge has no such wait.

A vanilla dedicated server and Horizon's each load a copy of one flat world vanilla generated first. Ticking is
frozen. In one lane a wind charge with no acceleration, drifting 0.05 a tick east, ticks six times first; then a
second one is summoned in the next lane, and an arrow falling at 0.2 a tick is summoned over each, just above its
box. The arrows reach the charges on the next tick. After every step each charge's exact Pos and Motion are read.
The transcripts must match vanilla's (the oracle) line for line, and vanilla's must show the older charge speeding
up and the young one not.
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

NAME = 'wind-charge-no-deflect-ticks-parity'
OLD = 6
TICKS = 4
TAGS = ['young', 'old']
CHARGE = 'Motion:[0.05d,0.0d,0.0d],acceleration_power:0.0d'
ARROW = 'Motion:[0.0d,-0.2d,0.0d],pickup:0b'
FIRST = [f'summon minecraft:wind_charge 12.5 -50.2625 4.5 {{{CHARGE},Tags:["old"]}}']
THEN = [
    f'summon minecraft:wind_charge 4.5 -50.2625 4.5 {{{CHARGE},Tags:["young"]}}',
    f'summon minecraft:arrow 4.5 -50.0 4.5 {{{ARROW}}}',
    f'summon minecraft:arrow {12.5 + 0.05 * OLD} -50.0 4.5 {{{ARROW}}}',
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
        server.console('time set 6000')
        server.console('say READY')
        server.wait_for(r'READY', timeout=120)
        time.sleep(3)
        for command_line in FIRST:
            server.console(command_line)
        time.sleep(1)
        for _ in range(OLD):
            server.console('tick step 1')
            time.sleep(1.0)
        for command_line in THEN:
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


def motion_x(lines, tick, tag):
    line = next(l for l in lines if l.startswith(f'tick {tick} {tag} '))
    return float(line.split('motion ')[1].split()[0]) if ' motion ' in line else None


def proves(vanilla):
    """Why vanilla's transcript does not show what this harness is for, or None."""
    young, old = motion_x(vanilla, TICKS, 'young'), motion_x(vanilla, TICKS, 'old')
    if young is None or old is None:
        return 'a wind charge is gone'
    if not old > 0.1:
        return 'the older wind charge did not turn (no acceleration power)'
    if abs(young - 0.05) > 1e-9:
        return 'the young wind charge turned'
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
    print(f'wind charge no deflect ticks: {len(vanilla)} lines identical')


if __name__ == '__main__':
    try:
        main()
    except AssertionError as error:
        sys.exit(f'{NAME}: {error}')
