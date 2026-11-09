#!/usr/bin/env python3
"""Allay duplication cooldowns count down independently of NoAI.

Fresh dedicated servers cover both AI modes and signed-int loader controls.
No duplication, healing, dance, sound, vibration, packet or reload claim.
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

CASES = [(no_ai, value) for no_ai in (0, 1)
         for value in (30, 1, 0, -2, 2147483648, 4294967296)]
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
        marker = f'allay_cooldown_end_{serial}'
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
            assert remaining > 0, f'{label}: Allay scene did not load'
            ready = [reply(f'execute if loaded {x} 200 {z}', remaining)
                     for x, z in ((0, 0), (16, 0), (0, 16), (16, 16))]
            if all(any('Test passed' in m for m in messages) for messages in ready):
                break
        assert any('filled' in m for m in reply('fill 0 198 0 31 199 31 stone'))
        for x, z in ((0, 0), (16, 0), (0, 16), (16, 16)):
            text = f'execute if loaded {x} 200 {z}'
            records.append({'field': 'loaded', 'command': text, 'messages': reply(text)})
        for index, (no_ai, cooldown) in enumerate(CASES):
            reply('kill @e[type=!minecraft:player]')
            advance(25)
            assert any('Test failed' in m for m in reply('execute if entity @e[tag=subject]'))
            nbt = (f'{{Tags:["subject","allay_{index}"],NoAI:{no_ai}b,NoGravity:1b,'
                   f'Silent:1b,PersistenceRequired:1b,DuplicationCooldown:{cooldown}L}}')
            assert any('Summoned new' in m for m in reply(f'summon minecraft:allay 10.5 200 10.5 {nbt}'))
            previous = 0
            for tick in (0, 1, 3, 10):
                if tick:
                    advance(tick - previous)
                previous = tick
                condition = 'if' if no_ai else 'unless'
                for field, text in (
                    ('subject', 'execute if entity @e[tag=subject]'),
                    ('NoAI', f'execute {condition} entity @e[tag=subject,nbt={{NoAI:1b}}]'),
                    ('DuplicationCooldown', 'data get entity @e[tag=subject,limit=1] DuplicationCooldown')):
                    records.append({'case': index, 'input': cooldown, 'no_ai': no_ai, 'tick': tick,
                                    'field': field, 'command': text, 'messages': reply(text)})
        return records
    finally:
        server.stop()


def validate_reference(records):
    for row in records:
        if row['field'] != 'DuplicationCooldown':
            assert len(row['messages']) == 1 and row['messages'][0].startswith('Test passed'), row
        else:
            loaded = {2147483648: -2147483648, 4294967296: 0}.get(row['input'], row['input'])
            expected = max(0, loaded - row['tick']) if loaded > 0 else loaded
            values = [int(m.group(1)) for line in row['messages']
                      if (m := re.search(r'data: (-?\d+)L$', line))]
            assert values == [expected], row


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, help='prebuilt Horizon server (otherwise build fast)')
    parser.add_argument('--out', type=Path, default=lab.LOCAL / 'allay-cooldown-parity')
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
    assert len(expected) == len(actual) == 148
    validate_reference(expected)
    failures = [{'vanilla': v, 'horizon': h} for v, h in zip(expected, actual) if v != h]
    lab.write_json(out / 'report.json', {'schema': 1, 'minecraft': lock['minecraft']['version'],
                                       'observations': len(expected), 'failures': failures,
                                       'scope': 'Cooldown dispatch only; no duplication/healing/dance/sound/vibration/packet/reload claim.'})
    print(f'{len(expected) - len(failures)}/{len(expected)} observations match', flush=True)
    for failure in failures:
        print(json.dumps(failure), flush=True)
    assert not failures, f'{len(failures)} Allay cooldown observations differ; see {out}'


if __name__ == '__main__':
    main()
