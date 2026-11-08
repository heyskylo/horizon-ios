#!/usr/bin/env python3
"""A mob in waterlogged scaffolding that hits a wall climbs, tick by tick on a dedicated server.

`LivingEntity.travelInWater` sets the vertical movement to 0.2 when the mob hit a wall and `onClimbable()` (a ladder
or scaffolding, the block it stands in; a spider's own climbing flag): a mob in waterlogged scaffolding moving into
a wall climbs out, where without the climbable block it would sink.

A vanilla dedicated server and Horizon's (`horizon-client --dedicated --nogui`) each load a copy of one flat world
vanilla generated first. Ticking is frozen, the cases are built (mobs with no movement speed, so no random goal
moves them; each in a sealed cell), and
the server steps one tick at a time; after each step every case mob's exact Pos and Motion are read with
`data get`. The transcripts must match vanilla's (the oracle) line for line, and vanilla's must show the climb.
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

NAME = 'mob-water-climb-parity'
TICKS = 6
# (tag, summon) for each case's mobs; the floor of the flat world is at y -61, so feet stand at -60.
MOB = 'minecraft:zombie'
# With their AI (a NoAI mob never travels, so a push never moves it) but no speed; each case is sealed in a glass
# cell one block wide, so a random stroll finds nowhere to go and never makes them jump (MoveControl). Zombies have
# no FloatGoal, and it is night, so none burns.
NBT = ('Silent:1b,PersistenceRequired:1b,IsBaby:0b,CanPickUpLoot:0b,'
       'attributes:[{id:"minecraft:movement_speed",base:0.0d}]')
SETUP = [
    'time set 18000',
    *(f'fill {x - 1} -61 3 {x + 1} -57 5 minecraft:glass hollow' for x in (10,)),
    # Waterlogged scaffolding against the cell's wall to the east, the mob moving into it: it climbs.
    'setblock 10 -60 4 minecraft:scaffolding[bottom=false,distance=0,waterlogged=true]',
    f'summon {MOB} 10.5 -60 4.5 {{{NBT},Tags:["climb"],Motion:[0.2d,0.0d,0.0d]}}',
]
TAGS = ['climb']
NUMBER = re.compile(r'(-?[0-9][0-9.E-]*)d')


def run(label, command, world, build):
    directory = build / label
    locator.server_dir(directory, world)
    server = Server(command, directory, label)
    lines = []
    try:
        server.wait_for(r'Done \(', timeout=600)
        server.console('tick freeze')
        server.console('forceload add 0 0 15 15')
        server.console('say READY')
        server.wait_for(r'READY', timeout=120)
        time.sleep(3)
        for command in SETUP:
            server.console(command)
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
            answers = [l.split('entity data:', 1)[1] for l in server.lines[start:] if 'entity data:' in l]
            if len(answers) != 2 * len(TAGS):
                raise AssertionError(f'{label}: tick {tick}: {len(answers)} answers, expected {2 * len(TAGS)}')
            for i, tag in enumerate(TAGS):
                pos, motion = (' '.join(NUMBER.findall(a)) for a in answers[2 * i:2 * i + 2])
                lines.append(f'tick {tick} {tag} pos {pos} motion {motion}')
    finally:
        server.stop()
    return lines


def values(lines, tick, tag):
    line = next(l for l in lines if l.startswith(f'tick {tick} {tag} '))
    parts = line.split()
    return [float(v) for v in parts[4:7]], [float(v) for v in parts[8:11]]


def proves(vanilla):
    """Why vanilla's transcript does not show what this harness is for, or None."""
    if not any(values(vanilla, t, 'climb')[0][1] > -60 + 1e-9 for t in range(1, TICKS + 1)):
        return 'the mob in waterlogged scaffolding did not climb'
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
    print(f'mob water climb: {len(vanilla)} lines identical')


if __name__ == '__main__':
    try:
        main()
    except AssertionError as error:
        sys.exit(f'{NAME}: {error}')
