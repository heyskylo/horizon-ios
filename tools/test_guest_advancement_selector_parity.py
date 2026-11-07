#!/usr/bin/env python3
"""Two real network players: selected-player advancement progress, zero observation ticks.

Canonical records keep every scripted command's resolved feedback/styles and exact
stored result/success/branch sentinels, typed identity/scene/clock facts and independently
observed actual model progress. Java uses phase save files; native uses the opt-in
read-only existing guest save-loop trace. Final disconnect files cross-check both. Complete wire components, storage, player NBT and
saved advancement timestamps remain raw diagnostics (no timestamp/reopen claim).
"""
import argparse
import json
import hashlib
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time

import anvil_dump as nbt
import test_locator_bar_parity as net
from test_disconnect_crafting_parity import generate, offline, uuid_words, wait_new

A, B = 'H264Alice', 'H264Bob'
ROOT, STONE = 'minecraft:story/root', 'minecraft:story/mine_stone'
KNOWN = {ROOT: 'crafting_table', STONE: 'get_stone'}


def program(unknown=False, packet=True, model=True, clock_file=True):
    ops = []
    def cmd(text, key=None):
        ops.append({'op': 'cmd', 'cmd': text, 'label': key or f'command{len(ops):04}'})
    def store(path, command, kind='int'):
        if model and clock_file and kind=='long' and command=='time query gametime':
            ops.append({'op':'world_clock','key':path,'label':f'command{len(ops):04}'})
        else:
            cmd(f'execute store result storage h264:proof {path} {kind} 1 run {command}')
    cmd('tick freeze')
    for text in ['gamerule spawn_mobs false', 'gamerule advance_time false',
                 'gamerule announce_advancements false', 'gamerule log_admin_commands false',
                 f'deop {B}', 'scoreboard objectives add h264 dummy',
                 'scoreboard players set result h264 -99',
                 'scoreboard players set success h264 -99',
                 'scoreboard players set branch h264 -99']:
        cmd(text)
    for i, name in enumerate([A, B]):
        cmd(f'gamemode creative {name}')
        cmd(f'tp {name} {4.5+i*2} 100 4.5 0 0')
        cmd(f'clear {name}')
        for identifier in KNOWN:
            cmd(f'advancement revoke {name} only {identifier}')
    cmd('summon pig 8.5 100 8.5 {UUID:[I;264,0,0,1],Tags:["h264_nonplayer"],NoAI:1b,NoGravity:1b}')
    store('start', 'time query gametime', 'long')
    phases = [
        ('empty', [], [False, False, False, False]),
        ('root_B', [f'advancement grant {B} only {ROOT} crafting_table'], [False, False, True, False]),
        ('both_root', [f'advancement grant {A} only {ROOT}'], [True, False, True, False]),
        ('revoke_B', [f'advancement revoke {B} only {ROOT} crafting_table'], [True, False, False, False]),
        ('stone_B', [f'advancement grant {B} only {STONE} get_stone'], [True, False, False, True]),
        ('root_B_again', [f'advancement grant {B} only {ROOT}'], [True, False, True, True]),
    ]
    for phase, changes, state in phases:
        for text in changes:
            cmd(text)
        for i, name in enumerate([A, B]):
            words = ','.join(map(str, uuid_words(name)))
            cmd(f'execute if entity @a[name={name},type=player,nbt={{UUID:[I;{words}],Health:20f,Pos:[{4.5+i*2}d,100d,4.5d]}}]')
            cmd(f'data modify storage h264:proof {phase}.player{i} set from entity {name}')
        for i,(x,z) in enumerate([(0,0),(0,16),(16,0),(16,16)]):
            cmd(f'execute store success storage h264:proof {phase}.loaded{i} byte 1 run execute if loaded {x} 100 {z}')
        cmd(f'execute store success storage h264:proof {phase}.floor byte 1 run execute if block 4 99 4 stone')
        cmd(f'execute store success storage h264:proof {phase}.pig byte 1 run execute if entity @e[type=pig,tag=h264_nonplayer,nbt={{UUID:[I;264,0,0,1],Health:10f,NoAI:1b,NoGravity:1b,Pos:[8.5d,100d,8.5d]}}]')
        store(f'{phase}.clock', 'time query gametime', 'long')
        tests = []
        def test(name, identifier, want, criterion=None):
            j = list(KNOWN).index(identifier)
            value = str(want).lower()
            if criterion: value = '{'+criterion+'='+value+'}'
            names = [name] if name else [A,B]
            present = any(state[[A,B].index(n)*2+j] == want for n in names)
            prefix = 'name='+name+',' if name else ''
            tests.append((f'@a[{prefix}advancements={{{identifier}={value}}}]',present))
        test(A,ROOT,True)
        test(B,ROOT,True); test(B,ROOT,False)
        test(None,ROOT,True); test(None,ROOT,False)
        test(None,ROOT,True,'crafting_table');test(None,ROOT,False,'crafting_table')
        test(A,ROOT,True,'crafting_table')
        test(B,ROOT,True,'crafting_table');test(B,ROOT,False,'crafting_table')
        test(B,STONE,True);test(B,STONE,False)
        tests += [(f'@a[advancements={{{ROOT}=false,{STONE}=true}}]',any(not state[i*2] and state[i*2+1] for i in [0,1])),
                  (f'@e[type=pig,tag=h264_nonplayer,advancements={{{ROOT}=false}}]',False)]
        if phase == 'stone_B': tests += tests[8:10]  # Repeated reads of the same known criterion.
        if unknown and phase == 'root_B_again':
            for name in [A,B]:
                for value in ['{h264_missing=false}','{h264_missing=true}']:
                    tests.append((f'@a[name={name},advancements={{{ROOT}={value}}}]',False))
                tests.append((f'@a[name={name},advancements={{h264:missing=false}}]',False))
        for index,(selector,present) in enumerate(tests):
            for sign in ['if','unless']:
                key=f'{phase}.q{index:02}_{sign}'
                cmd('scoreboard players set * h264 -99')
                for score in ['result','success','branch']:
                    store(f'{key}.before_{score}',f'scoreboard players get {score} h264')
                cmd(f'execute store result score result h264 store success score success h264 {sign} entity {selector} run scoreboard players set branch h264 1',key+'.action')
                for score in ['result','success','branch']:
                    store(f'{key}.{score}',f'scoreboard players get {score} h264')
                ops[-1]['expect_query']={'key':key,'taken':present if sign=='if' else not present}
        witness={'op':'witness' if packet else 'save','phase':phase,'state':state}
        if packet:witness['receipts']=1
        if packet and model:witness['model']=True
        if model and clock_file:witness['clock_source']='saved_level_Time'
        ops.append(witness)
    return ops


def read_wire(path, ops):
    grouped = {op['label']: {A:[],B:[]} for op in ops if op['op']=='cmd'}
    for line in path.read_text().splitlines():
        if ' chat-flat ' not in line:
            continue
        who, rest = line.split(' step=',1)
        step, raw = rest.split(' chat-flat ',1)
        if step in grouped:
            grouped[step][who].append(json.loads(raw))
    return grouped


def run(label, command, client_command, build, world, ops, packet=True, java_side=False, model=True, clock_file=True):
    directory=build/label
    port=net.server_dir(directory,world)
    if model and clock_file:
        path=directory/'ops.json';entries=json.loads(path.read_text());path.write_text(json.dumps([entry for entry in entries if entry['name']==A],indent=2))
    audit='HORIZON_DEV_ADVANCEMENT_SELECTOR_AUDIT'
    previous=os.environ.get(audit)
    if model and not java_side:os.environ[audit]='1'
    try:server=net.Server(command,directory,label)
    finally:
        if previous is None:os.environ.pop(audit,None)
        else:os.environ[audit]=previous
    model_rows={}
    clocks={}
    wire=build/f'{label}-wire.txt'
    log=(build/f'{label}-client.log').open('w')
    client=None
    try:
        server.wait_for(r'Done \(')
        client=net.Client(client_command,port,wire,log)
        client.tell('chat on');client.tell('flat-chat on');client.tell('state on')
        if packet:client.tell('advancements on')
        if model and clock_file:client.tell('list-syncs on')
        for name in [A,B]:
            client.tell(f'join {name}')
            client.sync([name])
        for op in ops:
            if op['op']=='cmd':
                client.tell('step '+op['label'])
                client.tell(f'send-cmd {A} '+op['cmd'])
                client.sync([A])
            elif op['op']=='world_clock':
                client.tell('step witness_clock_'+op['key'])
                start=len(server.lines);server.console('save-all flush');wait_new(server,start,'Saved the game')
                if model and clock_file:
                    client.sync([A]);client.sync([A])  # Drain save feedback before the next action label.
                source=directory/'world/level.dat'
                target=build/f'{label}-clock-{op["key"]}-level.dat'
                shutil.copy2(source,target)
                data=json.loads(json.dumps(nbt.read_dat(target)));value=data.get('Data',data)['Time']
                assert value[0]=='l',('actual saved Time must be Long',value)
                clocks[op['key']]={'absolute_Time':value,'saved_file':str(target),'sha256':hashlib.sha256(target.read_bytes()).hexdigest(),'save_trace':server.lines[start:]}
                (build/f'{label}-absolute-clocks.json').write_text(json.dumps(clocks,indent=2)+'\n')
            else:
                if packet:
                    client.tell('step witness_'+op['phase'])
                    for name in [A,B]:
                        client.tell(f'adv-snapshot {name} {op["phase"]} {ROOT},{STONE}')
                        ack=f'adv-snapshotted {op["phase"]} {name}\n'
                        while (line:=client.process.stdout.readline()) != ack:
                            assert line, 'client exited before snapshot receipt'
                            log.write(line)
                start=len(server.lines);server.console('save-all flush');wait_new(server,start,'Saved the game')
                if model and clock_file:
                    client.sync([A]);client.sync([A])  # Drain save feedback before the next action label.
                if model and not java_side:
                    rows=[json.loads(line.split('advancement selector model raw: ',1)[1]) for line in server.lines[start:] if 'advancement selector model raw: ' in line]
                    model_rows[op['phase']]=rows
                    (build/f'{label}-native-models.json').write_text(json.dumps(model_rows,indent=2)+'\n')
                    assert len(rows)==2 and {r['name'] for r in rows}=={A,B},('actual placed guests',rows)
                    assert len({r['conn'] for r in rows})==2 and all(type(r['conn']) is int and r['conn']>=0 for r in rows),rows
                for name in [A,B]:
                    files=[p for p in (directory/'world').rglob(offline(name)+'.json') if p.parent.name=='advancements']
                    if java_side or not packet:
                        assert len(files)==1, ('independent progress admission',name,files)
                    if files:
                        assert len(files)==1,files
                        shutil.copy2(files[0],build/f'{label}-{op["phase"]}-{name}-advancements.json')
        start=len(server.lines)
        client.end()
        for name in [A,B]:wait_new(server,start,name+r'.*(left the game|lost connection)')
    finally:
        try:
            if client and client.process.poll() is None:
                client.end()  # Flush the actual partial wire trace even on rejected admission.
        except Exception as error:
            log.write('partial wire flush failure: '+repr(error)+'\n')
        finally:
            if client:client.kill()
            server.stop();log.close()
    if packet:
        for name in [A,B]:
            files=[p for p in (directory/'world').rglob(offline(name)+'.json') if p.parent.name=='advancements']
            assert len(files)==1,('final disconnected progress',name,files)
            shutil.copy2(files[0],build/f'{label}-final-{name}-advancements.json')
    storage=[p for p in (directory/'world').rglob('*command_storage*.dat') if 'h264' in p.parts or 'h264' in p.name]
    assert len(storage)==1,storage
    data=json.loads(json.dumps(nbt.read_dat(storage[0])))
    (build/f'{label}-storage.json').write_text(json.dumps(data,indent=2))
    return data['data']['contents']['proof'],read_wire(wire,ops)


def facts(label,build,ops,proof,messages,validate=False,packet=True,java_side=False,model=True,packet_diagnostic=False,clock_file=True):
    snapshots={}
    if packet:
        for line in (build/f'{label}-wire.txt').read_text().splitlines():
            if ' adv-snapshot ' in line:
                who=line.split(' step=',1)[0];row=json.loads(line.split(' adv-snapshot ',1)[1]);snapshots[row['label'],who]=row
    model_rows=json.loads((build/f'{label}-native-models.json').read_text()) if model and not java_side else {}
    clocks=json.loads((build/f'{label}-absolute-clocks.json').read_text()) if model and clock_file else {}
    start_clock=clocks['start']['absolute_Time'] if clocks else proof['start']
    out={}
    for op in ops:
        if op['op']=='cmd':
            out[op['label']+'/feedback']=messages[op['label']]
            if q:=op.get('expect_query'):
                value=proof
                for part in q['key'].split('.'):value=value[part]
                out[q['key']+'/state']=value
                assert all(value['before_'+s]==-99 for s in ['result','success','branch']),q
                if validate:
                    wanted=1 if q['taken'] else -99
                    assert all(value[s]==wanted for s in ['result','success','branch']),(q,value)
        elif op['op']=='world_clock':
            value=clocks[op['key']]['absolute_Time']
            # An explicit admission, not a rewritten or cross-engine clock record.
            out[op['label']+'/relative-clock-invariant']={'long':value[0]=='l','stationary':value[1]==start_clock[1]}
            assert out[op['label']+'/relative-clock-invariant']=={'long':True,'stationary':True}
        else:
            phase=op['phase'];p=proof[phase]
            phase_clock=clocks[phase+'.clock']['absolute_Time'] if clocks else p['clock']
            assert phase_clock[0]=='l' and start_clock[0]=='l',('typed clock',phase_clock,start_clock)
            if clocks:out[phase+'/clock-stationary-invariant']=phase_clock[1]==start_clock[1]
            else:out[phase+'/clock-delta']=phase_clock[1]-start_clock[1]
            for key in ['floor','pig',*[f'loaded{i}' for i in range(4)]]:
                out[phase+'/'+key]=p[key]
                assert p[key]==['b',1],(phase,key,p[key])
            for i,name in enumerate([A,B]):
                player=p[f'player{i}']
                for key in ['UUID','Health','Pos','playerGameType']:
                    out[phase+'/'+name+'/'+key]=player[key]
                if packet:
                    snapshot=snapshots[phase,name]
                    assert snapshot['name']==name and snapshot['uuid']==offline(name) and snapshot['entity_id']>0,snapshot
                    assert snapshot['reset_count']>0 and snapshot['packet_count']>0 and snapshot['receipt_count']>0,snapshot
                    out[phase+'/'+name+'/progress_stream']={'name':snapshot['name'],'uuid':snapshot['uuid'],'reset_seen':snapshot['reset_count']>0,'receipt_count':snapshot['receipt_count']}
                    actual=snapshot['progress']
                    if not model:out[phase+'/'+name+'/progress']=actual
                    for j,(identifier,criterion) in (enumerate(KNOWN.items()) if not model and (java_side or not packet_diagnostic) else []):
                        state=actual[identifier];want=op['state'][i*2+j]
                        if want:
                            assert state=={'present':True,'progress_received':True,'done':True,'criteria':[criterion]},(phase,name,state)
                        elif state.get('progress_received'):
                            assert state['done'] is False and state['criteria']==[],(phase,name,state)
                        else:
                            assert 'done' not in state and 'criteria' not in state,('no fabricated missing progress',state)
                if model:
                    if java_side:
                        raw=json.loads((build/f'{label}-{phase}-{name}-advancements.json').read_text())
                        binding={'name':snapshot['name'],'uuid':snapshot['uuid']}
                        clock=phase_clock[1]
                    else:
                        record=next(r for r in model_rows[phase] if r['name']==name)
                        raw=record['progress'];binding={k:record[k] for k in ['name','uuid']};clock=record['game_time']
                    assert binding=={'name':snapshot['name'],'uuid':snapshot['uuid']},('model/actual connection binding',phase,name,binding,snapshot)
                    assert clock==phase_clock[1],('actual model / saved Time cross-check',clock,phase_clock)
                    out[phase+'/'+name+'/model_binding']=binding
                    if clocks:out[phase+'/'+name+'/model_epoch_stationary-invariant']=clock==start_clock[1]
                    else:out[phase+'/'+name+'/model_clock-delta']=clock-start_clock[1]
                    actual={identifier:{'present':identifier in raw,'done':raw.get(identifier,{}).get('done',False),'criteria':sorted(raw.get(identifier,{}).get('criteria',{}))} for identifier in KNOWN}
                    out[phase+'/'+name+'/progress']=actual
                    for j,(identifier,criterion) in enumerate(KNOWN.items()):
                        want=op['state'][i*2+j]
                        assert actual[identifier]=={'present':want,'done':want,'criteria':[criterion] if want else []},('independent model admission',phase,name,actual)
                elif java_side or not packet:
                    raw=json.loads((build/f'{label}-{phase}-{name}-advancements.json').read_text())
                    actual={identifier:{'done':raw.get(identifier,{}).get('done',False),'criteria':sorted(raw.get(identifier,{}).get('criteria',{}))} for identifier in KNOWN}
                    if not packet:out[phase+'/'+name+'/progress']=actual
                    # Independent Java saved phase files validate the bounded reader.
                    for j,(identifier,criterion) in enumerate(KNOWN.items()):
                        want=op['state'][i*2+j];assert actual[identifier]=={'done':want,'criteria':[criterion] if want else []},(phase,name,actual)
                assert player['UUID']==['I',uuid_words(name)] and player['Health']==['f',20.0],(phase,name,player)
                assert player['Pos']==[['d',4.5+i*2],['d',100.0],['d',4.5]] and player['playerGameType']==1,(phase,name,player)
            assert phase_clock[1]==start_clock[1],phase
    if packet:
        last=ops[-1]['state']
        for i,name in enumerate([A,B]):
            raw=json.loads((build/f'{label}-final-{name}-advancements.json').read_text())
            actual={identifier:{'done':raw.get(identifier,{}).get('done',False),'criteria':sorted(raw.get(identifier,{}).get('criteria',{}))} for identifier in KNOWN}
            if model:
                actual={identifier:{'present':identifier in raw,**value} for identifier,value in actual.items()}
            out['final/'+name+'/progress']=actual
            for j,(identifier,criterion) in enumerate(KNOWN.items()):
                want=last[i*2+j];expected={'done':want,'criteria':[criterion] if want else []}
                if model:expected['present']=want
                assert actual[identifier]==expected,(name,actual)
    return out


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--side',choices=['java','native','both'],default='both',
                        help="both (the default, as parity.py runs it): Java, then native against Java's facts")
    parser.add_argument('--label',help="the run's name (default: the side)")
    parser.add_argument('--binary',type=Path)
    parser.add_argument('--reference',type=Path)
    parser.add_argument('--unknown-diagnostic',action='store_true')
    parser.add_argument('--clock-command-diagnostic',action='store_true',help='earlier1781/full1889 mixed time-command/barrier capture diagnostic')
    parser.add_argument('--packet-diagnostic',action='store_true',help='non-authoritative packet1757/full1865, including omitted/stale empty progress')
    parser.add_argument('--live-save-diagnostic',action='store_true',help='original Java1743/full1851 and rejected native online-save prerequisite')
    args=parser.parse_args()
    packet=not args.live_save_diagnostic
    model=packet and not args.packet_diagnostic
    clock_file=model and not args.clock_command_diagnostic
    assert not(args.clock_command_diagnostic and not model),'clock diagnostic needs model witness'
    assert not (args.live_save_diagnostic and args.packet_diagnostic),'choose one diagnostic'
    net.PLAYERS=[A,B];net.PROPERTIES['force-gamemode']='false'
    _,build,java,native,client=net.sides('guest-advancement-selector-parity',args.binary.resolve() if args.binary else None)
    world=build/'generate/world'
    if not world.exists():world=generate(java,build)
    ops=program(args.unknown_diagnostic,packet,model,clock_file)
    script=build/(('model-saved-clock-' if clock_file else 'model-time-' if model else 'packet-' if packet else 'legacy-')+('unknown-script.json' if args.unknown_diagnostic else 'known-script.json'))
    text=json.dumps(ops,indent=2)+'\n'
    if script.exists():assert script.read_text()==text,'declared script changed'
    else:script.write_text(text)
    if args.side=='both':
        side(args,'java','java',None,java,native,client,build,world,ops,packet,model,clock_file)
        side(args,'native','native',build/'java-facts.json',java,native,client,build,world,ops,packet,model,clock_file)
    else:
        side(args,args.side,args.label or args.side,args.reference,java,native,client,build,world,ops,packet,model,clock_file)


def side(args,name,label,reference,java,native,client,build,world,ops,packet,model,clock_file):
    """One side's run: its facts, compared with `reference` when one is given."""
    proof,messages=run(label,java if name=='java' else native,client,build,world,ops,packet,name=='java',model,clock_file)
    actual=facts(label,build,ops,proof,messages,validate=name=='java',packet=packet,java_side=name=='java',model=model,packet_diagnostic=args.packet_diagnostic,clock_file=clock_file)
    (build/f'{label}-facts.json').write_text(json.dumps(actual,indent=2,sort_keys=True)+'\n')
    if reference:
        reference=json.loads(Path(reference).read_text());assert actual.keys()==reference.keys(),'canonical labels changed'
        differences=[{'label':k,'expected':reference[k],'actual':actual[k]} for k in reference if reference[k]!=actual[k]]
        report={'records':len(reference),'passed':len(reference)-len(differences),'differences':differences,'qualified_h264':model and clock_file,'scope':'authoritative model / saved-clock gate; absolute clocks retained engine-specific, only explicit relative invariants compare' if model and clock_file else 'non-authoritative prerequisite diagnostic'}
        (build/f'{label}-report.json').write_text(json.dumps(report,indent=2)+'\n');print(label,report['passed'],'/',len(reference))
        if differences:raise SystemExit(1)
    else:print(label,len(actual),'canonical records captured')

if __name__=='__main__':main()
