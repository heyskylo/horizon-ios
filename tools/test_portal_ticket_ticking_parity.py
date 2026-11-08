#!/usr/bin/env python3
"""What runs in the rings of chunks around a portal ticket that no player is near.

Real dedicated servers, vanilla and Horizon, on the same scene: an item goes through a Nether portal in a
forceloaded Nether chunk and arrives in a flat Overworld nobody is in. The exit portal's chunk gets a `PORTAL`
ticket of radius 3 (`Entity.placePortalTicket`, level 30), so its rings 0 and 1 are entity ticking, ring 2 block
ticking (level 32) and ring 3 only `FULL` (33): loaded, with nothing running.

In one chunk of rings 1, 2 and 3 east of the ticket's chunk the harness builds a repeater, a piston it powers and
a water source. A redstone block placed last behind the repeater schedules its tick; the piston extends through a
block event once the repeater is powered, and the water's placement schedules its fluid tick. For each ring it
records whether the chunk is loaded (`execute if block` finds the grass), and 40 game ticks after the placement whether the
repeater is powered (scheduled block tick: `ServerLevel.isPositionTickingWithEntitiesLoaded`), the piston
extended (block event: `ServerLevel.runBlockEvents`, `shouldTickBlocksAt`) and the water flowed (scheduled
fluid tick).

  test_portal_ticket_ticking_parity.py [--binary HORIZON] [--out DIR]
"""
import argparse
import json
import os
import re
import shutil
import subprocess
import time
from pathlib import Path

try:
    from . import lab, test_rust_parity as base
    from .test_dedicated_server_parity import Server, free_port, game_time
    from .test_server_world_gen_parity import escape
except ImportError:
    import lab
    import test_rust_parity as base
    from test_dedicated_server_parity import Server, free_port, game_time
    from test_server_world_gen_parity import escape

NETHER = 'execute in minecraft:the_nether run '
OVERWORLD = 'execute in minecraft:overworld run '
# The Nether portal: an obsidian frame along x at z = 101, its inside x 101..102, y 71..73. An item there
# leaves for about (812, 812) in the Overworld, inside chunk (50, 50).
PORTAL = (100, 70, 101)
# The flat world's grass is at y = -61.
Y = -60
RINGS = (1, 2, 3)
# 40 game ticks are enough for every reaction: a repeater of delay 1 ticks after 2, water flows after 5.
WAIT = 40
# Vanilla's PORTAL ticket lives 300 ticks once its chunk is ready: every observation must be taken within them.
TICKET_TICKS = 300
MESSAGE = re.compile(r'\[Server thread/INFO\]: (?:System chat: )?(.*)$')
POS = re.compile(r'\[(-?[\d.E-]+)d, (-?[\d.E-]+)d, (-?[\d.E-]+)d\]')


def run(command, out, label):
    directory = out / label
    if directory.exists():
        shutil.rmtree(directory)
    directory.mkdir(parents=True)
    props = {'server-ip': '127.0.0.1', 'server-port': str(free_port()), 'online-mode': 'false',
             'white-list': 'false', 'level-type': 'minecraft:flat', 'generate-structures': 'false',
             'level-seed': '42', 'view-distance': '2', 'simulation-distance': '2', 'spawn-protection': '0',
             'pause-when-empty-seconds': '-1', 'max-tick-time': '-1', 'spawn-monsters': 'false',
             'spawn-animals': 'false', 'spawn-npcs': 'false'}
    (directory / 'server.properties').write_text(
        ''.join(f'{k}={escape(v)}\n' for k, v in props.items()), encoding='iso-8859-1')
    (directory / 'eula.txt').write_text('eula=true\n')
    server = Server(command, directory, label)
    serial = 0

    def reply(text, timeout=180):
        nonlocal serial
        serial += 1
        marker = f'portal_ring_end_{serial}'
        begin = len(server.lines)
        server.console(text)
        server.console(f'say {marker}')
        server.wait_for(re.escape(f'[Server] {marker}') + '$', timeout=timeout)
        return [m.group(1) for line in server.lines[begin:] if (m := MESSAGE.search(line))
                and not m.group(1).endswith(f'[Server] {marker}')]

    def passes(text):
        return any('Test passed' in m for m in reply(text))

    try:
        server.wait_for(r'Done \(')
        px, py, pz = PORTAL
        reply(NETHER + f'forceload add {px} {pz}')
        deadline = time.monotonic() + 300
        while not passes(f'execute in minecraft:the_nether if loaded {px} {py} {pz}'):
            assert time.monotonic() < deadline, f'{label}: the Nether chunk did not load'
            time.sleep(0.2)
        reply(NETHER + f'fill {px - 4} {py - 1} {pz - 4} {px + 7} {py + 6} {pz + 4} air')
        reply(NETHER + f'fill {px - 4} {py - 1} {pz - 4} {px + 7} {py - 1} {pz + 4} obsidian')
        reply(NETHER + f'fill {px} {py} {pz} {px + 3} {py + 4} {pz} obsidian')
        reply(NETHER + f'fill {px + 1} {py + 1} {pz} {px + 2} {py + 3} {pz} nether_portal[axis=x]')
        reply(NETHER + f'summon item {px + 1.5} {py + 1} {pz + 0.5} '
              '{Item: {id: "minecraft:stone", count: 1}, Tags: ["traveller"], PickupDelay: 32767s}')
        deadline = time.monotonic() + 120
        while True:
            found = [POS.search(m) for m in reply(
                OVERWORLD + 'data get entity @e[tag=traveller,distance=0..,limit=1] Pos')]
            found = next((m for m in found if m), None)
            if found:
                arrived = game_time(server)
                break
            assert time.monotonic() < deadline, f'{label}: the item did not reach the Overworld'
            time.sleep(0.1)
        reply(OVERWORLD + 'kill @e[tag=traveller,distance=0..]')
        x, z = float(found.group(1)), float(found.group(3))
        # The ticket's chunk is the exit's: an item a block from its chunk's edge could have moved into the next
        # one since it arrived.
        assert 1 <= x % 16 <= 15 and 1 <= z % 16 <= 15, f'{label}: the item arrived at a chunk edge ({x}, {z})'
        cx, cz = int(x // 16), int(z // 16)
        bases = {r: (16 * (cx + r) + 8, 16 * cz + 8) for r in RINGS}

        def loaded():
            # `execute if loaded` asks for an entity-ticking chunk; `if block` answers in any loaded (`FULL`) one
            # and fails "That position is not loaded" in the others.
            return {r: passes(f'execute in minecraft:overworld if block {bx} {Y - 1} {bz} grass_block')
                    for r, (bx, bz) in bases.items()}

        # Rings 1 and 2 are loaded in every case; ring 3 has the moment they are and a second more.
        deadline = time.monotonic() + 60
        while not all((now := loaded())[r] for r in (1, 2)):
            assert time.monotonic() < deadline, f'{label}: rings 1 and 2 did not load ({now})'
            time.sleep(0.1)
        settle = time.monotonic() + 1
        while not now[3] and time.monotonic() < settle:
            time.sleep(0.1)
            now = loaded()
        for r, (bx, bz) in bases.items():
            reply(OVERWORLD + f'fill {bx - 3} {Y - 1} {bz - 3} {bx + 3} {Y - 1} {bz + 4} stone')
            reply(OVERWORLD + f'fill {bx - 3} {Y} {bz - 3} {bx + 3} {Y + 1} {bz + 4} air')
            reply(OVERWORLD + f'setblock {bx - 1} {Y} {bz} piston[facing=up]')
            reply(OVERWORLD + f'setblock {bx} {Y} {bz} repeater[facing=east,delay=1]')
            # A wall keeps the water off the repeater.
            reply(OVERWORLD + f'fill {bx - 3} {Y} {bz + 1} {bx + 3} {Y + 1} {bz + 1} stone')
            reply(OVERWORLD + f'setblock {bx} {Y} {bz + 3} water')
            reply(OVERWORLD + f'setblock {bx + 1} {Y} {bz} redstone_block')
        placed = game_time(server)
        deadline = time.monotonic() + 120
        while game_time(server) < placed + WAIT:
            assert time.monotonic() < deadline, f'{label}: the clock stalled'
            time.sleep(0.1)
        result = {}
        for r, (bx, bz) in bases.items():
            check = f'execute in minecraft:overworld if block '
            result[f'ring{r}'] = {
                'loaded': now[r],
                'repeater': passes(check + f'{bx} {Y} {bz} repeater[powered=true]'),
                'piston': passes(check + f'{bx - 1} {Y} {bz} piston[extended=true]'),
                'water': passes(check + f'{bx} {Y} {bz + 4} water'),
            }
        assert game_time(server) < arrived + TICKET_TICKS, f'{label}: the portal ticket may have run out'
        return result
    finally:
        server.stop()


def expected(results):
    """What vanilla showed, checked against the ticket levels: rings 1 and 2 run everything, ring 3 is loaded
    and runs nothing."""
    every = {'loaded': True, 'repeater': True, 'piston': True, 'water': True}
    want = {'ring1': every, 'ring2': every, 'ring3': {'loaded': True, 'repeater': False, 'piston': False,
                                                       'water': False}}
    assert results == want, f'vanilla showed {results}, not {want}'


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--binary', type=Path, help='prebuilt Horizon server (otherwise build fast)')
    parser.add_argument('--out', type=Path, default=lab.LOCAL / 'portal-ticket-ticking-parity')
    args = parser.parse_args()
    lock, jars, libraries = base.verify_artifacts()
    if args.binary:
        binary = args.binary.resolve(strict=True)
    else:
        subprocess.run(['cargo', 'build', '--profile', 'fast', '-p', 'horizon-client',
                        '--bin', 'horizon-client'], cwd=lab.ROOT, check=True)
        metadata = json.loads(subprocess.check_output(
            ['cargo', 'metadata', '--no-deps', '--format-version', '1'], cwd=lab.ROOT, text=True))
        binary = Path(metadata['target_directory']) / 'fast' / (
            'horizon-client.exe' if os.name == 'nt' else 'horizon-client')
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    cp = os.pathsep.join(map(str, [jars['server'], *libraries]))
    commands = {'vanilla': ['java', '-Xmx2g', '-cp', cp, 'net.minecraft.server.Main', 'nogui'],
                'horizon': [binary, '--resources', lab.RESOURCES.resolve(), '--dedicated', '--nogui']}
    results = {label: run(command, out, label) for label, command in commands.items()}
    lab.write_json(out / 'records.json', results)
    expected(results['vanilla'])
    failures = [{'ring': ring, 'what': key, 'vanilla': value, 'horizon': results['horizon'][ring][key]}
                for ring, seen in results['vanilla'].items() for key, value in seen.items()
                if results['horizon'][ring][key] != value]
    lab.write_json(out / 'report.json', {'schema': 1, 'minecraft': lock['minecraft']['version'],
                                         'results': results, 'failures': failures})
    for ring, seen in results['horizon'].items():
        print(f'{ring}: horizon {seen}', flush=True)
    for failure in failures:
        print(json.dumps(failure), flush=True)
    assert not failures, f'{len(failures)} observations differ; see {out}'


if __name__ == '__main__':
    main()
