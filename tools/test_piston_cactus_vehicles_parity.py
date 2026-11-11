#!/usr/bin/env python3
"""A cactus pricks a boat or a minecart a piston pushes. Dedicated servers.

`PistonMovingBlockEntity.moveEntityByPiston` moves the entity with `MoverType.PISTON`, and `Entity.move` ends in
`applyEffectsFromBlocks`: every block the box touches on the way (`checkInsideBlocks`, the box deflated by 1e-5) has
its `entityInside`. `CactusBlock.entityInside` hurts the entity by 1, and `VehicleEntity.hurtServer` adds 10 damage
to a boat or a minecart, which breaks over 40.

A vanilla dedicated server and Horizon's each load a copy of one flat world vanilla generated first. Ticking is
frozen; only chunk (10, 0) is forced, so chunk (11, 0) (x 176 and on) does not tick its entities: a vehicle there
never ticks, and its damage never wears off. Each lane is a piston at x 177 facing east, powered as the lane is
built, and a boat or a minecart beside its head. Its box reaches into the block of a cactus at x 179 without
touching the cactus's collision box; the control lane has no cactus. `damage` gives each vehicle 35 damage first,
so one prick breaks it. The servers are stepped; after every step each vehicle's exact Pos and Motion are read with
`data get`. The transcripts must match vanilla's (the oracle) line for line, and vanilla's must show the pricked
vehicles broken and the control kept.
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

NAME = 'piston-cactus-vehicles-parity'
TICKS = 3
# tag, the vehicle, its lane's z, its x and z offset, a cactus
LANES = [('boat', 'oak_boat', 2, 179.6, 1.65, True), ('minecart', 'minecart', 7, 179.4, 1.46, True),
         ('control_boat', 'oak_boat', 12, 179.6, 1.65, False)]
FORCELOAD = 'forceload add 160 0 175 15'
NUMBER = re.compile(r'(-?[0-9][0-9.E-]*)[df]')


def lane(z, cactus):
    """A cactus on sand at (179, -60, z), and a piston at (177, -60, z + 1) facing east, a redstone block behind it."""
    lines = [f'setblock 176 -60 {z + 1} minecraft:redstone_block', f'setblock 177 -60 {z + 1} minecraft:piston[facing=east]']
    if cactus:
        lines += [f'setblock 179 -61 {z} minecraft:sand', f'setblock 179 -60 {z} minecraft:cactus']
    return lines


BLOCKS = ['time set 6000', *[line for *_rest, z, _x, _dz, cactus in LANES for line in lane(z, cactus)]]
ENTITIES = [*[f'summon minecraft:{kind} {x} -60.0 {z + dz} {{Tags:["{tag}"]}}' for tag, kind, z, x, dz, _ in LANES],
            *[f'damage @e[tag={tag},limit=1] 3.5' for tag, *_rest in LANES]]


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
    for tag, *_rest, cactus in LANES:
        line = next(l for l in vanilla if l.startswith(f'tick 0 {tag} '))
        if line.endswith(' gone'):
            return f'{tag} broke before the push'
        line = next(l for l in vanilla if l.startswith(f'tick {TICKS} {tag} '))
        if line.endswith(' gone') != cactus:
            return f'{tag} was {"kept" if cactus else "broken"}'
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
    print(f'piston cactus vehicles: {len(vanilla)} lines identical')


if __name__ == '__main__':
    try:
        main()
    except AssertionError as error:
        sys.exit(f'{NAME}: {error}')
