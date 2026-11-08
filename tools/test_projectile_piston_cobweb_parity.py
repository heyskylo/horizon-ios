#!/usr/bin/env python3
"""A projectile a piston pushes through a cobweb is stuck in it, and the next push stops it. Dedicated servers.

`PistonMovingBlockEntity.moveEntityByPiston` moves an entity (`Entity.move(MoverType.PISTON, ...)`) and then applies
the effects of the blocks it went through (`applyEffectsFromBlocks`): a cobweb its box met makes it stuck
(`WebBlock.entityInside` → `makeStuckInBlock`). The next `Entity.move` while stuck sets its movement to zero
(`setDeltaMovement(Vec3.ZERO)`). A wind charge parked in a cobweb's reach and lifted by a piston, two pushes over the
piston's two ticks, keeps its position but loses its speed; without the cobweb it keeps its speed.

A vanilla dedicated server and Horizon's each load a copy of one flat world vanilla generated first. Ticking is
frozen. In two lanes an upward piston under a stone lifts a wind charge (no owner, no acceleration, drifting 0.02 a
tick along z) that rests on the stone, half over the next column; in one lane a cobweb fills that next column. The
pistons are powered and the servers stepped; after every step each charge's exact horizontal position and Motion are read with
`data get`. The transcripts must match vanilla's (the oracle) line for line, and vanilla's must show the charge by the
cobweb stopped and the other one not. The height is not compared: how far a piston lifts an entity is another
harness's (vanilla's second push here is 0.01 higher than Horizon's, by the cobweb or not).
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

NAME = 'projectile-piston-cobweb-parity'
TICKS = 6
CHARGE = 'acceleration_power:0.0d,Motion:[0.0d,0.0d,0.02d]'


def lane(x, cobweb, tag):
    """A piston in the floor at (x, -61, 4) under a stone, the charge on the stone over the column's east edge."""
    return [
        f'setblock {x} -61 4 minecraft:piston[facing=up]',
        f'setblock {x} -60 4 minecraft:stone',
        *([f'fill {x + 1} -59 4 {x + 1} -58 4 minecraft:cobweb'] if cobweb else []),
        f'summon minecraft:wind_charge {x + 0.9875} -59.0 4.5 {{{CHARGE},Tags:["{tag}"]}}',
    ]


SETUP = ['time set 6000', *lane(4, True, 'by_cobweb'), *lane(12, False, 'in_air')]
POWER = ['setblock 3 -61 4 minecraft:redstone_block', 'setblock 11 -61 4 minecraft:redstone_block']
TAGS = ['by_cobweb', 'in_air']
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
                if gone:
                    lines.append(f'tick {tick} {tag} gone')
                else:
                    x, _, z = NUMBER.findall(pos)
                    lines.append(f'tick {tick} {tag} x {x} z {z} motion {" ".join(NUMBER.findall(motion))}')
    finally:
        server.stop()
    return lines


def motion_of(lines, tag):
    last = [l for l in lines if f' {tag} ' in l][-1]
    return [float(v) for v in last.split('motion ')[1].split()] if 'motion' in last else None


def proves(vanilla):
    """Why vanilla's transcript does not show what this harness is for, or None."""
    by_cobweb, in_air = motion_of(vanilla, 'by_cobweb'), motion_of(vanilla, 'in_air')
    if by_cobweb is None or in_air is None:
        return 'a charge is gone'
    if any(by_cobweb):
        return 'the charge by the cobweb was not stopped'
    if not in_air[2]:
        return 'the charge without a cobweb was stopped too'
    return None


def main():
    lock, build, vanilla_server, horizon_server, _client = locator.sides(NAME)
    world = locator.generate_world(vanilla_server, build)
    vanilla = run('vanilla', vanilla_server, world, build)
    for label, lines in (('vanilla', vanilla),):
        (build / f'{label}.txt').write_text('\n'.join(lines) + '\n', encoding='utf-8')
    why = proves(vanilla)
    if why:
        raise AssertionError(f'vanilla is no proof: {why}:\n' + '\n'.join(vanilla))
    horizon = run('horizon', horizon_server, world, build)
    (build / 'horizon.txt').write_text('\n'.join(horizon) + '\n', encoding='utf-8')
    if vanilla != horizon:
        differ = [f'- {v}\n+ {h}' for v, h in zip(vanilla, horizon) if v != h]
        raise AssertionError(f'{len(differ)} lines differ from vanilla:\n' + '\n'.join(differ[:20]))
    print(f'projectile piston cobweb: {len(vanilla)} lines identical')


if __name__ == '__main__':
    try:
        main()
    except AssertionError as error:
        sys.exit(f'{NAME}: {error}')
