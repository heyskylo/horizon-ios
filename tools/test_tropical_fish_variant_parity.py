#!/usr/bin/env python3
"""Tropical fish packed NBT color bytes normalize through DyeColor.byId.

Fresh dedicated servers cover valid colors, invalid bytes and pattern fallback.
No rendering, bucket, spawn, loot or component parity claim.
"""
import argparse
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import time

try:
    from . import lab, test_rust_parity as base
    from .test_dedicated_server_parity import Server, free_port, game_time
    from .test_server_world_gen_parity import escape
except ImportError:
    import lab
    import test_rust_parity as base
    from test_dedicated_server_parity import Server, free_port, game_time
    from test_server_world_gen_parity import escape

CASES = [None, -1, 0, 1, 2, 1280, 1281, 1536, 65538, 117506305]
CASES += [value << 16 for value in (15, 16, 17, 255)]
CASES += [(value << 24) if value < 128 else (value << 24) - (1 << 32)
          for value in (15, 16, 17, 255)]
EXPECTED = [0, 0, 0, 1, 0, 1280, 1281, 0, 65536, 117506305,
            983040, 0, 0, 0, 251658240, 0, 0, 0]
MESSAGE = re.compile(r'\[Server thread/INFO\]: (?:System chat: )?(.*)$')


def run_side(command, out, label):
    directory = out / label
    if directory.exists():
        shutil.rmtree(directory)
    directory.mkdir(parents=True)
    props = {'server-ip': '127.0.0.1', 'server-port': str(free_port()),
             'online-mode': 'false', 'white-list': 'false', 'level-type': 'minecraft:flat',
             'generate-structures': 'false', 'level-seed': '42', 'difficulty': 'normal',
             'view-distance': '2', 'simulation-distance': '2', 'spawn-protection': '0',
             'pause-when-empty-seconds': '-1', 'max-tick-time': '-1'}
    (directory / 'server.properties').write_text(
        ''.join(f'{k}={escape(v)}\n' for k, v in props.items()), encoding='iso-8859-1')
    (directory / 'eula.txt').write_text('eula=true\n')
    server = Server(command, directory, label)
    serial, records = 0, []

    def reply(text, timeout=180):
        nonlocal serial
        serial += 1
        marker = f'fish_variant_end_{serial}'
        start = len(server.lines)
        server.console(text)
        server.console(f'say {marker}')
        server.wait_for(re.escape(f'[Server] {marker}') + '$', timeout=timeout)
        return [m.group(1) for line in server.lines[start:] if (m := MESSAGE.search(line))
                and not m.group(1).endswith(f'[Server] {marker}')]

    def advance(ticks):
        start = game_time(server)
        reply(f'tick step {ticks}')
        deadline = time.monotonic() + 180
        while game_time(server) < start + ticks:
            assert time.monotonic() < deadline, f'{label}: tick step stalled'
        assert game_time(server) == start + ticks

    try:
        server.wait_for(r'Done \(')
        reply('tick freeze')
        reply('tick rate 200')
        assert any('false' in m for m in reply('gamerule minecraft:spawn_mobs false'))
        assert any('normal' in m.lower() for m in reply('difficulty'))
        reply('forceload add 0 0 31 31')
        deadline = time.monotonic() + 300
        while True:
            remaining = deadline - time.monotonic()
            assert remaining > 0, f'{label}: fish scene did not load'
            ready = [reply(f'execute if loaded {x} 200 {z}', remaining)
                     for x, z in ((0, 0), (16, 0), (0, 16), (16, 16))]
            if all(any('Test passed' in m for m in messages) for messages in ready):
                break
        assert any('filled' in m for m in reply('fill 0 198 0 31 199 31 stone'))
        for index, variant in enumerate(CASES):
            reply('kill @e[type=!minecraft:player]')
            advance(25)
            reply('kill @e[type=minecraft:item]')
            assert any('Test failed' in m for m in reply('execute if entity @e[tag=subject]'))
            nbt = (f'{{Tags:["subject","fish_{index}"],NoAI:1b,NoGravity:1b,'
                   'Silent:1b,PersistenceRequired:1b')
            if variant is not None:
                nbt += f',Variant:{variant}'
            assert any('Summoned new' in m for m in reply(
                f'summon minecraft:tropical_fish 10.5 200 10.5 {nbt}}}'))
            for tick in (0, 1):
                if tick:
                    advance(1)
                for field, text in (
                    ('subject', 'execute if entity @e[tag=subject]'),
                    ('Variant', 'data get entity @e[tag=subject,limit=1] Variant')):
                    records.append({'case': index, 'input': variant, 'tick': tick,
                                    'field': field, 'command': text, 'messages': reply(text)})
        return records
    finally:
        server.stop()


def validate_reference(records):
    for row in records:
        if row['field'] == 'subject':
            assert row['messages'] == ['Test passed. Count: 1'], row
        else:
            values = [int(m.group(1)) for line in row['messages']
                      if (m := re.search(r'data: (-?\d+)$', line))]
            assert values == [EXPECTED[row['case']]], row


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, help='prebuilt Horizon server (otherwise build fast)')
    parser.add_argument('--out', type=Path, default=lab.LOCAL / 'tropical-fish-variant-parity')
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
    results = {label: run_side(command, out, label) for label, command in commands.items()}
    lab.write_json(out / 'records.json', results)
    expected, actual = results['vanilla'], results['horizon']
    assert len(expected) == len(actual) == 72
    validate_reference(expected)
    failures = [{'vanilla': v, 'horizon': h} for v, h in zip(expected, actual) if v != h]
    lab.write_json(out / 'report.json', {'schema': 1, 'minecraft': lock['minecraft']['version'],
                                       'observations': len(expected), 'failures': failures,
                                       'scope': 'Packed entity NBT only; no render/bucket/spawn/loot/component claim.'})
    print(f'{len(expected) - len(failures)}/{len(expected)} observations match', flush=True)
    for failure in failures:
        print(json.dumps(failure), flush=True)
    assert not failures, f'{len(failures)} fish variant observations differ; see {out}'


if __name__ == '__main__':
    main()
