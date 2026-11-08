#!/usr/bin/env python3
"""Mobs on a climbable block are not pushed, tick by tick on a dedicated server, from their first tick on.

`LivingEntity.isPushable` is false on a climbable block (`onClimbable()`, read live from the block the mob stands in),
so two mobs crowded in scaffolding stay where they are from the tick they appear, while the same two on plain ground
push apart (`Entity.push`).

A vanilla dedicated server and Horizon's (`horizon-client --dedicated --nogui`) each load a copy of one flat world
vanilla generated first. Ticking is frozen, the cases are built (mobs with no movement speed, so no random goal
moves them; each in a sealed cell), and
the server steps one tick at a time; after each step every case mob's exact Pos and Motion are read with
`data get`. The transcripts must match vanilla's (the oracle) line for line, and vanilla's must show the push on
plain ground and none in scaffolding.
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

NAME = 'mob-climbable-push-parity'
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
    *(f'fill {x - 1} -61 3 {x + 1} -57 5 minecraft:glass hollow' for x in (2, 6)),
    # Plain ground: two mobs 0.05 apart push each other apart.
    f'summon {MOB} 2.5 -60 4.5 {{{NBT},Tags:["ground_a"]}}',
    f'summon {MOB} 2.5 -60 4.55 {{{NBT},Tags:["ground_b"]}}',
    # The same pair inside scaffolding: neither is pushable.
    'setblock 6 -60 4 minecraft:scaffolding[bottom=false,distance=0]',
    f'summon {MOB} 6.5 -60 4.5 {{{NBT},Tags:["scaffold_a"]}}',
    f'summon {MOB} 6.5 -60 4.55 {{{NBT},Tags:["scaffold_b"]}}',
]
TAGS = ['ground_a', 'ground_b', 'scaffold_a', 'scaffold_b']
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
    (ground_a, _), (ground_b, _) = values(vanilla, TICKS, 'ground_a'), values(vanilla, TICKS, 'ground_b')
    if abs(ground_b[2] - ground_a[2]) <= 0.0501:
        return 'the mobs on plain ground did not push apart'
    for tag, z in (('scaffold_a', 4.5), ('scaffold_b', 4.55)):
        if abs(values(vanilla, TICKS, tag)[0][2] - z) > 1e-9:
            return f'{tag} moved in scaffolding'
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
    print(f'mob climbable push: {len(vanilla)} lines identical')


if __name__ == '__main__':
    try:
        main()
    except AssertionError as error:
        sys.exit(f'{NAME}: {error}')
