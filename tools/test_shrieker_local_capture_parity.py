#!/usr/bin/env python3
"""A real client jump lands on a shrieker; retain full raw wire packets.

Only the shriek's shared-packet absence, admitted level event and explicit shared
controls form the contract. Seeds, random pitch/order and unrelated counts stay raw.
"""
import argparse
import hashlib
import json
import os
import math
import uuid
import anvil_dump as nbt
from pathlib import Path
import struct
import subprocess
import time

import net_capture
import test_hostile_sound_categories_parity as sound
import test_locator_bar_parity as net
from test_dedicated_server_parity import Server, game_time

PLAYER = 'Alice'
ACTOR = 'Bob'
SHRIEK = 'minecraft:block.sculk_shrieker.shriek'
CONTROL = 'minecraft:block.note_block.pling'
FIELDS = ['event', 'source', 'position', 'volume_bits', 'pitch_bits']
CASES = [('dry', False), ('waterlogged', True)]
HELPERS = ['tools/anvil_dump.py','tools/net_capture.py','tools/test_hostile_sound_categories_parity.py',
           'tools/test_locator_bar_parity.py','tools/test_dedicated_server_parity.py',
           'mods/horizon/src/test/java/dev/horizonlab/parity/LocatorBarClient.java',
           'mods/horizon/src/test/java/dev/horizonlab/parity/VanillaPacketCodecOracle.java']

def helper_hashes():
    return {n: hashlib.sha256((net.lab.ROOT/n).read_bytes()).hexdigest() for n in HELPERS}


def program():
    return dict(cases=CASES, source=[8, 100, 5],
                observer=dict(name=PLAYER,position=[2.5,100.0,2.5]),
                actor=dict(name=ACTOR,start=[8.5,100.5,1.9],end=[8.5,100.5,5.5]),
                initialization='join both clients, then freeze and positively create objective after geometry setup',
                observation_ticks=12, action='existing client arc Bob -1, one world tick per received real movement packet',
                contract='actual stepOn 3007 at source, shared controls before/after; no separate shriek Sound packet',
                raw='all packets/counts/order/seeds/pitch/commands/bodies/absolute clocks',
                sound_emission_time=None)


def run(label, wet, command, client_command, build, world):
    directory = build / label
    port = net.server_dir(directory, world)
    server = Server(command, directory, label)
    commands = sound.Commands(server, build / (label+'-commands.json'))
    listen = net.lab.free_port()
    capture = build / (label+'-packets.jsonl')
    client_log = (build / (label+'-client.log')).open('w')
    client = actor = stop = None
    actor_log=(build/(label+'-actor-client.log')).open('w')
    clocks = []
    def clock(stage, elapsed):
        value = game_time(server)
        clocks.append(dict(stage=stage, absolute=value, relative=value-start, expected=elapsed,
                           recorded_at=sound.now(), sound_emission_time=None))
        assert value-start == elapsed, clocks[-1]
    try:
        server.wait_for(r'Done \(')
        stop = net_capture.start_background(listen, f'127.0.0.1:{port}', capture, full=True)
        client = net.Client(client_command, listen, build/(label+'-wire.txt'), client_log)
        for line in ['sounds on', 'events on', 'chat on', 'join Alice', 'fly Alice on']:
            client.tell(line)
        server.wait_for('Alice joined the game', timeout=120)
        client.sync([PLAYER])
        commands.reply('tp Alice 2.5 100 2.5 0 0')
        commands.passed('execute if entity @a[name=Alice,type=player,nbt={UUID:[I;'+sound.uuid_words(PLAYER)+'],Health:20f,Pos:[2.5d,100d,2.5d],playerGameType:1}]')
        commands.reply('data get entity Alice')
        actor=net.Client(client_command,listen,build/(label+'-actor-wire.txt'),actor_log)
        for line in ['chat on','join Bob','fly Bob on']:actor.tell(line)
        server.wait_for('Bob joined the game',timeout=120);actor.sync([ACTOR])
        commands.reply('tick freeze')
        commands.reply('tp Bob 8.5 100.5 1.9 0 0')
        commands.passed('execute if entity @a[name=Bob,type=player,nbt={UUID:[I;'+sound.uuid_words(ACTOR)+'],Health:20f,Pos:[8.5d,100.5d,1.9d],playerGameType:1}]')
        commands.reply('data get entity Bob')
        client.sync([PLAYER]);actor.sync([ACTOR])
        for z in range(0,6):
            commands.passed(f'execute if loaded 8 100 {z}')
            commands.passed(f'execute if block 8 99 {z} stone')
            commands.passed(f'execute if block 8 101 {z} air')
        for z in range(0,5):
            commands.reply(f'setblock 8 100 {z} stone_slab[type=bottom,waterlogged=false] strict')
            commands.passed(f'execute if block 8 100 {z} stone_slab[type=bottom,waterlogged=false]')
        water = str(wet).lower()
        commands.reply(f'setblock 8 100 5 sculk_shrieker[shrieking=false,can_summon=false,waterlogged={water}] strict')
        commands.passed(f'execute if block 8 100 5 sculk_shrieker[shrieking=false,can_summon=false,waterlogged={water}]')
        created=commands.reply('scoreboard objectives add h065 dummy')
        assert 'Created new objective' in created and 'h065' in created,created
        start = game_time(server)
        controls = []
        for phase in ['before','after']:
            client.tell('step '+phase);client.sync([PLAYER])
            if phase == 'after':
                client.tell('step actual-jump');client.sync([PLAYER])
                def movement_count():
                    try:
                        rows=net_capture.load(capture)
                    except json.JSONDecodeError:
                        return 0  # Proxy may still be appending the final complete raw line.
                    return sum(r.get('dir')=='c2s' and r.get('state')=='play' and r.get('name')=='move_player_pos_rot' for r in rows)
                initial_moves=movement_count()
                actor.tell('step actual-jump');actor.sync([ACTOR])
                actor.tell('arc Bob -1')
                for tick in range(1,13):
                    deadline=time.monotonic()+45
                    while movement_count()<initial_moves+tick:
                        assert time.monotonic()<deadline,('no actual movement input',tick)
                        time.sleep(.02)
                    # Observe real input and pass a server receipt barrier while
                    # frozen, then advance exactly one measured world tick.
                    commands.reply('time query gametime')
                    commands.reply('tick step 1')
                    while game_time(server)<start+tick:
                        assert time.monotonic()<deadline,('step stalled',tick)
                        time.sleep(.02)
                    clock('jump-'+str(tick),tick)
                actor.sync([ACTOR]);client.sync([PLAYER])
                assert movement_count()==initial_moves+12,'unexpected movement count'
                commands.passed(f'execute if block 8 100 5 sculk_shrieker[shrieking=true,can_summon=false,waterlogged={water}]')
                commands.reply('data get entity Bob')  # Full live projection remains a raw diagnostic.
                client.sync([PLAYER]);sound.dump(client,'actual-event')
                client.tell('step after');client.sync([PLAYER])
            for key,initial in [('result',31),('success',17)]:
                commands.reply(f'scoreboard players set {key} h065 {initial}')
            commands.reply('execute store result score result h065 store success score success h065 run playsound '+CONTROL+' block Alice 8.5 101 2.5 1 1')
            for key in ['result','success']:
                text=commands.reply(f'scoreboard players get {key} h065')
                assert f'{key} has 1 [h065]' in text,text
            deadline=time.monotonic()+30
            while True:
                client.sync([PLAYER]);sound.dump(client,phase)
                found=[r for r in sound.raw_sounds(build/(label+'-wire.txt')) if r['stage']==phase and r['event']==CONTROL]
                if found:break
                assert time.monotonic()<deadline,'shared control receipt missing'
                time.sleep(.02)
            assert len(found)==1,found
            row={k:found[0][k] for k in FIELDS}
            assert row==dict(event=CONTROL,source=4,position=[8.5,101.0,2.5],volume_bits=1065353216,pitch_bits=1065353216),row
            controls.append(row);clock(phase,0 if phase=='before' else 12)
        commands.passed('execute if entity @a[name=Bob,type=player,nbt={UUID:[I;'+sound.uuid_words(ACTOR)+'],Health:20f,playerGameType:1}]')
        commands.reply('data get entity Bob')
        commands.passed('execute if entity @a[name=Alice,type=player,nbt={UUID:[I;'+sound.uuid_words(PLAYER)+'],Health:20f,Pos:[2.5d,100d,2.5d],playerGameType:1}]')
        commands.reply('data get entity Alice')
        actor.sync([ACTOR]);sound.dump(actor,'actor-final');actor.end();actor.kill();actor=None
        client.sync([PLAYER]);sound.dump(client,'final')
        commands.reply('scoreboard objectives remove h065')
        commands.reply('tick unfreeze')
    finally:
        (build/(label+'-clocks.json')).write_text(json.dumps(clocks,indent=2))
        try:
            if actor:actor.kill()
            if client:
                try:sound.dump(client,'failure-or-final');client.end()
                finally:client.kill()
        finally:
            server.stop();client_log.close();actor_log.close()
            if stop:stop.set()
    # The actual saved actor is authoritative for position and ground state;
    # retain the live command view separately rather than patching its projection.
    raw=bytearray(hashlib.md5(('OfflinePlayer:'+ACTOR).encode()).digest())
    raw[6]=(raw[6]&15)|48;raw[8]=(raw[8]&63)|128
    identifier=str(uuid.UUID(bytes=bytes(raw)))
    candidates=[directory/'world/playerdata'/f'{identifier}.dat',directory/'world/players/data'/f'{identifier}.dat']
    save=next(path for path in candidates if path.exists());body=nbt.read_dat(save)
    (build/(label+'-actor-save.json')).write_text(json.dumps(body,indent=2))
    assert body['UUID']==('I',[int(x) for x in sound.uuid_words(ACTOR).split(',')]),body['UUID']
    assert body['Health']==('f',20.0) and body['playerGameType']==1,body
    assert body['OnGround']==('b',1),body['OnGround']
    assert all(t=='d' and abs(v-e)<.00001 for (t,v),e in zip(body['Pos'],[8.5,100.5,5.5])),body['Pos']
    typed=(build/(label+'-wire.txt')).read_text()
    assert ' error ' not in typed,typed
    events=[line for line in typed.splitlines() if ' event 3007 ' in line]
    assert len(events)==1 and events[0].endswith(' event 3007 8 100 5 0'),events
    sounds=sound.raw_sounds(build/(label+'-wire.txt'))
    for row in sounds:
        if row['event']==SHRIEK:
            assert row['position']==[8.5,100.5,5.5] and row['volume_bits']==1073741824,row
            assert math.isfinite(row['pitch']) and 0.6<=row['pitch']<=1.0,row
    (build/(label+'-sounds.json')).write_text(json.dumps(sounds,indent=2))
    packets=net_capture.load(capture)
    assert net_capture.play_sessions(packets)==[1,2], 'sequential observer/actor connections required'
    relevant=[r for r in packets if r.get('dir')=='s2c' and r.get('state')=='play' and r.get('name') in ['sound','sound_entity','level_event']]
    (build/(label+'-codec-input.jsonl')).write_text(''.join(json.dumps(r)+'\n' for r in relevant))
    for row in sounds:
        full=bytes.fromhex(row['payload_hex']);identifier,offset=net_capture.read_varint(full,0)
        assert any(r['conn']==1 and r['id']==identifier and r['data']==full[offset:].hex() for r in relevant),'observer/raw connection identity'
    level=[r for r in relevant if r['conn']==1 and r['name']=='level_event' and bytes.fromhex(r['data'])[:4]==struct.pack('>i',3007)]
    assert len(level)==1,level
    payload=bytes.fromhex(level[0]['data'])
    assert len(payload)==17 and payload[-1]==0,level
    # Typed official decoding above admits id/position/data; retain the entire
    # packet and compare its bytes, including global=false, across the pair.
    return dict(controls=controls,level_event=payload.hex(),shriek_packets=sum(r['event']==SHRIEK for r in sounds),
                clocks='relative 0 before action / 12 after action',source_save='UUID/Health20/mode1/typedPos bounded1e-5/OnGround1 admitted',sound_emission_time=None)


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary',type=Path,required=True)
    parser.add_argument('--work-dir',type=Path,required=True)
    parser.add_argument('--reference-only',action='store_true')
    parser.add_argument('--reference-dir',type=Path)
    args=parser.parse_args();build=args.work_dir.resolve();build.mkdir(parents=True,exist_ok=False)
    spec=json.dumps(program(),indent=2)+'\n';(build/'program.json').write_text(spec)
    source=Path(__file__).read_bytes();sha=hashlib.sha256(source).hexdigest()
    (build/'manifest.json').write_text(json.dumps(dict(script_sha256=sha,helpers=helper_hashes(),declared_at=sound.now()),indent=2))
    lock,_,java,native,client=net.sides(str(build),args.binary.resolve())
    _,jars,libs=net.base.verify_artifacts();cp=os.pathsep.join(map(str,[jars['server'],*libs]))
    oracle=net.lab.ROOT/'mods/horizon/src/test/java/dev/horizonlab/parity/VanillaPacketCodecOracle.java'
    subprocess.run(['javac','--release',str(lock['minecraft']['java_major']),'-proc:none','-cp',str(build/'classes')+os.pathsep+cp,'-d',build/'classes',oracle],check=True,capture_output=True)
    codec=['java','-cp',str(build/'classes')+os.pathsep+cp,'dev.horizonlab.parity.VanillaPacketCodecOracle']
    def side(prefix,command,world):
        out=[]
        for name,wet in CASES:
            label=prefix+'-'+name;out.append(run(label,wet,command,client,build,world))
            subprocess.run([*codec,build/(label+'-codec-input.jsonl'),build/(label+'-codec.txt'),'--verbose'],check=True)
        return out
    reference=args.reference_dir.resolve() if args.reference_dir else build
    if not args.reference_dir:
        world=sound.generate(java,build)
        expected=side('java',java,world)
        assert all(r['shriek_packets']==0 for r in expected),expected
        (build/'reference.json').write_text(json.dumps(expected,indent=2)+'\n')
    else:
        assert (reference/'program.json').read_text()==spec
        assert json.loads((reference/'manifest.json').read_text())['script_sha256']==sha
        assert json.loads((reference/'manifest.json').read_text())['helpers']==helper_hashes()
    if args.reference_only:return
    ref=(reference/'reference.json').read_bytes();expected=json.loads(ref)
    assert all(r['shriek_packets']==0 for r in expected)
    actual=side('native',native,reference/'generate/world')
    assert (reference/'reference.json').read_bytes()==ref
    report=dict(expected=expected,actual=actual,pass_contract=expected==actual,
                scope=program()['contract'],script_sha256=sha,reference_sha256=hashlib.sha256(ref).hexdigest())
    (build/'report.json').write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps(report))
    assert report['pass_contract'],'admitted shriek shared-capture mismatch'


if __name__=='__main__':main()
