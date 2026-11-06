#!/usr/bin/env python3
"""Bounded live Breeze local-sound isolation, using the existing typed sound client.

Complete wire packets, random seeds/pitch/order, commands and bodies remain raw.
The declared contract compares local-event absence and positive shared receipts.
"""
import argparse
import hashlib
import json
import math
from pathlib import Path
import time

import test_hostile_sound_categories_parity as sound
import test_locator_bar_parity as net
from test_dedicated_server_parity import Server, game_time

PLAYER = 'Alice'
LOCAL = [f'minecraft:entity.breeze.{name}' for name in ['idle_ground', 'idle_air', 'whirl']]
BREEZE = '00000045-0000-0000-0000-000000000001'
COW = '00000045-0000-0000-0000-000000000002'


def program():
    return dict(window_ticks=240, total_ticks=242, player=PLAYER,
                breeze=dict(uuid=BREEZE, position=[8.5, 100.0, 8.5], health=30, no_ai=True, no_gravity=True),
                local_events=LOCAL, local_packet_counts=[0, 0, 0],
                shared=[dict(stage=f'{phase}_{kind}', event=event, source=source, position=position)
                        for phase in ['before', 'after']
                        for kind, event, source, position in [
                            ('transport', 'minecraft:block.note_block.harp', 4, [4.5, 100.0, 4.5]),
                            ('cow', 'minecraft:entity.cow.hurt', 6, [12.5, 100.0, 8.5])]],
                scope='No Breeze local packets during the entire admitted lifetime; shared transport and live cow hurt receipts before/after 240 natural ticks. No exact random schedule, seed, pitch, raw clock or packet-family equality.',
                raw=['complete packets', 'seeds', 'pitch', 'order', 'all commands', 'actor bodies', 'absolute clocks'])


def run(label, command, client_command, build, world, spec):
    directory = build / label
    port = net.server_dir(directory, world)
    server = Server(command, directory, label)
    commands = sound.Commands(server, build / f'{label}-commands.json')
    path = build / f'{label}-wire.txt'
    log = (build / f'{label}-client.log').open('w')
    client = None
    timeline, shared = [], []
    elapsed = 0
    def clock(stage):
        actual = game_time(server)
        timeline.append(dict(stage=stage, absolute_clock=actual, relative_clock=actual-start,
                             recorded_at=sound.now()))
        assert actual-start == elapsed, (stage, actual, start, elapsed)
    def step(ticks):
        nonlocal elapsed
        commands.reply(f'tick step {ticks}')
        elapsed += ticks
        deadline = time.monotonic()+90
        while game_time(server) < start+elapsed:
            assert time.monotonic() < deadline, 'world step stalled'
            time.sleep(.01)
        clock('step-'+str(elapsed))
    def body():
        commands.passed('execute positioned 8.5 100 8.5 if entity @e[type=minecraft:breeze,tag=h069,nbt={UUID:[I;69,0,0,1],Health:30f,NoAI:1b,NoGravity:1b},distance=..0]')
        commands.reply(f'data get entity {BREEZE}')
    try:
        server.wait_for(r'Done \(')
        commands.reply('tick freeze')
        commands.reply('scoreboard objectives add h069 dummy')
        client = net.Client(client_command, port, path, log)
        for line in ['sounds on', 'chat on', 'spawns on', 'join Alice', 'fly Alice on']:
            client.tell(line)
        server.wait_for('Alice joined the game', timeout=120)
        client.sync([PLAYER])
        commands.reply('tp Alice 2.5 100 2.5 0 0')
        commands.passed('execute if entity @a[name=Alice,nbt={UUID:[I;'+sound.uuid_words(PLAYER)+'],Health:20f,playerGameType:1,Pos:[2.5d,100d,2.5d]}]')
        commands.reply('data get entity Alice')
        for x, z in [(8, 8), (12, 8), (2, 2)]:
            for y, block in [(99, 'stone'), (100, 'air'), (115, 'air'), (116, 'stone')]:
                commands.passed(f'execute if loaded {x} {y} {z}')
                commands.passed(f'execute if block {x} {y} {z} {block}')
        start = game_time(server)
        clock('initial')
        commands.reply('summon breeze 8.5 100 8.5 {UUID:[I;69,0,0,1],Tags:["h069"],Health:30f,NoAI:1b,NoGravity:1b,OnGround:1b,PersistenceRequired:1b}')
        body()
        commands.reply('summon cow 12.5 100 8.5 {UUID:[I;69,0,0,2],Tags:["h069_control"],Health:6f,NoAI:1b,NoGravity:1b,PersistenceRequired:1b}')
        for phase in ['before', 'after']:
            if phase == 'after':
                client.tell('step natural-window'); client.sync([PLAYER])
                step(spec['window_ticks'])
                body()
                client.sync([PLAYER]); sound.dump(client, 'window-end')
            for case in [c for c in spec['shared'] if c['stage'].startswith(phase+'_')]:
                client.tell('step '+case['stage']); client.sync([PLAYER])
                if case['stage'].endswith('transport'):
                    commands.reply('playsound minecraft:block.note_block.harp block Alice 4.5 100 4.5 1 1')
                else:
                    old_health, new_health = (6, 5) if phase == 'before' else (5, 4)
                    commands.passed(f'execute positioned 12.5 100 8.5 if entity @e[type=cow,tag=h069_control,nbt={{UUID:[I;69,0,0,2],Health:{old_health}f,NoAI:1b,NoGravity:1b}},distance=..0]')
                    commands.reply(f'data get entity {COW}')
                    for key in ['result', 'success']:
                        commands.reply(f'scoreboard players set {key} h069 -99')
                    commands.reply(f'execute store result score result h069 store success score success h069 run damage {COW} 1 minecraft:out_of_world')
                    for key in ['result', 'success']:
                        text = commands.reply(f'scoreboard players get {key} h069')
                        assert f'{key} has 1 [h069]' in text, text
                    commands.passed(f'execute if entity @e[type=cow,tag=h069_control,nbt={{UUID:[I;69,0,0,2],Health:{new_health}f}}]')
                    step(1)
                    commands.reply(f'data get entity {COW}')
                # Receipt wait only: no repeated action and no extra world ticks.
                deadline = time.monotonic()+30
                while True:
                    client.sync([PLAYER]); sound.dump(client, case['stage'])
                    found = [s for s in sound.raw_sounds(path) if s['stage']==case['stage'] and s['event']==case['event']]
                    if found:
                        break
                    assert time.monotonic() < deadline, ('missing shared receipt', case)
                    time.sleep(.02)
                assert len(found)==1, found
                record = found[0]
                assert record['position']==case['position'], record
                assert math.isfinite(record['volume']) and record['volume']>0
                assert math.isfinite(record['pitch']) and record['pitch']>0
                if case['stage'].endswith('transport'):
                    assert (record['volume_bits'], record['pitch_bits'])==(1065353216,1065353216)
                shared.append({k:record[k] for k in ['stage','event','source','position']})
                clock(case['stage'])
            body()
        assert elapsed==spec['total_ticks']
        commands.passed('execute if entity @a[name=Alice,nbt={UUID:[I;'+sound.uuid_words(PLAYER)+'],Health:20f,playerGameType:1,Pos:[2.5d,100d,2.5d]}]')
        client.sync([PLAYER]); sound.dump(client, 'final')
    finally:
        (build/f'{label}-clocks.json').write_text(json.dumps(timeline, indent=2)+'\n')
        try:
            if client:
                try:
                    sound.dump(client, 'failure-or-final'); client.end()
                finally:
                    client.kill()
        finally:
            server.stop(); log.close()
            records = sound.raw_sounds(path)
            (build/f'{label}-sounds.json').write_text(json.dumps(records, indent=2)+'\n')
    return dict(shared=shared, local_packet_counts=[sum(r['event']==event for r in records) for event in LOCAL],
                total_ticks=elapsed, recorded_at=sound.now(), sound_emission_time=None)


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--work-dir', type=Path, required=True)
    parser.add_argument('--reference-only', action='store_true')
    parser.add_argument('--reference-dir', type=Path)
    args=parser.parse_args()
    build=args.work_dir.resolve(); build.mkdir(parents=True,exist_ok=False)
    spec=program(); (build/'program.json').write_text(json.dumps(spec,indent=2)+'\n')
    script=Path(__file__).read_bytes()
    (build/'manifest.json').write_text(json.dumps(dict(script_sha256=hashlib.sha256(script).hexdigest(),
        declared_at=sound.now(), program_sha256=hashlib.sha256((build/'program.json').read_bytes()).hexdigest()),indent=2)+'\n')
    _,_,java,native,client=net.sides(str(build.relative_to(net.lab.LOCAL)),args.binary.resolve())
    reference=args.reference_dir.resolve() if args.reference_dir else build
    if not args.reference_dir:
        world=sound.generate(java,build)
        data=run('java',java,client,build,world,spec)
        assert data['shared']==spec['shared'] and data['local_packet_counts']==spec['local_packet_counts'],data
        (build/'reference.json').write_text(json.dumps(data,indent=2)+'\n')
    if args.reference_only:
        return
    expected_bytes=(reference/'reference.json').read_bytes(); expected=json.loads(expected_bytes)
    assert (reference/'manifest.json').read_text() and json.loads((reference/'manifest.json').read_text())['script_sha256']==hashlib.sha256(script).hexdigest()
    actual=run('native',native,client,build,reference/'generate/world',spec)
    assert expected_bytes==(reference/'reference.json').read_bytes()
    keys=['shared','local_packet_counts','total_ticks']
    passed=all(expected[k]==actual[k] for k in keys)
    (build/'report.json').write_text(json.dumps(dict(expected=expected,actual=actual,pass_=passed,
        reference_sha256=hashlib.sha256(expected_bytes).hexdigest()),indent=2)+'\n')
    print('Local packet counts:',actual['local_packet_counts'],'expected:',expected['local_packet_counts'])
    print('Shared controls:',actual['shared']==expected['shared'])
    if not passed:
        raise SystemExit(1)


if __name__=='__main__':
    main()
