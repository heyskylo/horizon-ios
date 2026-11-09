#!/usr/bin/env python3
"""A wind charge's hit damages a boat or a minecart, and the fifth hit in a tick breaks it. Dedicated servers.

`AbstractWindCharge.onHitEntity` hurts what it hits by one point before it bursts. A boat or a minecart takes that
through `VehicleEntity.hurtServer`: each point adds 10 to its damage, and past 40 it breaks and drops its item. The
damage wears off by one a tick, so four charges landing in the same tick leave the vehicle whole and five break it.

A vanilla dedicated server and Horizon's each load a copy of one flat world vanilla generated first. Ticking is
frozen. Four lanes hold a boat or a minecart on the floor with four or five breeze wind charges just over it, moving
down. The servers are stepped; after every step each lane's vehicle (there or gone), its item drops and its
remaining charges are read. The transcripts must match vanilla's (the oracle) line for line, and vanilla's must show
the four-charge vehicles left and the five-charge ones broken into their items.
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

NAME = 'wind-charge-vehicle-parity'
TICKS = 3
# tag, the vehicle, its x, how many charges land on it in the first tick
LANES = [('boat4', 'oak_boat', 4, 4), ('boat5', 'oak_boat', 20, 5),
         ('cart4', 'minecart', 36, 4), ('cart5', 'minecart', 52, 5)]
COUNT = re.compile(r'Test passed\. Count: (\d+)|Test failed')


def lane(tag, vehicle, x, charges):
    """The vehicle on the floor at (x.5, -60, 8.5), the charges 0.8 over the floor and falling half a block a tick."""
    charge = (f'summon minecraft:breeze_wind_charge {x}.5 -59.2 8.5 '
              '{Motion:[0.0d,-0.5d,0.0d],acceleration_power:0.0d}')
    return [f'summon minecraft:{vehicle} {x}.5 -60 8.5 {{Tags:["{tag}"]}}', *[charge] * charges]


SETUP = ['time set 6000', *[line for spec in LANES for line in lane(*spec)]]


def near(x, kind):
    return f'@e[type=minecraft:{kind},x={x}.5,y=-60,z=8.5,distance=..7]'


def run(label, command, world, build):
    directory = build / label
    locator.server_dir(directory, world)
    server = Server(command, directory, label)
    lines = []
    try:
        server.wait_for(r'Done \(', timeout=600)
        server.console('tick freeze')
        server.console('forceload add 0 0 63 31')
        server.console('say READY')
        server.wait_for(r'READY', timeout=120)
        time.sleep(3)
        for command_line in SETUP:
            server.console(command_line)
        time.sleep(1)
        for tick in range(TICKS + 1):
            if tick:
                server.console('tick step 1')
                time.sleep(1.0)
            start = len(server.lines)
            for tag, _vehicle, x, _charges in LANES:
                server.console(f'execute if entity @e[tag={tag}]')
                server.console(f'execute if entity {near(x, "item")}')
                server.console(f'execute if entity {near(x, "breeze_wind_charge")}')
            server.console(f'say END {tick}')
            server.wait_for(rf'END {tick}\b', timeout=120)
            counts = [int(m.group(1) or 0) for l in server.lines[start:] if (m := COUNT.search(l))]
            if len(counts) != 3 * len(LANES):
                raise AssertionError(f'{label}: {len(counts)} answers at tick {tick}')
            for i, (tag, *_rest) in enumerate(LANES):
                there, items, charges = counts[3 * i:3 * i + 3]
                lines.append(f'tick {tick} {tag} vehicle {there} items {items} charges {charges}')
    finally:
        server.stop()
    return lines


def proves(vanilla):
    """Why vanilla's transcript does not show what this harness is for, or None."""
    state = {l.split()[2]: l.split()[3:] for l in vanilla if l.startswith(f'tick {TICKS} ')}
    for tag, _vehicle, _x, charges in LANES:
        there, items = int(state[tag][1]), int(state[tag][3])
        if charges == 4 and (there, items) != (1, 0):
            return f'{tag} did not survive four charges'
        if charges == 5 and (there, items) != (0, 1):
            return f'{tag} was not broken into its item by five charges'
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
    print(f'wind charge on vehicles: {len(vanilla)} lines identical')


if __name__ == '__main__':
    try:
        main()
    except AssertionError as error:
        sys.exit(f'{NAME}: {error}')
