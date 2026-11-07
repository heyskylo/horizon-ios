#!/usr/bin/env python3
"""Real dedicated Game/Java spawner warning, through the existing typed network client."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import time

import test_locator_bar_parity as net
from test_dedicated_server_parity import Server, game_time

PLAYER = 'Alice'
SPAWNER = '00000080-0000-0000-0000-000000000001'
WARNING = 'minecraft:block.trial_spawner.about_to_spawn_item'
CONTROL = 'minecraft:block.note_block.pling'
STAGES = [('at0', 0), ('at1', 1), ('at3', 3), ('at4', 4), ('repeat4', 4), ('at5', 5)]


def words(name):
    raw = bytearray(hashlib.md5(('OfflinePlayer:' + name).encode()).digest())
    raw[6] = (raw[6] & 15) | 48
    raw[8] = (raw[8] & 63) | 128
    return [int.from_bytes(raw[i:i+4], 'big', signed=True) for i in range(0, 16, 4)]


def program():
    player_uuid = ','.join(map(str, words(PLAYER)))
    guards = [
        f'execute if entity @s[type=player,name={PLAYER},nbt={{UUID:[I;{player_uuid}],Health:20f,Pos:[4.5d,100d,4.5d],playerGameType:1}}]',
        'execute if loaded 6 102 4', 'execute if block 4 99 4 stone',
        'execute if block 6 102 4 air',
        f'execute if entity @e[type=ominous_item_spawner,nbt={{UUID:[I;128,0,0,1],Pos:[6.25d,102.75d,4.875d],item:{{id:"minecraft:diamond",count:1}},spawn_item_after_ticks:40L}}]',
    ]
    return {'player': PLAYER, 'stages': STAGES, 'guards': guards,
            'guard_translations': ['commands.execute.conditional.pass_count' if i in (0,4) else 'commands.execute.conditional.pass' for i in range(len(guards))],
            'step_barrier': 'client receipt before every requested world-step batch',
            'summon': 'summon ominous_item_spawner 6.25 102.75 4.875 {UUID:[I;128,0,0,1],item:{id:"minecraft:diamond",count:1},spawn_item_after_ticks:40L}',
            'sound_contract': ['stage', 'event', 'source', 'source_name', 'position', 'volume', 'pitch', 'volume_bits', 'pitch_bits'],
            'raw_diagnostics': 'all sound seeds/payloads, all chat, absolute clocks and available stage timestamps; emission time unknown'}


def dump(client, label):
    client.tell('dump ' + label)
    while (line := client.process.stdout.readline()) != 'dumped ' + label + '\n':
        if not line:
            raise AssertionError('network client exited before dump')
        client.log.write(line)


def wait_new(server, start, expression, timeout=60):
    until = time.monotonic() + timeout
    while time.monotonic() < until:
        if any(re.search(expression, s) for s in server.lines[start:]):
            return
        if server.process.poll() is not None:
            break
        time.sleep(.02)
    raise AssertionError(f'no new {expression}: {server.lines[start:][-8:]}')


def generate(command, build):
    directory = build / 'generate'
    net.server_dir(directory)
    server = Server(command, directory, 'generate')
    try:
        server.wait_for(r'Done \(')
        for cmd, ack in [('gamerule spawn_mobs false', 'spawn_mobs'),
                         ('gamerule advance_time false', 'advance_time'),
                         ('gamerule respawn_radius 0', 'respawn_radius'),
                         ('forceload add 0 0 15 15', 'force loaded')]:
            start = len(server.lines); server.console(cmd); wait_new(server, start, ack)
        # Readiness only, before actors or the timed program.
        until = time.monotonic() + 60
        while True:
            start = len(server.lines); server.console('execute if loaded 4 100 4'); game_time(server)
            if any('Test passed' in s for s in server.lines[start:]):
                break
            assert time.monotonic() < until, 'generation chunk did not become ready'
            time.sleep(.05)
        for cmd, ack in [('fill 0 99 0 15 99 15 stone', '256'),
                         ('setworldspawn 4 100 4', 'world spawn'), ('save-all flush', 'Saved the game')]:
            start = len(server.lines); server.console(cmd); wait_new(server, start, ack)
    finally:
        server.stop()
    return directory / 'world'


def run(label, command, client_command, build, world, script):
    directory = build / label
    port = net.server_dir(directory, world)
    server = Server(command, directory, label)
    raw = build / (label + '-wire.txt')
    log = (build / (label + '-client.log')).open('w')
    client = None
    timeline = []
    def mark(stage, **data):
        timeline.append({'stage': stage, 'wall_time': time.time(), 'monotonic': time.monotonic(), **data})
    try:
        server.wait_for(r'Done \(')
        client = net.Client(client_command, port, raw, log)
        for line in ['sounds on', 'chat on', 'events on', 'join ' + PLAYER, 'fly Alice on']:
            client.tell(line)
        server.wait_for('Alice joined the game', timeout=120)
        client.sync([PLAYER])
        def cmd(text, stage):
            client.tell('step ' + stage)
            client.tell('cmd Alice ' + text)
            client.sync([PLAYER])
            mark(stage, command=text)
        cmd('tick freeze', 'freeze')
        cmd('tp @s 4.5 100 4.5 0 0', 'position')
        cmd('playsound ' + CONTROL + ' neutral Alice 4.5 100 4.5 1 1', 'sound-control')
        cmd(script['summon'], 'summon')
        start_clock = game_time(server)
        previous = 0
        for stage, at in STAGES:
            client.tell('step ' + stage)
            client.sync([PLAYER])
            if at != previous:
                server.console(f'tick step {at - previous}')
                until = time.monotonic() + 30
                while game_time(server) < start_clock + at:
                    assert time.monotonic() < until, 'world steps did not finish'
                    time.sleep(.01)
            actual_clock = game_time(server)
            assert actual_clock - start_clock == at, (stage, actual_clock, start_clock)
            client.sync([PLAYER])
            mark(stage, clock=actual_clock, relative_clock=actual_clock-start_clock, sound_emission_time=None)
            for i, guard in enumerate(script['guards']):
                cmd(guard, stage + '-guard' + str(i))
            cmd('data get entity ' + SPAWNER, stage + '-body')
            cmd('data get entity Alice', stage + '-player-body')
            dump(client, stage)
            previous = at
    finally:
        (build / (label + '-stages.json')).write_text(json.dumps(timeline, indent=2) + '\n')
        try:
            if client:
                try:
                    dump(client, 'failure-or-final')
                    client.end()
                finally:
                    client.kill()
        finally:
            server.stop()
            log.close()
    records = []
    chats = {}
    for line in raw.read_text().splitlines():
        who, rest = line.split(' step=', 1)
        stage, kind, data = rest.split(' ', 2)
        assert who == PLAYER
        if kind == 'sound-wire':
            value = json.loads(data)
            records.append({'stage': stage, **value})
        elif kind == 'chat':
            chats.setdefault(stage, []).append(json.loads(data))
    for stage, _ in STAGES:
        for i in range(len(script['guards'])):
            rows = chats[stage + '-guard' + str(i)]
            key = script['guard_translations'][i]
            assert any(r.get('translate') == key and (key.endswith('.pass') or r.get('with') == [1]) for r in rows), (stage, i, rows)
    assert not any(' error ' in s for s in raw.read_text().splitlines()), 'client error in raw transcript'
    (build / (label + '-sounds.json')).write_text(json.dumps(records, indent=2) + '\n')
    fields = script['sound_contract']
    selected = [{k: r[k] for k in fields} for r in records if r['event'] in (WARNING, CONTROL)]
    expected = [dict(stage='sound-control', event=CONTROL, source=6, source_name='NEUTRAL', position=[4.5,100.0,4.5], volume=1.0, pitch=1.0, volume_bits=1065353216, pitch_bits=1065353216),
                dict(stage='at4', event=WARNING, source=6, source_name='NEUTRAL', position=[6.5,102.5,4.5], volume=1.0, pitch=1.0, volume_bits=1065353216, pitch_bits=1065353216)]
    # Warning must actually exist with every non-category field correct, even in the old binary.
    assert len(selected) == 2
    assert selected[0] == expected[0], ("independent neutral sound control", selected[0])
    for a, e in zip(selected, expected):
        assert {k:v for k,v in a.items() if k not in ('source','source_name')} == {k:v for k,v in e.items() if k not in ('source','source_name')}, (a,e)
    return selected


def main():
    # The parity runner passes no arguments: net.sides builds the game, and the folder has a default.
    p=argparse.ArgumentParser();p.add_argument('--binary',type=Path);p.add_argument('--work-dir',type=Path);p.add_argument('--reference-only',action='store_true');p.add_argument('--reference',type=Path);a=p.parse_args()
    build=(a.work_dir or net.lab.LOCAL/'ominous-warning-sound-parity').resolve();build.mkdir(parents=True,exist_ok=True)
    script=program();(build/'program.json').write_text(json.dumps(script,indent=2)+'\n')
    name=str(build.relative_to(net.lab.LOCAL));_,_,java,native,client=net.sides(name,a.binary.resolve() if a.binary else None)
    world=generate(java,build)
    reference=run('java',java,client,build,world,script)
    (build/'reference.json').write_text(json.dumps(reference,indent=2)+'\n')
    assert all(r['source']==6 and r['source_name']=='NEUTRAL' for r in reference)
    if a.reference_only:return
    if a.reference:assert a.reference.read_bytes()==(build/'reference.json').read_bytes(),'fresh Java reference changed'
    actual=run('native',native,client,build,world,script)
    report={'expected':reference,'actual':actual,'pass':reference==actual,'scope':script['sound_contract']}
    (build/'report.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2))
    if reference!=actual:raise SystemExit(1)

if __name__=='__main__':main()
