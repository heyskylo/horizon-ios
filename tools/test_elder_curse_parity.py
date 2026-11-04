#!/usr/bin/env python3
"""An elder guardian's curse on a client, against vanilla's dedicated server.

A vanilla dedicated server and Horizon's each serve a fresh flat world; a vanilla-codec client
(VanillaProtocolClient, elder-curse mode: ElderCurseTranscript) joins in survival. With the ticks frozen the console
walls an elder guardian into a water box near the player (no line of sight: it never attacks), then steps 1201
ticks, so ElderGuardian.customServerAiStep's `(tickCount + id) % 1200 == 0` comes exactly once: Mining Fatigue III
for players within 50 blocks, and to each a GUARDIAN_ELDER_EFFECT game event whose parameter is 1 for a loud elder
and 0 for a silent one (`isSilent() ? 0.0F : 1.0F`; ClientPacketListener.handleGameEvent then shows the apparition
and plays the curse only for 1). Once with a loud elder, then, the fatigue cleared, with a `Silent:1b` one. Each
phase must send exactly one curse with that parameter.
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

PLAYER = 'Tester'
# The phase, the elder's Silent flag and the parameter its curse is sent with.
PHASES = [('loud', False, 1.0), ('silent', True, 0.0)]
# One more than the curse's period: its tick comes exactly once.
STEPS = 1201
MESSAGE = re.compile(r'\[Server thread/INFO\]: (?:System chat: )?(.*)$')


def run_side(label, command, client_command, out):
    directory = out / label
    if directory.exists():
        shutil.rmtree(directory)
    directory.mkdir(parents=True)
    port = free_port()
    props = {'server-ip': '127.0.0.1', 'server-port': str(port), 'online-mode': 'false', 'white-list': 'false',
             'level-type': 'minecraft:flat', 'generate-structures': 'false', 'level-seed': '42',
             'gamemode': 'survival', 'force-gamemode': 'true', 'difficulty': 'normal',
             'view-distance': '2', 'simulation-distance': '2', 'spawn-protection': '0',
             'pause-when-empty-seconds': '-1', 'max-tick-time': '-1'}
    (directory / 'server.properties').write_text(
        ''.join(f'{k}={escape(v)}\n' for k, v in props.items()), encoding='iso-8859-1')
    (directory / 'eula.txt').write_text('eula=true\n')
    transcript = out / f'{label}-packets.txt'
    server = Server(command, directory, label)
    client = None
    serial = 0

    def reply(text):
        nonlocal serial
        serial += 1
        marker = f'elder_curse_barrier_{serial}'
        start = len(server.lines)
        server.console(text)
        server.console(f'say {marker}')
        server.wait_for(re.escape(f'[Server] {marker}') + '$', timeout=180)
        return [m.group(1) for line in server.lines[start:] if (m := MESSAGE.search(line))
                and not m.group(1).endswith(f'[Server] {marker}')]

    def advance(ticks):
        start = game_time(server)
        reply(f'tick step {ticks}')
        deadline = time.monotonic() + 300
        while game_time(server) < start + ticks:
            assert time.monotonic() < deadline, f'{label}: tick step stalled'
        assert game_time(server) == start + ticks

    try:
        server.wait_for(r'Done \(')
        with (out / f'{label}-client.log').open('w') as log:
            client = subprocess.Popen([*map(str, client_command), '127.0.0.1', str(port), PLAYER,
                                       str(transcript), 'elder-curse'], cwd=lab.ROOT,
                                      stdout=log, stderr=subprocess.STDOUT)
            server.wait_for(rf'<{PLAYER}> summon the elders', timeout=180)
            reply('tick freeze')
            reply('gamerule minecraft:spawn_mobs false')
            reply('kill @e[type=!minecraft:player]')
            reply('forceload add 0 0 15 15')
            deadline = time.monotonic() + 180
            while not any('Test passed' in line for line in reply('execute if loaded 0 220 0')):
                assert time.monotonic() < deadline, f'{label}: scene chunk did not load'
            reply('fill 0 219 0 15 219 15 minecraft:stone')
            reply(f'tp {PLAYER} 8.5 220 11.5')
            reply('tick rate 200')
            for name, silent, _ in PHASES:
                reply('fill 1 215 1 5 219 5 minecraft:stone')
                reply('fill 2 216 2 4 218 4 minecraft:water')
                reply('summon minecraft:elder_guardian 3.5 216.5 3.5 '
                      f'{{PersistenceRequired:1b,Silent:{int(silent)}b,Tags:["elder_curse"]}}')
                # Entity.saveWithoutId writes Silent only when it is set.
                observed = reply('execute if entity @e[tag=elder_curse'
                                 + (',nbt={Silent:1b}]' if silent else ',nbt=!{Silent:1b}]'))
                assert any('Test passed' in line for line in observed), (name, observed)
                server.console(f'say elder_curse_{name}')
                server.wait_for(rf'<{PLAYER}> elder curse phase {name}', timeout=120)
                advance(STEPS)
                reply('kill @e[tag=elder_curse]')
                reply(f'effect clear {PLAYER} minecraft:mining_fatigue')
            server.console('say elder_curse_done')
            client.wait(timeout=180)
    finally:
        if client is not None and client.poll() is None:
            client.kill()
            client.wait()
        server.stop()
    lines = transcript.read_text().splitlines()
    assert not any(line.startswith('error ') for line in lines), lines[-10:]
    curses = []
    for line in lines:
        if line.startswith('play-elder-curse '):
            phase, param = line.split()[1:]
            curses.append({'phase': phase, 'param': float(param)})
    return {'curses': curses}


def checks(result):
    failures = []
    for name, _, param in PHASES:
        sent = [c['param'] for c in result['curses'] if c['phase'] == name]
        if sent != [param]:
            failures.append({'phase': name, 'expected': [param], 'sent': sent})
    stray = [c for c in result['curses'] if c['phase'] not in {p[0] for p in PHASES}]
    if stray:
        failures.append({'phase': 'outside', 'expected': [], 'sent': stray})
    return {'checks': len(PHASES) + 1, 'failures': failures}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, help='prebuilt Horizon game (otherwise build fast)')
    parser.add_argument('--out', type=Path, default=lab.LOCAL / 'elder-curse-parity')
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
    classes = out / 'classes'
    classes.mkdir(exist_ok=True)
    source = lab.ROOT / 'mods/horizon/src/test/java'
    cp = os.pathsep.join(map(str, [jars['server'], *libraries]))
    subprocess.run(['javac', '--release', str(lock['minecraft']['java_major']), '-proc:none',
                    '-sourcepath', source, '-cp', cp, '-d', classes,
                    source / 'dev/horizonlab/parity/VanillaProtocolClient.java'], check=True)
    client = ['java', '-Xmx1g', '-cp', str(classes) + os.pathsep + cp,
              'dev.horizonlab.parity.VanillaProtocolClient']
    commands = {'vanilla': ['java', '-Xmx2g', '-cp', cp, 'net.minecraft.server.Main', 'nogui'],
                'horizon': [binary, '--resources', lab.RESOURCES.resolve(),
                            '--dedicated', '--nogui']}
    results = {label: run_side(label, command, client, out) for label, command in commands.items()}
    report = {label: checks(result) for label, result in results.items()}
    lab.write_json(out / 'records.json', results)
    lab.write_json(out / 'report.json', {'schema': 1, 'minecraft': lock['minecraft']['version'], **report})
    for label, case in report.items():
        print(f"{label}: {case['checks'] - len(case['failures'])}/{case['checks']} checks pass", flush=True)
        for failure in case['failures']:
            print(json.dumps(failure), flush=True)
    assert not report['vanilla']['failures'], report['vanilla']
    assert not report['horizon']['failures'], f'elder curse failures; see {out}'


if __name__ == '__main__':
    main()
