#!/usr/bin/env python3
"""Real block-hit arrow category, captured by the unchanged headless client's Studio recorder.

The contract is admission, air-to-ground, one arrow hit on NEUTRAL, and two
independent RECORDS controls. Full packets (including random pitch/seed and
order) remain raw diagnostics; trajectories and random draws are not compared.
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

UUID = '0000014b-0000-0000-0000-000000000001'


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
        server.console(f'data modify storage h331:barrier value set value {serial}')
        server.console('data get storage h331:barrier value')
        server.wait_for(rf'h331:barrier.*contents: {serial}$', timeout=180)
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
        reply('scoreboard objectives add h331 dummy')
        reply('forceload add 0 0 15 15')
        deadline = time.monotonic()+180
        while 'Test passed' not in reply('execute if loaded 4 200 4'):
            assert time.monotonic() < deadline, 'chunk readiness'
        assert '256' in reply('fill 0 199 0 15 199 15 minecraft:stone')
        reply('fill 0 200 0 15 205 15 minecraft:air')
        passed('execute if block 4 199 4 minecraft:stone')
        passed('execute if block 4 201 4 minecraft:air')
        # The final silent command keeps the existing net harness connected
        # while the console runs this bounded scene. It then stops Studio.
        env = lab.game_env(HORIZON_NET_REPORT=folder / 'client-report.json',
            HORIZON_NET_CHAT='/studio record start h331;h331_capture_ready;'
            '/scoreboard players set #hold h331 0;/studio record stop')
        with (folder / 'client.log').open('w') as log:
            process = subprocess.Popen([str(client), '--resources', str(lab.RESOURCES.resolve()),
                '--server', f'127.0.0.1:{port}', '--name', 'Tester', '--render-distance', '2'],
                cwd=folder, env=env, stdout=log, stderr=subprocess.STDOUT)
            server.wait_for(r'<Tester> h331_capture_ready', timeout=180)
            reply('tp Tester 8.5 201 8.5')
            passed('execute if entity @a[name=Tester,nbt={Health:20f,playerGameType:3}]')
            reply('data get entity Tester UUID')
            start = game_time(server)
            control = 'playsound minecraft:block.note_block.harp record @a 4.5 200 4.5 0.5 1.25'
            reply(control)
            reply('summon minecraft:arrow 4.5 202.5 4.5 '
                '{UUID:[I;331,0,0,1],Tags:["h331_arrow"],NoGravity:1b,Motion:[0d,-1d,0d],'
                'item:{id:"minecraft:arrow",count:1}}')
            passed(f'execute if entity {UUID}')
            passed('execute if entity @e[type=minecraft:arrow,tag=h331_arrow,'
                'nbt={UUID:[I;331,0,0,1],NoGravity:1b,Pos:[4.5d,202.5d,4.5d],Motion:[0d,-1d,0d],'
                'inGround:0b,item:{id:"minecraft:arrow",count:1}}]')
            reply(f'data get entity {UUID}')
            reply('tick step 14')
            deadline = time.monotonic()+180
            while game_time(server) < start+14:
                assert time.monotonic() < deadline, 'step timeout'
            assert game_time(server) == start+14
            passed(f'execute if entity {UUID}')
            passed('execute if entity @e[type=minecraft:arrow,tag=h331_arrow,'
                'nbt={UUID:[I;331,0,0,1],inGround:1b,item:{id:"minecraft:arrow",count:1}}]')
            reply(f'data get entity {UUID}')
            reply(control)
            process.wait(timeout=180)
            assert process.returncode == 0
        report = json.loads((folder / 'client-report.json').read_text())
        assert report.get('error') is None, report
        inputs = recorded_sounds(folder)
        subprocess.run([*decoder, 'decode-recorded', inputs, folder / 'sounds.jsonl'], check=True)
        sounds = [json.loads(line) for line in (folder / 'sounds.jsonl').read_text().splitlines()]
        events = [s['event'] for s in sounds]
        assert events == ['minecraft:block.note_block.harp', 'minecraft:entity.arrow.hit',
                          'minecraft:block.note_block.harp'], sounds
        checks.extend(['actual 14 tick interval', 'one hit between two control sounds', 'complete sound roundtrips'])
        assert [s['category'] for s in (sounds[0], sounds[2])] == [2, 2]
        assert [(s['volume_bits'], s['pitch_bits']) for s in (sounds[0], sounds[2])] == [('3f000000', '3fa00000')]*2
        assert sounds[1]['volume_bits'] == '3f800000'
        pitch = struct.unpack('>f', bytes.fromhex(sounds[1]['pitch_bits'].zfill(8)))[0]
        assert 1.0 < pitch < 1.4
        return dict(admission=checks, category=sounds[1]['category'], sounds=sounds,
                    contract_pass=sounds[1]['category'] == 6)
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
    out = (args.out or lab.LOCAL / 'arrow-hit-sound-parity' / f'run-{time.time_ns()}').resolve()
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
    print(f"Arrow block-hit source: {rust['category']} (expected 6); {len(rust['admission'])} admission/control checks passed")
    return 0 if rust['contract_pass'] else 1


if __name__ == '__main__':
    raise SystemExit(main())
