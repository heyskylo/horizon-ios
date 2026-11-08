#!/usr/bin/env python3
"""Saved scheduled ticks fire when their saved delay says, after a restart and after a frozen reload.

Real dedicated servers, vanilla and Horizon, on the same scene: in a forceloaded chunk of a frozen
world, repeaters of delay 1 to 4 are powered and a water source is placed, so their block and fluid
ticks are pending (due 2, 4, 6, 8 and 5 ticks on). Then either

- `restart`: the server stops and starts again. `MinecraftServer.prepareLevels` waits for the
  restored forced chunk (`ChunkLoadCounter`), and the first tick unpacks its saved ticks at the saved
  game time (`ServerLevel.startTickingChunk`); or
- `frozen`: the chunk is unloaded and loaded back while the world stays frozen, then it runs.

A datapack's tick function records the first game time it runs and the game time each repeater is
first seen powered and the water first seen flowing; the harness compares those times relative to the
first run. After the restart, vanilla skips the chunk's ticks until its entity section has loaded
(`isPositionTickingWithEntitiesLoaded`), which can still be pending on the second tick, so the delay-1
repeater (due on the second tick) is left out of that comparison.

  test_chunk_restart_ticks_parity.py [--binary HORIZON] [--out DIR]
"""
import argparse
import json
import os
import re
import shutil
import subprocess
import time
import zipfile
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

X0, Z0, Y = 512, 512, 101
REPEATERS = [(f'r{d}', X0 + 2 + 3 * d, Z0 + 4, d) for d in (1, 2, 3, 4)]
WATER = ('w', X0 + 4, Z0 + 10)
KEYS = [name for name, *_ in REPEATERS] + [WATER[0]]
# Ticks after the first tick-function run: a repeater's delay d is 2d ticks, water flows after 5.
EXPECTED = {'r1': 2, 'r2': 4, 'r3': 6, 'r4': 8, 'w': 5}
COMPARED = {'restart': ['r2', 'r3', 'r4', 'w'], 'frozen': KEYS}
MESSAGE = re.compile(r'\[Server thread/INFO\]: (?:System chat: )?(.*)$')


def install_recorder(world, jar):
    with zipfile.ZipFile(jar) as archive:
        version = json.loads(archive.read('version.json'))['pack_version']
    fmt = [version['data_major'], version['data_minor']]
    pack = world / 'datapacks' / 'saved-ticks'
    (pack / 'data/saved_ticks/function').mkdir(parents=True, exist_ok=True)
    (pack / 'data/minecraft/tags/function').mkdir(parents=True, exist_ok=True)
    (pack / 'pack.mcmeta').write_text(json.dumps({'pack': {
        'description': 'Saved tick recorder', 'pack_format': fmt[0], 'min_format': fmt, 'max_format': fmt}}))
    (pack / 'data/minecraft/tags/function/tick.json').write_text(json.dumps({'values': ['saved_ticks:tick']}))
    unset = 'execute unless score {} st matches -2147483648..'
    lines = ['execute unless score armed st matches 1 run return 0',
             unset.format('start') + ' store result score start st run time query gametime']
    for name, x, z, _ in REPEATERS:
        lines.append(unset.format(name) + f' if block {x} {Y} {z} repeater[powered=true]'
                     f' store result score {name} st run time query gametime')
    name, x, z = WATER
    for dx, dz in ((1, 0), (-1, 0), (0, 1), (0, -1)):
        lines.append(unset.format(name) + f' if block {x + dx} {Y} {z + dz} water'
                     f' store result score {name} st run time query gametime')
    (pack / 'data/saved_ticks/function/tick.mcfunction').write_text('\n'.join(lines) + '\n')


def run_case(command, out, label, case, jar):
    directory = out / f'{label}-{case}'
    if directory.exists():
        shutil.rmtree(directory)
    directory.mkdir(parents=True)
    props = {'server-ip': '127.0.0.1', 'server-port': str(free_port()), 'online-mode': 'false',
             'white-list': 'false', 'level-type': 'minecraft:flat', 'generate-structures': 'false',
             'level-seed': '42', 'view-distance': '2', 'simulation-distance': '2', 'spawn-protection': '0',
             'pause-when-empty-seconds': '-1', 'max-tick-time': '-1', 'function-permission-level': '4'}
    (directory / 'server.properties').write_text(
        ''.join(f'{k}={escape(v)}\n' for k, v in props.items()), encoding='iso-8859-1')
    (directory / 'eula.txt').write_text('eula=true\n')
    install_recorder(directory / 'world', jar)

    def start(session):
        server = Server(command, directory, f'{label}-{case}-{session}')
        serial = 0

        def reply(text, timeout=180):
            nonlocal serial
            serial += 1
            marker = f'saved_ticks_end_{serial}'
            begin = len(server.lines)
            server.console(text)
            server.console(f'say {marker}')
            server.wait_for(re.escape(f'[Server] {marker}') + '$', timeout=timeout)
            return [m.group(1) for line in server.lines[begin:] if (m := MESSAGE.search(line))
                    and not m.group(1).endswith(f'[Server] {marker}')]

        def until(condition, what):
            deadline = time.monotonic() + 300
            while not any('Test passed' in m for m in reply(f'execute {condition}')):
                assert time.monotonic() < deadline, f'{label} {case}: {what}'
                time.sleep(0.2)

        try:
            server.wait_for(r'Done \(')
        except BaseException:
            server.stop()
            raise
        return server, reply, until

    def recorded(reply):
        """The recorder's times, once every one is in."""
        deadline = time.monotonic() + 120
        while True:
            scores = {}
            for key in ['start'] + KEYS:
                found = [re.search(r'has (-?\d+)', line) for line in reply(f'scoreboard players get {key} st')]
                scores[key] = next((int(m.group(1)) for m in found if m), None)
            if all(v is not None for v in scores.values()) or time.monotonic() > deadline:
                return {k: None if scores[k] is None or scores['start'] is None else scores[k] - scores['start']
                        for k in KEYS}
            time.sleep(0.5)

    server, reply, until = start('setup')
    try:
        reply('tick freeze')
        reply('scoreboard objectives add st dummy')
        reply(f'forceload add {X0} {Z0}')
        until(f'if loaded {X0} {Y} {Z0}', 'the chunk did not load')
        # One running tick: the chunk's tick queues are registered before the commands below schedule.
        before = game_time(server)
        reply('tick step 1')
        deadline = time.monotonic() + 60
        while game_time(server) < before + 1:
            assert time.monotonic() < deadline, f'{label} {case}: tick step stalled'
        assert any('filled' in m for m in reply(f'fill {X0} {Y - 1} {Z0} {X0 + 15} {Y - 1} {Z0 + 15} stone'))
        reply(f'fill {X0} {Y} {Z0} {X0 + 15} {Y + 2} {Z0 + 15} air')
        for _, x, z, d in REPEATERS:
            reply(f'setblock {x} {Y} {z} repeater[facing=east,delay={d}]')
        saved_at = game_time(server)
        for _, x, z, _ in REPEATERS:
            reply(f'setblock {x + 1} {Y} {z} redstone_block')
        reply(f'setblock {WATER[1]} {Y} {WATER[2]} water')
        for name, x, z, _ in REPEATERS:
            assert any('Test passed' in m for m in reply(f'execute if block {x} {Y} {z} repeater[powered=false]')), name
        if case == 'frozen':
            reply(f'forceload remove {X0} {Z0}')
            until(f'unless loaded {X0} {Y} {Z0}', 'the chunk did not unload')
            reply(f'forceload add {X0} {Z0}')
            until(f'if loaded {X0} {Y} {Z0}', 'the chunk did not load again')
            # Vanilla loads the chunk's entity section asynchronously; let it settle while frozen.
            time.sleep(3)
            reply('scoreboard players set armed st 1')
            assert game_time(server) == saved_at, f'{label} {case}: the frozen clock moved'
            reply('tick unfreeze')
            return recorded(reply)
        reply('scoreboard players set armed st 1')
        assert game_time(server) == saved_at, f'{label} {case}: the frozen clock moved'
    finally:
        server.stop()
    server, reply, until = start('restart')
    try:
        assert not any('frozen' in m for m in reply('tick query')), f'{label}: the restart is frozen'
        return recorded(reply)
    finally:
        server.stop()


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--binary', type=Path, help='prebuilt Horizon server (otherwise build fast)')
    parser.add_argument('--out', type=Path, default=lab.LOCAL / 'chunk-restart-ticks-parity')
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
    results = {label: {case: run_case(command, out, label, case, jars['server']) for case in COMPARED}
               for label, command in commands.items()}
    lab.write_json(out / 'records.json', results)
    failures = []
    for case, keys in COMPARED.items():
        expected, actual = results['vanilla'][case], results['horizon'][case]
        for key in keys:
            assert expected[key] == EXPECTED[key], f'vanilla {case} {key}: {expected[key]}, not {EXPECTED[key]}'
            if actual[key] != expected[key]:
                failures.append({'case': case, 'tick': key, 'vanilla': expected[key], 'horizon': actual[key]})
        matched = len(keys) - sum(f['case'] == case for f in failures)
        print(f'{case}: {matched}/{len(keys)} saved ticks fire on time', flush=True)
    lab.write_json(out / 'report.json', {'schema': 1, 'minecraft': lock['minecraft']['version'],
                                         'results': results, 'failures': failures})
    for failure in failures:
        print(json.dumps(failure), flush=True)
    assert not failures, f'{len(failures)} saved ticks fire at other times; see {out}'


if __name__ == '__main__':
    main()
