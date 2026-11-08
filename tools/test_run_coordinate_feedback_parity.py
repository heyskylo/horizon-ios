#!/usr/bin/env python3
"""Completed /test run feedback through the existing real player and raw wire route.

Original Component CODEC objects and packet bodies remain chronological. Whole
elapsed-time reports and batch-start messages are predeclared diagnostics.
"""
import argparse
import hashlib
import json
from pathlib import Path
import time

import net_capture
import test_locator_bar_parity as net
import test_hostile_sound_categories_parity as sound
from test_dedicated_server_parity import Server, game_time

KEYS = ['commands.test.summary', 'commands.test.summary.all_required_passed',
        'test.player.coordinates', 'test.run.coordinates']
SCENES = [('nether-player', 'the_nether', 8.5), ('overworld-player', 'overworld', 24.5),
          ('console', 'overworld', 16.5)]
HELPERS = ['tools/net_capture.py', 'tools/test_locator_bar_parity.py',
           'tools/test_hostile_sound_categories_parity.py', 'tools/test_dedicated_server_parity.py',
           'mods/horizon/src/test/java/dev/horizonlab/parity/LocatorBarClient.java']


def keys(value):
    if isinstance(value, dict):
        return ([value['translate']] if 'translate' in value else []) + [
            k for v in value.values() for k in keys(v)]
    if isinstance(value, list):
        return [k for v in value for k in keys(v)]
    return []


def chats(path, stage):
    rows = []
    for line in path.read_text().splitlines():
        prefix = f'Alice step={stage} chat '
        if line.startswith(prefix):
            value = line[len(prefix):]
            assert not value.startswith('overlay '), line
            rows.append(json.loads(value))
    return rows


def generate(java, build):
    directory = build / 'generate'
    net.server_dir(directory)
    server = Server(java, directory, 'generate')
    commands = sound.Commands(server, build / 'generate-commands.json')
    try:
        server.wait_for(r'Done \(')
        commands.reply('tick freeze')
        for name, value in [('spawn_mobs', 'false'), ('advance_time', 'false'),
                            ('advance_weather', 'false'), ('random_tick_speed', '0'),
                            ('show_advancement_messages', 'false')]:
            commands.reply(f'gamerule {name} {value}')
        for dim in ['overworld', 'the_nether']:
            commands.reply(f'execute in minecraft:{dim} run forceload add 0 0 31 31')
            commands.reply(f'execute in minecraft:{dim} run fill 0 199 0 31 199 31 stone')
            commands.reply(f'execute in minecraft:{dim} run fill 0 200 0 31 215 31 air')
        commands.reply('setworldspawn 4 200 4')
        commands.reply('save-all flush')
    finally:
        server.stop()
    return directory / 'world'


def run(label, command, client_command, build, world, java):
    directory = build / label
    port = net.server_dir(directory, world)
    server = Server(command, directory, label)
    commands = sound.Commands(server, build / (label + '-commands.json'))
    capture = build / (label + '-packets.jsonl')
    listen = net.lab.free_port()
    client = stop = None
    result, clocks = [], []
    log = (build / (label + '-client.log')).open('w')
    wire = build / (label + '-wire.txt')
    try:
        server.wait_for(r'Done \(')
        stop = net_capture.start_background(listen, f'127.0.0.1:{port}', capture, full=True)
        client = net.Client(client_command, listen, wire, log)
        for line in ['chat on', 'join Alice', 'fly Alice on']:
            client.tell(line)
        server.wait_for('Alice joined the game', timeout=120)
        client.sync(['Alice'])
        commands.reply('tick freeze')
        commands.reply('scoreboard objectives add coords dummy')
        for scene, dim, x in SCENES:
            commands.reply(f'execute in minecraft:{dim} run tp Alice {x} 200.0 8.5 30 15')
            client.sync(['Alice'])
            commands.passed('execute if entity @a[name=Alice,type=player,nbt={UUID:[I;' +
                            sound.uuid_words('Alice') + '],Health:20f,playerGameType:1,Pos:[' +
                            f'{x}d,200d,8.5d],Rotation:[30f,15f]' + '}]')
            commands.passed(f'execute as Alice at @s if dimension minecraft:{dim}')
            for level in ['overworld', 'the_nether']:
                commands.passed(f'execute in minecraft:{level} if loaded {int(x)} 200 8')
                commands.passed(f'execute in minecraft:{level} if block {int(x)} 199 8 stone')
            commands.reply('data get entity Alice')
            for name in ['result', 'success']:
                commands.reply(f'scoreboard players set {name} coords 99')
            start = game_time(server)
            client.tell('step ' + scene)
            client.sync(['Alice'])
            action = 'execute store result score result coords store success score success coords run test run minecraft:always_pass'
            if scene == 'console':
                commands.reply(f'execute positioned {x} 200.0 8.5 run ' + action)
            else:
                client.tell('cmd Alice ' + action)
                client.sync(['Alice'])
            for name in ['result', 'success']:
                text = commands.reply(f'scoreboard players get {name} coords')
                assert f'{name} has 1 [coords]' in text, text
            for at in range(1, 9):
                commands.reply('tick step 1')
                deadline = time.monotonic() + 45
                while game_time(server) < start + at:
                    assert time.monotonic() < deadline, ('step stalled', scene, at)
                    time.sleep(.02)
                assert game_time(server) == start + at
            clocks.append(dict(scene=scene, before=start, after=game_time(server), elapsed=8))
            client.sync(['Alice'])
            sound.dump(client, scene)
            # Actual created block and completion, independent of coordinate feedback.
            p = f'{int(x)} 200 11'
            commands.passed(f'execute in overworld if block {p} test_instance_block')
            for field, expected in [('test', 'minecraft:always_pass'), ('status', 'finished')]:
                text = commands.reply(f'execute in overworld run data get block {p} data.{field}')
                assert expected in text, (scene, field, text)
            commands.reply(f'execute in overworld run data get block {p}')
            commands.reply('data get entity Alice')
            client.sync(['Alice'])
            sound.dump(client, scene)
            all_chat = chats(wire, scene)
            selected = [v for v in all_chat if any(k in KEYS for k in keys(v))]
            # Summary must be real before diagnosing its following coordinate messages.
            assert any('commands.test.summary' in keys(v) for v in selected), (scene, all_chat)
            assert any('commands.test.summary.all_required_passed' in keys(v) for v in selected), (scene, all_chat)
            if java:
                expected = KEYS if scene == 'nether-player' else KEYS[:2]
                assert [k for v in selected for k in keys(v) if k in KEYS] == expected, (scene, selected)
            result.append(dict(scene=scene, messages=selected))
        client.end()
        client.kill()
        client = None
    finally:
        (build / (label + '-clocks.json')).write_text(json.dumps(clocks, indent=2) + '\n')
        if client:
            client.kill()
        if stop:
            stop.set()
        server.stop()
        log.close()
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--work-dir', type=Path, required=True)
    parser.add_argument('--reference-only', action='store_true')
    parser.add_argument('--reference-dir', type=Path)
    args = parser.parse_args()
    build = args.work_dir.resolve()
    build.mkdir(parents=True, exist_ok=False)
    manifest = dict(script=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
                    helpers={p: hashlib.sha256((net.lab.ROOT / p).read_bytes()).hexdigest() for p in HELPERS},
                    scenes=[list(s) for s in SCENES], elapsed_ticks=8, strict_keys=KEYS,
                    diagnostics='whole elapsed-ms/batch-start messages and all remaining chat/packet bodies retained; no row mask',
                    declared_at=sound.now())
    (build / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
    _, _, java, native, client = net.sides(str(build), args.binary.resolve())
    reference = args.reference_dir.resolve() if args.reference_dir else build
    if not args.reference_dir:
        world = generate(java, build)
        expected = run('java', java, client, build, world, True)
        (build / 'reference.json').write_text(json.dumps(expected, indent=2) + '\n')
    else:
        old = json.loads((reference / 'manifest.json').read_text())
        assert all(old[k] == manifest[k] for k in ['script', 'helpers', 'scenes', 'elapsed_ticks', 'strict_keys'])
    if args.reference_only:
        return
    frozen = (reference / 'reference.json').read_bytes()
    expected = json.loads(frozen)
    actual = run('native', native, client, build, reference / 'generate/world', False)
    assert (reference / 'reference.json').read_bytes() == frozen
    report = dict(expected=expected, actual=actual, equal=expected == actual,
                  reference_sha256=hashlib.sha256(frozen).hexdigest())
    (build / 'strict-report.json').write_text(json.dumps(report, indent=2) + '\n')
    assert report['equal'], 'admitted completed-test feedback differs'


if __name__ == '__main__':
    main()
