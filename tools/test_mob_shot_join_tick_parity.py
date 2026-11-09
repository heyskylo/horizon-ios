#!/usr/bin/env python3
"""A mob's shot first moves on the tick after it is fired; a dispenser's on the tick it is fired. Dedicated servers.

`ServerLevel.tick` ticks its entities with `EntityTickList.forEach`, which walks the list as it was when the pass
began (`ensureActiveIsNotIterated`): an entity that joins during the pass, such as a breeze's wind charge
(`Shoot.tick` in the breeze's own tick), first ticks on the next tick and is seen where it was fired. One that joins
before the pass, such as a dispenser's (`DispenserBlock.dispenseFrom` in the block ticks), ticks in the same tick.

A vanilla dedicated server and Horizon's each load a copy of one flat world vanilla generated first. Ticking is
frozen and stepped. A breeze shoots at a NoAI iron golem three blocks away; a dispenser, powered once, fires a wind
charge into open air. After every step the wind charges and the breeze are read with `data get`. For the
breeze's first shot (fired before the breeze has moved) and for the dispenser's, the harness records whether, on the first tick it is seen, the
charge is still where it was fired: at its breeze (horizontally), or within the dispenser's front half block. The
spread both draw from their random is not compared. vanilla's transcript is the oracle.
"""
import math
import re
import sys
import time

try:
    from . import test_locator_bar_parity as locator
    from .test_dedicated_server_parity import Server
except ImportError:
    import test_locator_bar_parity as locator
    from test_dedicated_server_parity import Server

NAME = 'mob-shot-join-tick-parity'
MAX_TICKS = 300
# Only the first shot: the breeze has not moved yet, so where it stands after the tick is where it fired from
# (later ones may be fired from a jump, which moves it within the same tick).
SHOTS = 1
SETUP = [
    'time set 6000',
    'summon minecraft:breeze 2.5 -60 4.5 {PersistenceRequired:1b,Tags:["shooter"],'
    'attributes:[{id:"minecraft:movement_speed",base:0.0d}]}',
    'summon minecraft:iron_golem 5.5 -60 4.5 {NoAI:1b,PersistenceRequired:1b}',
    # The dispenser, 40 blocks away, faces east into open air.
    'setblock 40 -60 4 minecraft:dispenser[facing=east]{Items:[{Slot:0b,id:"minecraft:wind_charge",count:1}]}',
]
NUMBER = re.compile(r'(-?[0-9][0-9.E-]*)d')


def numbers(line):
    return [float(v) for v in NUMBER.findall(line.split('entity data:', 1)[1])]


def run(label, command, world, build):
    directory = build / label
    locator.server_dir(directory, world)
    server = Server(command, directory, label)
    lines = []
    try:
        server.wait_for(r'Done \(', timeout=600)
        server.console('tick freeze')
        server.console('forceload add 0 0 47 15')
        server.console('say READY')
        server.wait_for(r'READY', timeout=120)
        time.sleep(3)
        for command_line in SETUP:
            server.console(command_line)
        seen_shots = 0
        dispensed = None
        known = set()
        for tick in range(1, MAX_TICKS + 1):
            if tick == 20:
                server.console('setblock 39 -60 4 minecraft:redstone_block')
            server.console('tick step 1')
            time.sleep(0.15)
            start = len(server.lines)
            server.console('data get entity @e[tag=shooter,limit=1] Pos')
            server.console('execute as @e[type=minecraft:breeze_wind_charge] run data get entity @s Pos')
            server.console('execute as @e[type=minecraft:wind_charge] run data get entity @s Pos')
            server.console(f'say END {tick}')
            server.wait_for(rf'END {tick}\b', timeout=120)
            answers = [l for l in server.lines[start:] if 'entity data:' in l]
            breeze = [numbers(l) for l in answers if 'Breeze has' in l]
            charges = [numbers(l) for l in answers if 'Wind Charge has' in l]
            if breeze:
                bx, _, bz = breeze[0]
                breeze_charges = [c for c in charges if c[0] < 20]
                fresh = [c for c in breeze_charges if tuple(round(v, 6) for v in c) not in known]
                known = {tuple(round(v, 6) for v in c) for c in breeze_charges}
                if fresh and seen_shots < SHOTS:
                    near = min(fresh, key=lambda c: math.hypot(c[0] - bx, c[2] - bz))
                    seen_shots += 1
                    at_breeze = math.hypot(near[0] - bx, near[2] - bz) < 0.2
                    lines.append(f'breeze shot {seen_shots}: first seen at its breeze: {at_breeze}')
            far = [c for c in charges if c[0] > 30]
            if far and dispensed is None:
                dispensed = far[0]
                in_front = far[0][0] < 41.0
                lines.append(f'dispenser shot: first seen in front of the dispenser: {in_front}')
            if seen_shots >= SHOTS and dispensed is not None:
                break
    finally:
        server.stop()
    # The breeze's shots and the dispenser's need not come in the same order on both servers.
    return sorted(lines)


def proves(vanilla):
    if sum(l.startswith('breeze shot') for l in vanilla) < SHOTS:
        return f'the breeze fired fewer than {SHOTS} shots'
    if not any(l.startswith('dispenser shot') for l in vanilla):
        return 'the dispenser did not fire'
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
        raise AssertionError('differs from vanilla:\nvanilla:\n' + '\n'.join(vanilla) + '\nhorizon:\n' + '\n'.join(horizon))
    print(f'mob shot join tick: {len(vanilla)} lines identical')


if __name__ == '__main__':
    try:
        main()
    except AssertionError as error:
        sys.exit(f'{NAME}: {error}')
