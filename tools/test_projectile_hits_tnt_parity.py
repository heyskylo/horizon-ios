#!/usr/bin/env python3
"""A projectile hits a primed TNT. Dedicated servers.

`PrimedTnt.isPickable` is true while it is not removed, so `ProjectileUtil.getEntityHitResult` finds a primed TNT in
a projectile's path: a wind charge bursts on it (and its burst pushes the TNT), a snowball breaks on it. An arrow
or a trident hurts it for nothing and turns back (`AbstractArrow.onHitEntity`'s `ProjectileDeflection.REVERSE`,
`ThrownTrident.onHitEntity`).

A vanilla dedicated server and Horizon's each load a copy of one flat world vanilla generated first. Ticking is
frozen. Each lane is a primed TNT on the floor and a projectile without gravity flying +z at it, which reaches its
box on the first tick. The servers are stepped; after every step the TNT's and the projectile's exact Pos and
Motion are read with `data get`. The transcripts must match vanilla's (the oracle) line for line, and vanilla's
must show the wind charge and the snowball gone, the arrow and the trident turned back, and the wind
charge's TNT pushed.
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

NAME = 'projectile-hits-tnt-parity'
TICKS = 3
# the projectile, its data, its lane's x
ARROW = 'NoGravity:1b,pickup:0b,'
LANES = [('wind_charge', '', 4), ('snowball', 'NoGravity:1b,', 8), ('arrow', ARROW, 12), ('trident', ARROW, 16)]
FORCELOAD = 'forceload add 0 0 31 31'
NUMBER = re.compile(r'(-?[0-9][0-9.E-]*)[df]')
TAGS = [(f'{kind}_tnt', f'{kind}_shot') for kind, _data, _x in LANES]
READ = [tag for pair in TAGS for tag in pair]
BLOCKS = ['time set 6000']
ENTITIES = [*[f'summon minecraft:tnt {x}.5 -60.0 8.5 {{fuse:200s,Motion:[0.0d,0.0d,0.0d],Tags:["{kind}_tnt"]}}'
              for kind, _data, x in LANES],
            *[f'summon minecraft:{kind} {x}.5 -59.6 7.6 {{{data}Motion:[0.0d,0.0d,0.5d],Tags:["{kind}_shot"]}}'
              for kind, data, x in LANES]]


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
            for tag in READ:
                for path in ('Pos', 'Motion'):
                    server.console(f'data get entity @e[tag={tag},limit=1] {path}')
            server.console(f'say END {tick}')
            server.wait_for(rf'END {tick}\b', timeout=120)
            answers = [l.split('INFO]: ', 1)[-1] for l in server.lines[start:]
                       if 'entity data:' in l or 'No entity was found' in l or 'Found no' in l]
            for i, tag in enumerate(READ):
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
    for kind, _data, _x in LANES:
        line = next(l for l in vanilla if l.startswith(f'tick 1 {kind}_shot '))
        if kind in ('wind_charge', 'snowball') and not line.endswith(' gone'):
            return f'the {kind} did not hit the TNT'
        if kind in ('arrow', 'trident') and not (' motion ' in line and float(line.split()[-1]) < 0.0):
            return f'the {kind} did not turn back from the TNT'
    pushed = next(l for l in vanilla if l.startswith('tick 1 wind_charge_tnt '))
    if pushed.endswith('motion 0.0 0.0 0.0'):
        return 'the burst did not push the TNT'
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
    print(f'projectile hits TNT: {len(vanilla)} lines identical')


if __name__ == '__main__':
    try:
        main()
    except AssertionError as error:
        sys.exit(f'{NAME}: {error}')
