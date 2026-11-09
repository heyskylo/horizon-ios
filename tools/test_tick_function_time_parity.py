#!/usr/bin/env python3
"""A tick function runs before the game time moves on, and before that tick's scheduled functions.

`MinecraftServer.tickChildren` runs the `#minecraft:tick` functions (`ServerFunctionManager.tick`) before
the world clocks and the levels tick. The Overworld's `ServerLevel.tickTime` then raises the game time
and runs the `/schedule`d functions due at the new time. Real dedicated servers, vanilla and Horizon,
take one `/tick step 1` from a frozen world with:

- a function scheduled from the console for the next tick (`sched`),
- an armed tick function (`tick`) that also schedules one for the next tick (`inner`).

Each records its place in the run order and what `time query gametime` and `time query time` return,
relative to the frozen values.

  test_tick_function_time_parity.py [--binary HORIZON] [--out DIR]
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

FUNCTIONS = ('tick', 'sched', 'inner')
FIELDS = ('order', 'game', 'clock')
# Vanilla 26.3: the tick function first, at the old times; both scheduled ones after it, at the new.
EXPECTED = {'tick': {'order': 1, 'game': 0, 'clock': 0},
            'sched': {'order': 2, 'game': 1, 'clock': 1},
            'inner': {'order': 3, 'game': 1, 'clock': 1}}
MESSAGE = re.compile(r'\[Server thread/INFO\]: (?:System chat: )?(.*)$')


def install_functions(world, jar):
    with zipfile.ZipFile(jar) as archive:
        version = json.loads(archive.read('version.json'))['pack_version']
    fmt = [version['data_major'], version['data_minor']]
    pack = world / 'datapacks' / 'tick-function-time'
    (pack / 'data/tft/function').mkdir(parents=True, exist_ok=True)
    (pack / 'data/minecraft/tags/function').mkdir(parents=True, exist_ok=True)
    (pack / 'pack.mcmeta').write_text(json.dumps({'pack': {
        'description': 'Tick function time', 'pack_format': fmt[0], 'min_format': fmt, 'max_format': fmt}}))
    (pack / 'data/minecraft/tags/function/tick.json').write_text(json.dumps({'values': ['tft:tick']}))
    (pack / 'data/tft/function/tick.mcfunction').write_text(
        'execute if score armed tft matches 1 run function tft:armed\n')

    def record(name):
        return ['scoreboard players add runs tft 1',
                f'scoreboard players operation {name}_order tft = runs tft',
                f'execute store result score {name}_game tft run time query gametime',
                f'execute store result score {name}_clock tft run time query time']
    (pack / 'data/tft/function/armed.mcfunction').write_text('\n'.join(
        ['scoreboard players set armed tft 0', *record('tick'), 'schedule function tft:inner 1t']) + '\n')
    for name in ('sched', 'inner'):
        (pack / f'data/tft/function/{name}.mcfunction').write_text('\n'.join(record(name)) + '\n')


def run_side(command, out, label, jar):
    directory = out / label
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
    install_functions(directory / 'world', jar)
    server = Server(command, directory, label)
    serial = 0

    def reply(text, timeout=180):
        nonlocal serial
        serial += 1
        marker = f'tick_function_time_end_{serial}'
        begin = len(server.lines)
        server.console(text)
        server.console(f'say {marker}')
        server.wait_for(re.escape(f'[Server] {marker}') + '$', timeout=timeout)
        return [m.group(1) for line in server.lines[begin:] if (m := MESSAGE.search(line))
                and not m.group(1).endswith(f'[Server] {marker}')]

    def value(lines, pattern):
        found = [m for line in lines if (m := re.search(pattern, line))]
        return int(found[0].group(1)) if found else None

    try:
        server.wait_for(r'Done \(')
        reply('tick freeze')
        reply('scoreboard objectives add tft dummy')
        reply('scoreboard players set runs tft 0')
        game = value(reply('time query gametime'), r'time is (-?\d+)')
        clock = value(reply('time query time'), r' at (-?\d+)')
        assert game is not None and clock is not None, f'{label}: no frozen times'
        assert any('Scheduled' in m for m in reply('schedule function tft:sched 1t')), f'{label}: not scheduled'
        reply('scoreboard players set armed tft 1')
        reply('tick step 1')
        deadline = time.monotonic() + 60
        while game_time(server) < game + 1:
            assert time.monotonic() < deadline, f'{label}: tick step stalled'
        assert game_time(server) == game + 1, f'{label}: more than one tick ran'
        records = {}
        for name in FUNCTIONS:
            records[name] = {}
            for field in FIELDS:
                got = value(reply(f'scoreboard players get {name}_{field} tft'), r'has (-?\d+)')
                if got is not None and field != 'order':
                    got -= game if field == 'game' else clock
                records[name][field] = got
        return records
    finally:
        server.stop()


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument('--binary', type=Path, help='prebuilt Horizon server (otherwise build fast)')
    parser.add_argument('--out', type=Path, default=lab.LOCAL / 'tick-function-time-parity')
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
    results = {label: run_side(command, out, label, jars['server']) for label, command in commands.items()}
    lab.write_json(out / 'records.json', results)
    assert results['vanilla'] == EXPECTED, f'vanilla changed: {results["vanilla"]}'
    failures = [{'function': name, 'field': field, 'vanilla': EXPECTED[name][field],
                 'horizon': results['horizon'][name][field]}
                for name in FUNCTIONS for field in FIELDS
                if results['horizon'][name][field] != EXPECTED[name][field]]
    print(f'{len(FUNCTIONS) * len(FIELDS) - len(failures)}/{len(FUNCTIONS) * len(FIELDS)} observations match',
          flush=True)
    lab.write_json(out / 'report.json', {'schema': 1, 'minecraft': lock['minecraft']['version'],
                                         'results': results, 'failures': failures})
    for failure in failures:
        print(json.dumps(failure), flush=True)
    assert not failures, f'{len(failures)} observations differ; see {out}'


if __name__ == '__main__':
    main()
