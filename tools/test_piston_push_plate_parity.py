#!/usr/bin/env python3
"""A piston's push presses the pressure plate it carries an entity onto, during the push. Dedicated servers.

`PistonMovingBlockEntity.moveEntityByPiston` moves an entity and then applies the effects of the blocks it went
through (`Entity.applyEffectsFromBlocks`): a pressure plate the moved box now touches is pressed right then
(`BasePressurePlateBlock.entityInside` → `checkPressed`), and powers its neighbours before the piston pushes anything
else. A redstone machine that lets one of several stacked entities through this way (the plate opens a trapdoor over
the others) depends on it.

A vanilla dedicated server and Horizon's each load a copy of one flat world vanilla generated first. Ticking is
frozen. An upward piston in the floor lifts a stone under a snowball with no gravity and no motion, floating over
the stone's east edge. Its second push carries the snowball's box onto a light weighted pressure plate standing on a
cobblestone wall in the next column; an iron trapdoor stands beside the plate. The piston is powered and the server
stepped; after every step the snowball's exact height, the plate's power and whether the trapdoor is open are
read. The transcripts must match vanilla's (the oracle) line for line, and vanilla's must show the plate pressed on
the tick of the push that carried the snowball onto it.
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

NAME = 'piston-push-plate-parity'
TICKS = 4
SETUP = [
    'time set 6000',
    'setblock 4 -61 4 minecraft:piston[facing=up]',
    'setblock 4 -60 4 minecraft:stone',
    'setblock 5 -59 4 minecraft:cobblestone_wall',
    'setblock 5 -58 4 minecraft:light_weighted_pressure_plate',
    'setblock 6 -58 4 minecraft:iron_trapdoor[half=bottom,facing=east]',
    'summon minecraft:snowball 4.95 -58.6 4.85 {Motion:[0.0d,0.0d,0.0d],NoGravity:1b,Tags:["ball"]}',
]
POWER = ['setblock 3 -61 4 minecraft:redstone_block']
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
        for command_line in POWER:
            server.console(command_line)
        for tick in range(TICKS + 1):
            if tick:
                server.console('tick step 1')
                time.sleep(1.0)
            start = len(server.lines)
            server.console('data get entity @e[tag=ball,limit=1] Pos')
            for power in range(4):
                server.console(f'execute if block 5 -58 4 minecraft:light_weighted_pressure_plate[power={power}]')
            server.console('execute if block 6 -58 4 minecraft:iron_trapdoor[open=true]')
            server.console(f'say END {tick}')
            server.wait_for(rf'END {tick}\b', timeout=120)
            answers = [l.split('INFO]: ', 1)[-1] for l in server.lines[start:]
                       if 'entity data:' in l or 'No entity was found' in l or 'Test ' in l]
            pos = answers[0] if answers else ''
            height = NUMBER.findall(pos)[1] if 'entity data:' in pos else 'gone'
            tests = ['passed' in a for a in answers[1:]]
            power = next((p for p, ok in enumerate(tests[:4]) if ok), 'other')
            lines.append(f'tick {tick} snowball y {height} plate power {power} '
                         f'trapdoor {"open" if len(tests) > 4 and tests[4] else "closed"}')
    finally:
        server.stop()
    return lines


def proves(vanilla):
    """Why vanilla's transcript does not show what this harness is for, or None."""
    if any('gone' in line for line in vanilla):
        return 'the snowball is gone'
    pressed = [int(l.split()[1]) for l in vanilla if 'plate power 0 ' not in l]
    if not pressed:
        return 'the plate was never pressed'
    first = pressed[0]
    heights = [float(l.split()[4]) for l in vanilla]
    if heights[first] == heights[first - 1]:
        return 'the plate was not pressed on the tick of a push'
    if 'trapdoor open' not in vanilla[first]:
        return 'the trapdoor did not open with the plate'
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
    print(f'piston push plate: {len(vanilla)} lines identical')


if __name__ == '__main__':
    try:
        main()
    except AssertionError as error:
        sys.exit(f'{NAME}: {error}')
