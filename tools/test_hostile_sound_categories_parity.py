#!/usr/bin/env python3
"""Live hurt/death sound categories, through the existing typed network capture.

Seeds, pitch, payloads, packet order, chat, bodies and real clocks stay raw.
Only the predeclared event/category/position contract is compared across sides.
"""
import argparse
import datetime
import hashlib
import json
import math
from pathlib import Path
import re
import time

import test_locator_bar_parity as net
from test_dedicated_server_parity import Server, game_time

PLAYER = 'Alice'
FAMILIES = ['bogged', 'breeze', 'endermite', 'parched', 'warden', 'zombie_villager', 'vex', 'wither', 'giant', 'zombie', 'cow']
CONTROLS = [
    ('block', 'minecraft:block.note_block.harp', 'block', 4),
    ('player', 'minecraft:entity.player.hurt', 'player', 7),
    ('record', 'minecraft:music_disc.13', 'record', 2),
    ('override', 'minecraft:entity.warden.hurt', 'neutral', 6),
]


def now():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()


def uuid_words(name):
    raw = bytearray(hashlib.md5(('OfflinePlayer:' + name).encode()).digest())
    raw[6] = (raw[6] & 15) | 48
    raw[8] = (raw[8] & 63) | 128
    return ','.join(str(int.from_bytes(raw[i:i+4], 'big', signed=True)) for i in range(0, 16, 4))


def actor(index):
    return f'00000044-0000-0000-0000-{index:012x}'


def program():
    expected = [dict(stage='control_' + label, event=event, source=source, position=[4.5, 100.0, 4.5])
                for label, event, _, source in CONTROLS]
    for kind in FAMILIES:
        family = 'hostile' if kind == 'giant' else kind
        expected.append(dict(stage=kind, event=f'minecraft:entity.{family}.hurt', source=6 if kind == 'cow' else 5,
                             position=[8.5, 100.0, 8.5]))
    expected.append(dict(stage='giant_death', event='minecraft:entity.hostile.death', source=5, position=[8.5, 100.0, 8.5]))
    return dict(player=PLAYER, families=FAMILIES, controls=CONTROLS, expected=expected,
                contract=['stage', 'event', 'source', 'position'], world_ticks=12,
                damage='1 minecraft:out_of_world (fresh Health6→5); fresh giant death uses100',
                diagnostic_fields=['source_name', 'volume', 'pitch', 'volume_bits', 'pitch_bits', 'seed', 'payload_hex', 'receipt_order'],
                sound_emission_time=None, packet_receipt_time=None,
                scope='Positional sound category and admitted live actor/action; no exact random pitch/seed/trajectory or packet-family claim')


def dump(client, label):
    client.tell('dump ' + label)
    while (line := client.process.stdout.readline()) != 'dumped ' + label + '\n':
        if not line:
            raise AssertionError('client exited before dump')
        client.log.write(line)


def raw_sounds(path):
    out = []
    if path.exists():
        for order, line in enumerate(path.read_text().splitlines()):
            who, rest = line.split(' step=', 1)
            stage, kind, data = rest.split(' ', 2)
            assert who == PLAYER
            if kind == 'sound-wire':
                out.append(dict(stage=stage, receipt_order=order, **json.loads(data)))
    return out


class Commands:
    def __init__(self, server, path):
        self.server, self.path, self.serial, self.rows = server, path, 0, []

    def reply(self, command):
        self.serial += 1
        start = len(self.server.lines)
        row = dict(command=command, sent_at=now(), receipt_at=None, lines=[])
        self.rows.append(row)
        self.server.console(command)
        self.server.console(f'data modify storage h068:barrier value set value {self.serial}')
        self.server.console('data get storage h068:barrier value')
        try:
            self.server.wait_for(rf'h068:barrier.*contents: {self.serial}$', timeout=90)
            row['receipt_at'] = now()
            return '\n'.join(self.server.lines[start:])
        finally:
            row['lines'] = self.server.lines[start:]
            self.path.write_text(json.dumps(self.rows, indent=2) + '\n')

    def passed(self, command):
        text = self.reply(command)
        assert 'Test passed' in text, (command, text)


def generate(java, build):
    directory = build / 'generate'
    net.server_dir(directory)
    server = Server(java, directory, 'generate')
    commands = Commands(server, build / 'generate-commands.json')
    try:
        server.wait_for(r'Done \(')
        for rule in ['spawn_mobs', 'advance_time', 'mob_griefing']:
            commands.reply(f'gamerule {rule} false')
        commands.reply('gamerule respawn_radius 0')
        commands.reply('forceload add 0 0 15 15')
        deadline = time.monotonic() + 90
        while 'Test passed' not in commands.reply('execute if loaded 8 100 8'):
            assert time.monotonic() < deadline, 'generation readiness'
            time.sleep(.05)
        assert '256' in commands.reply('fill 0 99 0 15 99 15 stone')
        commands.reply('fill 0 100 0 15 115 15 air')
        assert '256' in commands.reply('fill 0 116 0 15 116 15 stone')
        commands.reply('setworldspawn 2 100 2')
        commands.reply('save-all flush')
    finally:
        server.stop()
    return directory / 'world'


def run(label, command, client_command, build, world, spec):
    directory = build / label
    port = net.server_dir(directory, world)
    server = Server(command, directory, label)
    commands = Commands(server, build / (label + '-commands.json'))
    path = build / (label + '-wire.txt')
    client_log = (build / (label + '-client.log')).open('w')
    client, timeline, checks = None, [], []
    def mark(stage, **data):
        timeline.append(dict(stage=stage, recorded_at=now(), monotonic_ns=time.monotonic_ns(), sound_emission_time=None, **data))
    try:
        server.wait_for(r'Done \(')
        commands.reply('tick freeze')
        commands.reply('scoreboard objectives add h068 dummy')
        client = net.Client(client_command, port, path, client_log)
        for line in ['sounds on', 'chat on', 'spawns on', 'join ' + PLAYER, 'fly Alice on']:
            client.tell(line)
        server.wait_for('Alice joined the game', timeout=120)
        client.sync([PLAYER])
        commands.reply('tp Alice 2.5 100 2.5 0 0')
        commands.passed('execute if entity @a[name=Alice,nbt={UUID:[I;' + uuid_words(PLAYER) + '],Health:20f,playerGameType:1,Pos:[2.5d,100d,2.5d]}]')
        commands.reply('data get entity Alice')
        for y, block in [(99, 'stone'), (100, 'air'), (112, 'air'), (115, 'air'), (116, 'stone')]:
            commands.passed(f'execute if loaded 8 {y} 8')
            commands.passed(f'execute if block 8 {y} 8 {block}')
        start = game_time(server)
        elapsed = 0
        for case in spec['expected']:
            stage, event = case['stage'], case['event']
            client.tell('step ' + stage); client.sync([PLAYER])
            mark(stage, game_time=game_time(server), relative_clock=elapsed)
            if stage.startswith('control_'):
                _, _, category, _ = next(c for c in CONTROLS if 'control_' + c[0] == stage)
                commands.reply(f'playsound {event} {category} Alice 4.5 100 4.5 1 1')
            else:
                kind = 'giant' if stage == 'giant_death' else stage
                index = len(FAMILIES) + 1 if stage == 'giant_death' else FAMILIES.index(kind) + 1
                uid = actor(index)
                tag = 'h068_' + stage
                commands.reply(f'summon minecraft:{kind} 8.5 100 8.5 {{UUID:[I;68,0,0,{index}],Tags:["{tag}"],Health:6f,NoAI:1b,NoGravity:1b,PersistenceRequired:1b,Invul:0}}')
                selector = f'@e[type=minecraft:{kind},tag={tag},nbt={{UUID:[I;68,0,0,{index}],Health:6f,NoAI:1b,NoGravity:1b}},distance=..0]'
                commands.passed('execute positioned 8.5 100 8.5 if entity ' + selector)
                commands.reply(f'data get entity {uid}')
                for key in ['hit_result', 'hit_success']:
                    commands.reply(f'scoreboard players set {key} h068 -99')
                amount = 100 if stage == 'giant_death' else 1
                commands.reply(f'execute store result score hit_result h068 store success score hit_success h068 run damage {uid} {amount} minecraft:out_of_world')
                for key in ['hit_result', 'hit_success']:
                    text = commands.reply(f'scoreboard players get {key} h068')
                    assert f'{key} has 1 [h068]' in text, (key, text)
                if stage != 'giant_death':
                    commands.passed(f'execute if entity @e[type=minecraft:{kind},tag={tag},nbt={{UUID:[I;68,0,0,{index}],Health:5f}}]')
                commands.reply(f'data get entity {uid}')
                commands.reply('tick step 1')
                elapsed += 1
                deadline = time.monotonic() + 60
                while game_time(server) < start + elapsed:
                    assert time.monotonic() < deadline, 'world step stalled'
                    time.sleep(.01)
                assert game_time(server) == start + elapsed
                if stage != 'giant_death':
                    commands.passed(f'execute if entity @e[type=minecraft:{kind},tag={tag},nbt={{UUID:[I;68,0,0,{index}],Health:5f,NoAI:1b,NoGravity:1b}}]')
                commands.reply(f'data get entity {uid}')
            # Wait for receipt only: never retry damage or add a world tick.
            deadline = time.monotonic() + 30
            while True:
                client.sync([PLAYER]); dump(client, stage)
                matches = [r for r in raw_sounds(path) if r['stage'] == stage and r['event'] == event]
                if matches:
                    break
                assert time.monotonic() < deadline, (stage, 'no expected hurt/control sound')
                time.sleep(.02)
            assert len(matches) == 1, (stage, matches)
            sound = matches[0]
            assert sound['position'] == case['position'], (stage, sound)
            assert math.isfinite(sound['volume']) and sound['volume'] > 0
            assert math.isfinite(sound['pitch']) and sound['pitch'] > 0
            if stage.startswith('control_'):
                assert (sound['volume_bits'], sound['pitch_bits']) == (1065353216, 1065353216)
            checks.append(stage)
            mark(stage + '-received', game_time=game_time(server), relative_clock=elapsed)
        assert game_time(server) - start == spec['world_ticks']
    finally:
        (build / (label + '-stages.json')).write_text(json.dumps(timeline, indent=2) + '\n')
        try:
            if client:
                try:
                    dump(client, 'failure-or-final'); client.end()
                finally:
                    client.kill()
        finally:
            server.stop(); client_log.close()
            (build / (label + '-sounds.json')).write_text(json.dumps(raw_sounds(path), indent=2) + '\n')
    records = raw_sounds(path)
    names = {c['event'] for c in spec['expected']}
    contract = [{k: r[k] for k in spec['contract']} for r in records if r['event'] in names]
    assert len(contract) == len(spec['expected']), (label, contract)
    return dict(contract=contract, admission=checks, total_world_ticks=elapsed,
                recorded_at=now(), sound_emission_time=None, packet_receipt_time=None)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path)
    parser.add_argument('--work-dir', type=Path)
    parser.add_argument('--reference-only', action='store_true')
    parser.add_argument('--reference-dir', type=Path)
    args = parser.parse_args()
    build = (args.work_dir or net.lab.LOCAL / 'hostile-sound-categories' / f'run-{time.time_ns()}').resolve()
    build.mkdir(parents=True, exist_ok=False)
    spec = program(); (build / 'program.json').write_text(json.dumps(spec, indent=2) + '\n')
    name = str(build.relative_to(net.lab.LOCAL))
    _, _, java, native, client = net.sides(name, args.binary.resolve() if args.binary else None)
    reference = args.reference_dir.resolve() if args.reference_dir else build
    if not args.reference_dir:
        world = generate(java, build)
        data = run('java', java, client, build, world, spec)
        assert data['contract'] == spec['expected'], data
        (build / 'reference.json').write_text(json.dumps(data, indent=2) + '\n')
    if args.reference_only:
        return
    expected_bytes = (reference / 'reference.json').read_bytes()
    expected = json.loads(expected_bytes)
    assert expected['contract'] == spec['expected']
    actual = run('native', native, client, build, reference / 'generate/world', spec)
    assert expected_bytes == (reference / 'reference.json').read_bytes()
    report = dict(expected=expected, actual=actual, pass_=expected['contract'] == actual['contract'],
                  reference_sha256=hashlib.sha256(expected_bytes).hexdigest())
    (build / 'report.json').write_text(json.dumps(report, indent=2) + '\n')
    print('Sound category contract:', sum(a == b for a, b in zip(expected['contract'], actual['contract'])), '/', len(spec['expected']))
    if not report['pass_']:
        raise SystemExit(1)


if __name__ == '__main__':
    main()
