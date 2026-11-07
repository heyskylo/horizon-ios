#!/usr/bin/env python3
"""Same-level guest death/respawn: world information W stays distinct from landing P.

Uses the existing real connected client and full decoding capture proxy. All wire
bytes remain diagnostic; only the declared default-point fields/actor admissions
are compared. No general packet-family, dimension or landing-policy claim.
"""
import argparse
import hashlib
import json
import math
from pathlib import Path
import re
import shutil
import struct
import time

import anvil_dump as nbt
import lab
import net_capture as wire
import test_locator_bar_parity as net
from test_dedicated_server_parity import game_time

PLAYER = 'Alice'
W, P = [0, 100, 0], [5, 101, 5]
PROGRAM = {
    'world': W, 'personal': P, 'mode': 'survival', 'world_steps': 5, 'join_steps': [2, 2],
    'actions': ['join', 'spawnpoint @s 5 101 5', 'snapshot configured',
                'disconnect/save/copy personal file', 'same UUID rejoin',
                'snapshot before', 'kill @s', 'snapshot dead',
                'ClientCommand action0', 'one delivery world tick', 'receipt', 'snapshot after', 'disconnect/save'],
    'strict': ['complete packet99 fields/count/order relative to Respawn84',
               'actual UUID/type/Health/mode and unchanged pre-death Pos; landing blockP', 'saved forced personal configuration',
               'world point unchanged', 'floor/air/load', 'actual join deltas2; clock delta configured0/before2/dead2/after3'],
    'diagnostic': ['every full raw packet in both directions/order', 'complete player bodies',
                   'absolute clocks', 'exact post-respawn Pos (physical resolver out of scope)', 'unrelated packet fields'],
}


def wait_new(server, start, pattern, timeout=30):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if any(re.search(pattern, line) for line in server.lines[start:]):
            return
        assert server.process.poll() is None, server.lines[-10:]
        time.sleep(.02)
    raise AssertionError((pattern, server.lines[start:][-10:]))


def console(server, text, ack):
    start = len(server.lines)
    server.console(text)
    wait_new(server, start, ack)


def generate(command, build):
    directory = build / 'generate'
    net.server_dir(directory)
    server = net.Server(command, directory, 'generate')
    try:
        server.wait_for(r'Done \(')
        for cmd, ack in [('tick freeze', 'frozen'), ('gamerule spawn_mobs false', 'spawn_mobs'),
                         ('gamerule advance_time false', 'advance_time'),
                         ('gamerule respawn_radius 0', 'respawn_radius'),
                         ('forceload add -16 -16 15 15', 'force loaded'),
                         ('fill -8 99 -8 15 99 15 stone', '576'),
                         ('fill -8 100 -8 15 104 15 air', 'filled'),
                         ('setblock 5 100 5 stone', 'Changed'),
                         ('setworldspawn 0 100 0', 'world spawn'),
                         ('save-all flush', 'Saved the game')]:
            console(server, cmd, ack)
    finally:
        server.stop()
    (directory / 'finished').touch()
    return directory / 'world'


def dump(client, log, label):
    client.tell(f'dump {label}')
    while (line := client.process.stdout.readline()) != f'dumped {label}\n':
        assert line, 'client exited before trace dump'
        log.write(line)


def run(label, command, client_command, build, world):
    directory = build / label
    port = net.server_dir(directory, world)
    server = net.Server(command, directory, label)
    capture = build / f'{label}-packets.jsonl'
    capture.unlink(missing_ok=True)
    proxy = net.free_port()
    stop = wire.start_background(proxy, f'127.0.0.1:{port}', capture, full=True)
    log = (build / f'{label}-client.log').open('w')
    client = None
    try:
        server.wait_for(r'Done \(')
        console(server, 'tick freeze', 'frozen')
        client = net.Client(client_command, proxy, build / f'{label}-client.txt', log)
        client.tell('chat on')
        client.tell('state on')
        client.tell('players on')

        def phase(name):
            client.tell(f'step {name}')

        def cmd(text):
            client.tell(f'send-cmd {PLAYER} {text}')
            client.sync([PLAYER])

        def snapshot(name):
            phase(name)
            cmd(f'data modify storage h330:proof {name}.player set from entity @s')
            cmd(f'execute store result storage h330:proof {name}.clock long 1 run time query gametime')
            cmd(f'execute store success storage h330:proof {name}.type byte 1 run execute if entity @s[type=minecraft:player]')
            dump(client, log, name)

        join_deltas = []

        def join(name):
            phase(name)
            client.tell(f'join {PLAYER}')
            client.sync([PLAYER])
            before = game_time(server)
            console(server, 'tick step 2', 'Stepping')
            deadline = time.monotonic() + 20
            while (after := game_time(server)) < before + 2:
                assert time.monotonic() < deadline, 'join steps did not finish'
                time.sleep(.02)
            assert after == before + 2, (before, after)
            join_deltas.append(after - before)
            (build / f'{label}-join-deltas.json').write_text(json.dumps(join_deltas))
            client.sync([PLAYER])
            dump(client, log, name)

        def leave(name):
            phase(name)
            start = len(server.lines)
            client.tell(f'quit {PLAYER}')
            wait_new(server, start, PLAYER + r'.*(left the game|lost connection)')
            console(server, 'save-all flush', 'Saved the game')
            files = list((directory / 'world').rglob(str(lab.offline_uuid(PLAYER)) + '.dat'))
            assert len(files) == 1, files
            shutil.copy2(files[0], build / f'{label}-{name}-player.dat')

        join('initial')
        cmd('tick freeze')
        cmd('gamemode survival @s')
        cmd('tp @s 0.5 100 0.5 0 0')
        for key, condition in [('world_floor', 'if block 0 99 0 minecraft:stone'),
                               ('world_air', 'if block 0 100 0 minecraft:air'),
                               ('personal_floor', 'if block 5 100 5 minecraft:stone'),
                               ('personal_air', 'if block 5 101 5 minecraft:air')]:
            cmd(f'execute store success storage h330:proof guards.{key} byte 1 run execute {condition}')
        for x, z in [(-16, -16), (-16, 0), (0, -16), (0, 0)]:
            cmd(f'execute store success storage h330:proof guards.loaded_{x}_{z} byte 1 run execute if loaded {x} 100 {z}')
        cmd('spawnpoint @s 5 101 5')
        snapshot('configured')
        leave('personal_saved')
        join('rejoined')
        snapshot('before')
        phase('kill')
        cmd('kill @s')
        snapshot('dead')
        phase('respawn')
        client.tell(f'respawn {PLAYER}')
        client.sync([PLAYER])
        before_delivery = game_time(server)
        console(server, 'tick step 1', 'Stepping')
        deadline = time.monotonic() + 20
        while (after_delivery := game_time(server)) < before_delivery + 1:
            assert time.monotonic() < deadline, 'respawn delivery tick did not finish'
            time.sleep(.02)
        assert after_delivery == before_delivery + 1
        snapshot('after')
        leave('final_saved')
        client.end()
    finally:
        if client is not None:
            # Retain partial client traces on failed prerequisites too.
            if client.process.poll() is None:
                try:
                    dump(client, log, 'failure-path')
                finally:
                    client.kill()
        log.close()
        server.stop()
        stop.set()


def default_point(entry):
    raw = bytes.fromhex(entry['data'])
    size, offset = wire.read_varint(raw, 0)
    dimension = raw[offset:offset + size].decode()
    offset += size
    packed = int.from_bytes(raw[offset:offset + 8], 'big')
    signed = lambda n, bits: n - (1 << bits) if n >= (1 << (bits - 1)) else n
    pos = [signed(packed >> 38, 26), signed(packed & 4095, 12),
           signed((packed >> 12) & 0x3ffffff, 26)]
    angles = raw[offset + 8:offset + 16]
    assert len(angles) == 8 and offset + 16 == len(raw), 'complete packet99 required'
    yaw, pitch = struct.unpack('>ff', angles)
    return {'dimension': dimension, 'pos': pos, 'yaw': yaw, 'pitch': pitch,
            'angle_bits': angles.hex(), 'wire': raw.hex()}


def facts(build, label):
    storage = list((build / label / 'world').rglob('command_storage.dat'))
    assert len(storage) == 1, storage
    data = nbt.read_dat(storage[0])['data']['contents']['proof']
    result = {'setup/join-deltas': json.loads((build / f'{label}-join-deltas.json').read_text())}
    result.update({f'guard/{k}': v for k, v in data['guards'].items()})
    for phase in ['configured', 'before', 'dead', 'after']:
        p = data[phase]['player']
        for key in ['UUID', 'Health', 'playerGameType', 'Pos', 'respawn']:
            if phase == 'after' and key == 'Pos':
                result['after/landing-block'] = [math.floor(v[1]) for v in p['Pos']]
            else:
                result[f'{phase}/{key}'] = p.get(key)
        result[f'{phase}/type'] = data[phase]['type']
        result[f'{phase}/clock-delta'] = data[phase]['clock'][1] - data['configured']['clock'][1]
    for name in ['personal_saved', 'final_saved']:
        p = nbt.read_dat(build / f'{label}-{name}-player.dat')
        for key in ['UUID', 'Health', 'playerGameType', 'respawn']:
            result[f'{name}/{key}'] = p.get(key)
    level = nbt.read_dat(build / label / 'world/level.dat')['Data']
    result['world/spawn'] = level['spawn']
    entries = wire.load(build / f'{label}-packets.jsonl')
    sessions = wire.play_sessions(entries)
    assert len(sessions) == 2, sessions
    for n, conn in enumerate(sessions):
        play = [e for e in entries if e['conn'] == conn and e.get('state') == 'play']
        inbound = [e for e in play if e['dir'] == 's2c']
        respawns = [i for i, e in enumerate(inbound) if e['id'] == 84]
        points = [(i, default_point(e)) for i, e in enumerate(inbound) if e['id'] == 99]
        result[f'network/{n}/initial-points'] = [p for i, p in points if not respawns or i < respawns[0]]
        result[f'network/{n}/respawns'] = len(respawns)
        result[f'network/{n}/after-points'] = [p for i, p in points if respawns and i > respawns[0]]
        result[f'network/{n}/respawn-actions'] = [e['data'] for e in play if e['dir'] == 'c2s' and e['id'] == 12]
    return json.loads(json.dumps(result))


def validate_java(value):
    words = list(struct.unpack('>4i', lab.offline_uuid(PLAYER).bytes))
    for k, v in value.items():
        if k.startswith('guard/') or k.endswith('/type'):
            assert v == ['b', 1], (k, v)
        elif k.endswith('/clock-delta'):
            assert v == (0 if k.startswith('configured/') else 3 if k.startswith('after/') else 2), (k, v)
        elif k.endswith('/UUID'):
            assert v == ['I', words], (k, v)
        elif k.endswith('/Health'):
            assert v == ['f', 0.0 if k.startswith('dead/') else 20.0], (k, v)
        elif k.endswith('/playerGameType'):
            assert v == 0, (k, v)
        elif k.endswith('/Pos'):
            expected = [.5, 100., .5] if k.startswith('configured/') else [5.5, 101., 5.5] if k.startswith('after/') else [.5, 100., .5]
            assert v == [['d', x] for x in expected], (k, v)
    assert value['setup/join-deltas'] == [2, 2]
    assert value['network/0/respawns'] == 0
    assert value['network/1/respawns'] == 1
    assert value['network/0/respawn-actions'] == []
    assert value['network/1/respawn-actions'] == ['00']
    for n in [0, 1]:
        initial = value[f'network/{n}/initial-points']
        assert len(initial) == 1, initial
        assert initial[0]['dimension'] == 'minecraft:overworld' and initial[0]['pos'] == W
    assert value['network/0/after-points'] == []
    after = value['network/1/after-points']
    assert len(after) == 2 and after == value['network/1/initial-points'] * 2, after
    assert value['after/landing-block'] == P
    personal = {'dimension': 'minecraft:overworld', 'pos': ['I', P], 'forced': ['b', 1],
                'pitch': ['f', 0.0], 'yaw': ['f', 0.0]}
    for k, v in value.items():
        if k.endswith('/respawn'):
            assert v == personal, (k, v)
    assert value['world/spawn'] == {'dimension': 'minecraft:overworld', 'pos': ['I', W],
                                   'pitch': ['f', 0.0], 'yaw': ['f', 0.0]}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--side', choices=['java', 'native', 'both'], default='both')
    parser.add_argument('--label')
    parser.add_argument('--binary', type=Path)
    parser.add_argument('--build-name', default='respawn-world-point-parity')
    parser.add_argument('--reference', type=Path)
    args = parser.parse_args()
    net.PLAYERS = [PLAYER]
    net.PROPERTIES.update({'gamemode': 'survival', 'force-gamemode': 'false',
                           'view-distance': '2', 'simulation-distance': '2'})
    _, build, java, native, client = net.sides(args.build_name, args.binary.resolve() if args.binary else None)
    (build / 'program.json').write_text(json.dumps(PROGRAM, indent=2, sort_keys=True) + '\n')
    world = build / 'generate/world'
    # A generate step that died leaves a world that must not be reused.
    if not (build / 'generate/finished').exists():
        world = generate(java, build)
    reference = args.reference
    for side in (['java', 'native'] if args.side == 'both' else [args.side]):
        label = args.label or side
        if args.side == 'both' and args.label:
            label += '-' + side
        assert reference is None or reference.resolve() != (build / f'{label}-facts.json').resolve(), \
            'the reference must remain a separate immutable input'
        run(label, java if side == 'java' else native, client, build, world)
        value = facts(build, label)
        target = build / f'{label}-facts.json'
        target.write_text(json.dumps(value, indent=2, sort_keys=True) + '\n')
        if side == 'java':
            validate_java(value)
            if reference:
                assert target.read_bytes() == reference.read_bytes(), 'fresh reference bytes changed'
            reference = target
        if reference:
            expected = json.loads(reference.read_text())
            assert value.keys() == expected.keys(), 'declared labels changed'
            differences = [{'label': k, 'expected': expected[k], 'actual': value[k]}
                           for k in sorted(value) if expected[k] != value[k]]
            report = {'records': len(value), 'passed': len(value) - len(differences), 'differences': differences,
                      'reference_sha256': hashlib.sha256(reference.read_bytes()).hexdigest()}
            (build / f'{label}-report.json').write_text(json.dumps(report, indent=2) + '\n')
            print(f'{label}: {report["passed"]}/{report["records"]}')
            if differences:
                raise SystemExit(1)
        else:
            print(f'{label}: {len(value)} declared facts')



if __name__ == '__main__':
    main()
