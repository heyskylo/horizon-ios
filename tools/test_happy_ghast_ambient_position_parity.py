#!/usr/bin/env python3
"""Natural HappyGhast ambient positions through unchanged Studio and the official codec.

Require adult, baby and ordinary cow events in 1200 actual ticks. Match subjects
by sound name and X/Z, so a displaced Y is not called missing transport. Retain
every raw packet/field/count/order/seed; no exact random schedule is claimed.
Silent suppression is a separate diagnostic and is not repaired here.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import struct
import subprocess
import time
import uuid
import zipfile

try:
    from . import lab, oracle_harness, test_rust_parity as base
    from .test_dedicated_server_parity import Server, free_port, game_time
except ImportError:
    import lab
    import oracle_harness
    import test_rust_parity as base
    from test_dedicated_server_parity import Server, free_port, game_time

TICKS = 1200
CASES = [
    ('adult', 'happy_ghast', 4.5, 4.5, 0, 20, False, 'entity.happy_ghast.ambient'),
    ('baby', 'happy_ghast', 12.5, 4.5, -24000, 20, False, 'entity.ghastling.ambient'),
    ('cow', 'cow', 4.5, 12.5, 0, 10, False, 'entity.cow.ambient'),
    ('silent', 'happy_ghast', 20.5, 4.5, 0, 20, True, 'entity.happy_ghast.ambient'),
]


def recorded_sounds(folder):
    archives = list((folder / 'recordings').glob('*.mcpr'))
    assert len(archives) == 1, archives
    with zipfile.ZipFile(archives[0]) as archive:
        assert archive.testzip() is None
        data = archive.read('recording.tmcpr')
        json.loads(archive.read('metaData.json'))
    rows, at, previous = [], 0, -1
    while at < len(data):
        assert len(data)-at >= 8, 'truncated Studio header'
        stamp, length = struct.unpack_from('>ii', data, at)
        at += 8
        assert stamp >= previous and 0 < length <= len(data)-at, 'incomplete/out-of-order recording'
        previous = stamp
        payload = data[at:at+length]
        at += length
        rows.append(dict(sequence=len(rows), timestamp_ms=stamp, payload=payload.hex()))
    assert rows, 'empty recording'
    (folder / 'packets.jsonl').write_text(''.join(json.dumps(r)+'\n' for r in rows))
    # 119 is ClientboundSoundPacket in pinned protocol 777. Decode and reencode
    # with the existing official codec below; no handwritten sound decoder.
    sounds = [r for r in rows if bytes.fromhex(r['payload'])[0] == 119]
    assert sounds, 'no sound packets'
    path = folder / 'sound-inputs.jsonl'
    path.write_text(''.join(json.dumps(r)+'\n' for r in sounds))
    return path


def run_side(label, command, client, folder, decoder):
    folder.mkdir(parents=True, exist_ok=False)
    port = free_port()
    (folder / 'server.properties').write_text(
        f'server-ip=127.0.0.1\nserver-port={port}\nonline-mode=false\nwhite-list=false\n'
        'level-type=minecraft:flat\ngenerate-structures=false\nlevel-seed=42\n'
        'gamemode=spectator\nforce-gamemode=true\nview-distance=2\nsimulation-distance=2\n'
        'spawn-protection=0\npause-when-empty-seconds=-1\nmax-tick-time=-1\n')
    (folder / 'eula.txt').write_text('eula=true\n')
    player_uuid = str(uuid.UUID(bytes=hashlib.md5(b'OfflinePlayer:Tester').digest(), version=3))
    (folder / 'ops.json').write_text(json.dumps([dict(uuid=player_uuid, name='Tester', level=4, bypassesPlayerLimit=True)]))
    server, process, serial, transcript, checks = Server(command, folder, label), None, 0, [], []

    def reply(command):
        nonlocal serial
        serial += 1
        start = len(server.lines)
        server.console(command)
        server.console(f'data modify storage h074:barrier value set value {serial}')
        server.console('data get storage h074:barrier value')
        server.wait_for(rf'h074:barrier.*contents: {serial}$', timeout=180)
        lines = server.lines[start:]
        transcript.append(dict(command=command, lines=lines))
        return '\n'.join(lines)

    def passed(command):
        text = reply(command)
        assert 'Test passed' in text, (label, command, text)
        checks.append(command)

    try:
        server.wait_for(r'Done \(')
        reply('tick freeze')
        reply('gamerule minecraft:spawn_mobs false')
        reply('gamerule minecraft:send_command_feedback false')
        reply('kill @e[type=!minecraft:player]')
        reply('scoreboard objectives add h074 dummy')
        reply('forceload add 0 0 31 31')
        deadline = time.monotonic()+180
        for x in [0, 16]:
            for z in [0, 16]:
                command = f'execute if loaded {x} 208 {z}'
                while 'Test passed' not in reply(command):
                    assert time.monotonic() < deadline, 'chunk readiness'
                checks.append(command)
        assert '1024' in reply('fill 0 199 0 31 199 31 minecraft:stone')
        assert '16384' in reply('fill 0 200 0 31 215 31 minecraft:barrier')
        assert '16384' in reply('fill 0 200 0 31 215 31 minecraft:air')
        # Existing silent commands each keep the unmodified net harness open 15s.
        env = lab.game_env(HORIZON_NET_REPORT=folder / 'client-report.json',
            HORIZON_NET_CHAT='/studio record start h074;h074_capture_ready;'
            + '/scoreboard players set #hold h074 0;'*3 + '/studio record stop')
        with (folder / 'client.log').open('w') as log:
            process = subprocess.Popen([str(client), '--resources', str(lab.RESOURCES.resolve()),
                '--server', f'127.0.0.1:{port}', '--name', 'Tester', '--render-distance', '2'],
                cwd=folder, env=env, stdout=log, stderr=subprocess.STDOUT)
            server.wait_for(r'<Tester> h074_capture_ready', timeout=180)
            reply('tp Tester 12.5 210 12.5')
            passed('execute if entity @a[name=Tester,nbt={Health:20f,playerGameType:3}]')
            reply('data get entity Tester UUID')
            for i, (name, kind, x, z, age, health, silent, _) in enumerate(CASES, 1):
                target = str(uuid.UUID(int=(74 << 96)+i))
                passed(f'execute if block {int(x)} 199 {int(z)} stone')
                passed(f'execute if block {int(x)} 208 {int(z)} air')
                reply(f'summon minecraft:{kind} {x} 208 {z} '
                      f'{{UUID:[I;74,0,0,{i}],Tags:["h074_{name}"],NoAI:1b,NoGravity:1b,'
                      f'PersistenceRequired:1b,Silent:{int(silent)}b,Age:{age},Health:{health}f}}')
                passed(f'execute as {target} if entity @s[type=minecraft:{kind},tag=h074_{name}]')
                passed(f'execute if data entity {target} {{UUID:[I;74,0,0,{i}],Age:{age},Health:{health}f,'
                       f'Pos:[{x}d,208d,{z}d],NoAI:1b,NoGravity:1b,PersistenceRequired:1b}}')
                reply(f'data get entity {target}')
            reply('tick rate 200')
            control = 'playsound minecraft:block.note_block.harp record @a 8.5 208 8.5 0.5 1.25'
            reply(control)
            start = game_time(server)
            reply(f'tick step {TICKS}')
            deadline = time.monotonic()+180
            while game_time(server) < start+TICKS:
                assert time.monotonic() < deadline, 'step timeout'
            assert game_time(server) == start+TICKS
            for i, (name, kind, x, z, _, health, silent, _) in enumerate(CASES, 1):
                target = str(uuid.UUID(int=(74 << 96)+i))
                passed(f'execute as {target} if entity @s[type=minecraft:{kind},tag=h074_{name}]')
                passed(f'execute if data entity {target} {{Health:{health}f,Pos:[{x}d,208d,{z}d],'
                       f'NoAI:1b,NoGravity:1b,PersistenceRequired:1b}}')
                reply(f'data get entity {target}')
                reply(f'data get entity {target} Age')
            checks.append(f'exact elapsed world ticks {TICKS}')
            reply(control)
            process.wait(timeout=180)
            assert process.returncode == 0
        report = json.loads((folder / 'client-report.json').read_text())
        assert report.get('error') is None, report
        inputs = recorded_sounds(folder)
        subprocess.run([*decoder, 'decode-recorded', inputs, folder / 'sounds.jsonl'], check=True)
        sounds = [json.loads(line) for line in (folder / 'sounds.jsonl').read_text().splitlines()]
        markers = [s for s in sounds if s['event'] == 'minecraft:block.note_block.harp']
        assert len(markers) == 2, 'recording did not cover both window boundaries'
        assert all((s['x'], s['y'], s['z'], s['category'], s['volume_bits'], s['pitch_bits']) ==
                   (8.5, 208.0, 8.5, 2, '3f000000', '3fa00000') for s in markers)
        checks.append('two exact capture boundary sounds')
        subjects, failures = {}, []
        for name, kind, x, z, _, _, silent, event in CASES:
            heard = [s for s in sounds if s['event'] == 'minecraft:'+event and s['x'] == x and s['z'] == z]
            subjects[name] = heard
            assert all(markers[0]['sequence'] < x['sequence'] < markers[1]['sequence'] for x in heard)
            if silent:
                continue  # retain raw Silent policy diagnostic, never a parity success
            assert heard, f'{label}: missing natural ambient for {name}'
            assert all(s['category'] == 6 for s in heard), (name, heard)
            if kind == 'happy_ghast':
                assert all(s['volume_bits'] == ('40800000' if name == 'adult' else '3f800000')
                           and s['pitch_bits'] == '3f800000' for s in heard), (name, heard)
            else:
                for s in heard:
                    pitch = struct.unpack('>f', bytes.fromhex(s['pitch_bits'].zfill(8)))[0]
                    assert .8 <= pitch <= 1.2 and int(s['volume_bits'], 16) > 0, s
            for s in heard:
                if s['y'] != 208.0:
                    failures.append(dict(subject=name, actual_y=s['y'], expected_y=208.0, sequence=s['sequence']))
        result = dict(admission=checks, elapsed_ticks=TICKS, sounds=sounds, subjects=subjects,
                      position_failures=failures, contract_pass=not failures,
                      diagnostic_silent_count=len(subjects['silent']))
        (folder / 'result.json').write_text(json.dumps(result, indent=2)+'\n')
        return result
    finally:
        (folder / 'commands.json').write_text(json.dumps(transcript, indent=2)+'\n')
        if process is not None and process.poll() is None:
            process.kill(); process.wait()
        server.stop()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, help='prebuilt server; otherwise build fast')
    parser.add_argument('--client-binary', type=Path, help='unchanged Studio client; defaults to server binary')
    parser.add_argument('--out', type=Path, help='fresh capture directory; default retains a new run')
    parser.add_argument('--reference-dir', type=Path, help='retained fresh Java capture; never rewritten')
    parser.add_argument('--reference-only', action='store_true')
    args = parser.parse_args()
    out = (args.out or lab.LOCAL / 'happy-ghast-ambient-parity' / f'run-{time.time_ns()}').resolve()
    out.mkdir(parents=True, exist_ok=False)
    if args.binary:
        binary = args.binary.resolve(strict=True)
    else:
        subprocess.run(['cargo', 'build', '--locked', '--profile', 'fast', '-p', 'horizon-client',
                        '--bin', 'horizon-client'], cwd=lab.ROOT, check=True)
        binary = lab.cargo_target() / 'fast' / ('horizon-client.exe' if os.name == 'nt' else 'horizon-client')
    client_binary = args.client_binary.resolve(strict=True) if args.client_binary else binary
    lock, jars, libraries = base.verify_artifacts()
    classes, dependencies, _ = oracle_harness.compile_oracle(out, 'VanillaEntitySoundPacketOracle',
        jars, libraries, lock['minecraft']['java_major'])
    decoder = ['java', '-Xmx1g', '-cp', str(classes)+os.pathsep+dependencies,
               'dev.horizonlab.parity.VanillaEntitySoundPacketOracle']
    cp = os.pathsep.join(map(str, [jars['server'], *libraries]))
    reference = args.reference_dir.resolve() if args.reference_dir else out
    if not args.reference_dir:
        java = run_side('java', ['java', '-Xmx2g', '-cp', cp, 'net.minecraft.server.Main', 'nogui'],
                        client_binary, out / 'java', decoder)
        assert java['contract_pass'], java
        (out / 'reference.json').write_text(json.dumps(java, indent=2)+'\n')
    if args.reference_only:
        return 0
    rust = run_side('horizon', [binary, '--resources', lab.RESOURCES.resolve(), '--dedicated', '--nogui'],
                    client_binary, out / 'horizon', decoder)
    assert json.loads((reference / 'reference.json').read_text())['contract_pass']
    (out / 'report.json').write_text(json.dumps(rust, indent=2)+'\n')
    print(f"Ambient positions: {len(rust['position_failures'])} displaced events; {len(rust['admission'])} admission/control checks passed; Silent count {rust['diagnostic_silent_count']} (diagnostic)")
    return 0 if rust['contract_pass'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
