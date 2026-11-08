#!/usr/bin/env python3
"""A young projectile does not hit an entity it only passes near, tick by tick on a dedicated server.

`ProjectileUtil.getHitResultOnMoveVector` grows the boxes of the entities on a projectile's way by
`computeMargin`: `max(0, min(0.3, (tickCount - 2) / 20))`, nothing for its first two ticks and 0.3 from its eighth.
A wind charge that passes 0.2 beside a mob on its second tick flies on; one that passes as close on its eighth hits.

A vanilla dedicated server and Horizon's (`horizon-client --dedicated --nogui`) each load a copy of one flat world
vanilla generated first. Ticking is frozen; two NoAI zombies stand in two lanes, and a wind charge (no owner, no
acceleration) flies along each lane one block a tick, 0.2 blocks beside its zombie's box: the first reaches it on its
second tick, the second on its eighth. After every step each charge's exact Pos and Motion (or its absence) are read with
`data get`. The transcripts must match vanilla's (the oracle) line for line, and vanilla's must show the young
charge flying past and the old one hitting.
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

NAME = 'projectile-hit-margin-parity'
TICKS = 12
ZOMBIE = 'NoAI:1b,Silent:1b,PersistenceRequired:1b,IsBaby:0b'
CHARGE = 'acceleration_power:0.0d,Motion:[0.0d,0.0d,1.0d]'
# Each zombie's box spans x 4.2..4.8 (or 14.2..14.8) and z 10.2..10.8; each charge flies along x = 5.0 (15.0),
# 0.2 beside it, at the zombie's chest height.
SETUP = [
    'time set 18000',
    f'summon minecraft:zombie 4.5 -60 10.5 {{{ZOMBIE},Tags:["young_mob"]}}',
    f'summon minecraft:wind_charge 5.0 -59 9.0 {{{CHARGE},Tags:["young"]}}',
    f'summon minecraft:zombie 14.5 -60 10.5 {{{ZOMBIE},Tags:["old_mob"]}}',
    f'summon minecraft:wind_charge 15.0 -59 2.0 {{{CHARGE},Tags:["old"]}}',
]
# The charges are compared; the zombies are only there to be hit (a wind burst's push is another harness's).
TAGS = ['young', 'old']
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
                gone = 'entity data:' not in pos
                lines.append(f'tick {tick} {tag} ' + ('gone' if gone else
                             f'pos {" ".join(NUMBER.findall(pos))} motion {" ".join(NUMBER.findall(motion))}'))
    finally:
        server.stop()
    return lines


def proves(vanilla):
    """Why vanilla's transcript does not show what this harness is for, or None."""
    young = [l for l in vanilla if ' young ' in l]
    old = [l for l in vanilla if ' old ' in l]
    if not any('pos' in l and float(l.split()[6]) > 11.0 for l in young):
        return 'the young charge did not fly past its zombie'
    if not any(l.endswith('gone') for l in old):
        return 'the old charge did not hit its zombie'
    return None


def main():
    lock, build, vanilla_server, horizon_server, _client = locator.sides(NAME)
    world = locator.generate_world(vanilla_server, build)
    vanilla = run('vanilla', vanilla_server, world, build)
    why = proves(vanilla)
    if why:
        raise AssertionError(f'vanilla is no proof: {why}:\n' + '\n'.join(vanilla))
    horizon = run('horizon', horizon_server, world, build)
    for label, lines in (('vanilla', vanilla), ('horizon', horizon)):
        (build / f'{label}.txt').write_text('\n'.join(lines) + '\n', encoding='utf-8')
    if vanilla != horizon:
        differ = [f'- {v}\n+ {h}' for v, h in zip(vanilla, horizon) if v != h]
        raise AssertionError(f'{len(differ)} lines differ from vanilla:\n' + '\n'.join(differ[:20]))
    print(f'projectile hit margin: {len(vanilla)} lines identical')


if __name__ == '__main__':
    try:
        main()
    except AssertionError as error:
        sys.exit(f'{NAME}: {error}')
