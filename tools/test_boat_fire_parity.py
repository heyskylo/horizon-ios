#!/usr/bin/env python3
"""Boat fire through the real Game tick against the pinned vanilla server.

`Entity.baseTick` burns a boat down a tick at a time and hurts it every 20; the two
`applyEffectsFromBlocks` of `AbstractBoat.tick` light it on fire blocks, hurt it once per
fire block and pass, and water puts it out. Each case summons one boat, steps the frozen
server a tick at a time (12 ticks) and records whether the boat is there, its `Fire` and the dropped
items. All other arguments, including --binary, are the command-oracle runner's.
"""
import sys

try:
    from . import command_exec_corpus as corpus
    from . import test_command_exec_parity as runner
except ImportError:
    import command_exec_corpus as corpus
    import test_command_exec_parity as runner

# name, kind, environment, saved fields, entity_drops, ridden by the player, damage dealt first
CASES = [
    ('air', 'oak_boat', None, '', True, False, 0),
    ('fire', 'oak_boat', 'fire', '', True, False, 0),
    ('soul_fire', 'oak_boat', 'soul_fire', '', True, False, 0),
    ('wide_fire', 'oak_boat', 'wide_fire', '', True, False, 0),
    ('invulnerable', 'oak_boat', 'fire', ',Invulnerable:1b', True, False, 0),
    ('burning_air', 'oak_boat', None, ',Fire:160s', True, False, 0),
    ('burning_water', 'oak_boat', 'water', ',Fire:160s', True, False, 0),
    ('cooldown_fire', 'oak_boat', 'fire', ',Fire:-3s', True, False, 0),
    ('no_drops', 'oak_boat', 'fire', '', False, False, 0),
    ('chest_fire', 'oak_chest_boat', 'fire', ',Items:[{Slot:0b,id:"minecraft:diamond",count:1}]', True, False, 0),
    ('ridden_burning', 'oak_boat', None, ',Fire:160s', True, True, 0),
    ('ridden_fire', 'oak_boat', 'fire', '', True, True, 0),
    # Leaving the water it started in puts it out.
    ('burning_water_exit', 'oak_boat', 'water', ',Fire:160s,Motion:[0d,2d,0d]', True, False, 0),
    # Out of the water and into the fire beside it in one move: put out, then lit.
    ('water_to_fire', 'oak_boat', 'water_fire', ',Motion:[2d,0d,0d]', False, False, 0),
    # Dropping from above the water's surface into it puts it out.
    ('falling_into_water', 'oak_boat', 'water_below', ',Fire:160s', True, False, 0),
    # 40 damage wears off before Fire 20 burns it: it survives at 40.
    ('ridden_damaged', 'oak_boat', None, ',Fire:29s', True, True, 4),
]
TICKS = 12


def program(cmd, setup):
    out = [cmd('/tick freeze', expect_result=1),
           cmd('/forceload add 0 0', expect_result=1),
           {'op': 'settle'},
           cmd('/execute if loaded 8 200 8', expect_result=1),
           cmd('/fill 0 199 0 15 199 15 soul_soil', expect_result=256)]
    for n, (name, kind, environment, fields, drops, ridden, hit) in enumerate(CASES, 1):
        uuid = f'00000200-0000-0000-0000-{n:012x}'
        out += setup(f'/gamerule entity_drops {str(drops).lower()}', '/fill 4 200 4 12 202 12 air',
                     '/kill @e[type=item,x=8.5,y=200,z=8.5,distance=..8]')
        if environment == 'wide_fire':
            out.append(cmd('/fill 7 200 7 9 200 9 fire', expect_result=9))
        elif environment == 'water_below':
            out.append(cmd('/setblock 8 200 8 water', expect_result=1))
        elif environment == 'water_fire':
            out += [cmd('/setblock 8 200 8 water', expect_result=1),
                    cmd('/setblock 10 200 8 fire', expect_result=1)]
        elif environment:
            out.append(cmd(f'/setblock 8 200 8 {environment}', expect_result=1))
        y = '200.9' if environment == 'water_below' else '200'
        out.append(cmd(f'/summon {kind} 8.5 {y} 8.5 {{UUID:[I;512,0,0,{n}],Rotation:[0f,0f]{fields}}}',
                       expect_result=1))
        if ridden:
            out.append(cmd(f'/ride @s mount {uuid}', expect_result=1))
        if hit:
            out.append(cmd(f'/damage {uuid} {hit} minecraft:generic', expect_result=1))
        for tick in range(TICKS):
            if tick:
                out += [cmd('/tick step 1', expect_result=1), {'op': 'tick', 'n': 1}]
            out += [cmd(f'/execute if entity {uuid}'),
                    cmd(f'/data get entity {uuid} Fire'),
                    # The generated world drops items of its own far away.
                    cmd('/execute positioned 8.5 200 8.5 if entity @e[type=item,distance=..4]')]
        if ridden:
            out += setup('/ride @s dismount', '/tp @s 8.5 230 8.5 0 0')
        out += setup(f'/kill {uuid}')
    return out


def main():
    if not any(a == '--work-dir' or a.startswith('--work-dir=') for a in sys.argv[1:]):
        sys.argv.extend(['--work-dir', str(runner.lab.LOCAL / 'boat-fire-parity')])
    runner.corpus_module.groups = lambda: [
        ('setup', corpus.group_setup()), ('boat fire', program(corpus.cmd, corpus.setup))]
    runner.main()


if __name__ == '__main__':
    main()
