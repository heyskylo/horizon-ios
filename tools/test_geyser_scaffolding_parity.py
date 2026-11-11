#!/usr/bin/env python3
"""A geyser goes through scaffolding seen from below. Dedicated servers.

`PotentSulfurBlockEntity` reads the column over a potent sulfur block with
`CollisionContext.positionContext(<the height under the column>)`: `findNoxiousGasSourceBlock` and
`getUnobstructedBlockCount` pass a block whose collision shape is empty for that context. Scaffolding has its full
top only for what stands above it (`ScaffoldingBlock.getCollisionShape`, `isAbove`), so from the sulfur's height it
is empty: a waterlogged scaffolding holds the geyser's water, and the geyser launches what is over the scaffolding.

A vanilla dedicated server and Horizon's each load a copy of one flat world vanilla generated first. Ticking is
frozen. Each lane is a continuous geyser (lava, potent sulfur) with an item or a primed TNT without gravity or
motion three blocks over the sulfur. In the control lane the geyser's water is a water block; in the others it is a
waterlogged scaffolding with a dry scaffolding on it. The servers are stepped; after every step each entity's exact
Pos and Motion are read with `data get`. The transcripts must match vanilla's (the oracle) line for line, and
vanilla's must show every one of them rising.
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

NAME = 'geyser-scaffolding-parity'
TICKS = 4
STILL = 'Motion:[0.0d,0.0d,0.0d],NoGravity:1b,'
ITEM = STILL + 'Item:{id:"minecraft:stone",count:1},'
# tag, the entity, its data, its lane's x, scaffolding in the column
LANES = [('water_item', 'item', ITEM, 4, False), ('scaffolding_item', 'item', ITEM, 12, True),
         ('scaffolding_tnt', 'tnt', STILL + 'fuse:200s,', 20, True)]
NUMBER = re.compile(r'(-?[0-9][0-9.E-]*)[df]')


def geyser(x, scaffolding):
    """Lava in the floor at (x, -61, 8), the sulfur on it, its water between four glass blocks: a water block, or
    a waterlogged scaffolding under a dry one."""
    glass = [f'setblock {x + dx} -59 {8 + dz} minecraft:glass' for dx, dz in ((1, 0), (-1, 0), (0, 1), (0, -1))]
    water = ([f'setblock {x} -59 8 minecraft:scaffolding[waterlogged=true]', f'setblock {x} -58 8 minecraft:scaffolding']
             if scaffolding else [f'setblock {x} -59 8 minecraft:water'])
    return [f'setblock {x} -61 8 minecraft:lava', f'setblock {x} -60 8 minecraft:stone', *glass, *water,
            f'setblock {x} -60 8 minecraft:potent_sulfur[potent_sulfur_state=continuous]']


BLOCKS = ['time set 6000', *[line for *_rest, x, scaffolding in LANES for line in geyser(x, scaffolding)]]
ENTITIES = [f'summon minecraft:{kind} {x}.5 -57.0 8.5 {{{data}Tags:["{tag}"]}}' for tag, kind, data, x, _ in LANES]


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
            for tag, *_rest in LANES:
                for path in ('Pos', 'Motion'):
                    server.console(f'data get entity @e[tag={tag},limit=1] {path}')
            server.console(f'say END {tick}')
            server.wait_for(rf'END {tick}\b', timeout=120)
            answers = [l.split('INFO]: ', 1)[-1] for l in server.lines[start:]
                       if 'entity data:' in l or 'No entity was found' in l or 'Found no' in l]
            for i, (tag, *_rest) in enumerate(LANES):
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
    for tag, *_rest in LANES:
        heights = []
        for tick in (0, TICKS):
            line = next(l for l in vanilla if l.startswith(f'tick {tick} {tag} '))
            if ' pos ' not in line:
                return f'{tag} is gone'
            heights.append(float(line.split('pos ')[1].split()[1]))
        if not heights[1] > heights[0] + 0.5:
            return f'{tag} was not launched'
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
    print(f'geyser through scaffolding: {len(vanilla)} lines identical')


if __name__ == '__main__':
    try:
        main()
    except AssertionError as error:
        sys.exit(f'{NAME}: {error}')
