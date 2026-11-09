#!/usr/bin/env python3
"""Connected dedicated non-owner advancement files at explicit save completion.

Disconnect is an independent cross-check. Full files, timestamps and packets remain raw.
"""
import argparse, hashlib, json, os, re, shutil, time
from pathlib import Path
try:
    from . import net_capture, test_locator_bar_parity as net
    from .test_builtin_component_delivery_parity import Commands, generate, offline, uuid_words
except ImportError:
    import net_capture, test_locator_bar_parity as net
    from test_builtin_component_delivery_parity import Commands, generate, offline, uuid_words

IDS = ('minecraft:story/root', 'minecraft:story/mine_stone')
PHASES = (('save', 'save-all', (True, True)),
          ('flush', 'save-all flush', (False, True)),
          ('repeat', 'save-all flush', (False, True)))

def progress(raw):
    return {i: {'present': i in raw, 'done': raw.get(i, {}).get('done', False),
                'criteria': sorted(raw.get(i, {}).get('criteria', {}))} for i in IDS}

def wanted(state):
    return {i: {'present': yes, 'done': yes, 'criteria': [c] if yes else []}
            for i, c, yes in zip(IDS, ('crafting_table', 'get_stone'), state)}

def observe_file(directory, build, label, phase):
    # Bounded completion observation, never interpret disconnect output as an online save.
    start = time.monotonic(); polls = []; last = None
    while True:
        files = list((directory/'world').rglob(offline('Bob')+'.json'))
        files = [p for p in files if p.parent.name == 'advancements']
        assert len(files) <= 1, files
        row = {'elapsed': time.monotonic()-start, 'exists': bool(files)}
        if files:
            p = files[0]; raw = p.read_bytes(); st = p.stat()
            row.update(path=str(p), bytes=len(raw), sha256=hashlib.sha256(raw).hexdigest(),
                       mtime_ns=st.st_mtime_ns, inode=st.st_ino)
            try:
                last = json.loads(raw); row['progress'] = progress(last)
            except (ValueError, UnicodeDecodeError) as error:
                row['parse_error'] = repr(error)
            (build/f'{label}-{phase}-advancements.raw.json').write_bytes(raw)
        polls.append(row)
        if time.monotonic()-start >= 2: break
        time.sleep(.1)
    result = {'polls': polls, 'final': polls[-1], 'progress': progress(last) if last is not None else None}
    (build/f'{label}-{phase}-file-observation.json').write_text(json.dumps(result, indent=2)+'\n')
    return result

def run_side(label, command, client_command, build, world):
    directory = build/label; port = net.server_dir(directory, world)
    audit = 'HORIZON_DEV_ADVANCEMENT_SELECTOR_AUDIT'; before = os.environ.get(audit)
    if label == 'native': os.environ[audit] = '1'
    try: server = net.Server(command, directory, label)
    finally:
        if before is None: os.environ.pop(audit, None)
        else: os.environ[audit] = before
    commands = Commands(server, build/(label+'-commands.json'))
    out = build/(label+'-client.txt'); log = (build/(label+'-client.log')).open('w')
    client = stop = None; results = {}; guard_reads = []; model = {}
    def cmd(text): return commands.reply(text)
    def guard(phase, key, condition):
        path = phase+'.'+key
        cmd(f'data modify storage onlinesave:proof {path} set value {{result:91,success:17b}}')
        cmd(f'execute store result storage onlinesave:proof {path}.result int 1 store success storage onlinesave:proof {path}.success byte 1 run execute {condition}')
        text = cmd(f'data get storage onlinesave:proof {path}')
        assert 'result: 1' in text and 'success: 1b' in text, (path, text)
        guard_reads.append(path)
    def admissions(phase):
        for name, x in [('Alice', 4.5), ('Bob', 6.5)]:
            words = ','.join(map(str, uuid_words(name)))
            guard(phase, name, f'if entity @a[name={name},type=player,gamemode=creative,nbt={{UUID:[I;{words}],Health:20.0f,Pos:[{x}d,200.0d,4.5d]}}]')
            cmd('data get entity '+name)
        guard(phase, 'floor', 'if block 6 199 4 minecraft:stone')
        guard(phase, 'load', 'if loaded 6 200 4')
    def snapshot(phase):
        for name in ['Alice', 'Bob']:
            client.tell(f'adv-snapshot {name} {phase} '+','.join(IDS))
            ack=f'adv-snapshotted {phase} {name}\n'
            while (line:=client.process.stdout.readline()) != ack:
                assert line, 'actual snapshot receipt missing'
                log.write(line)
    def clock():
        cmd('execute store result storage onlinesave:clock value long 1 run time query gametime')
        text=cmd('data get storage onlinesave:clock value'); m=re.findall(r'contents: (\d+)L',text)
        assert m, text
        return int(m[-1])
    try:
        server.wait_for(r'Done \('); cmd('tick freeze')
        listen=net.free_port(); stop=net_capture.start_background(listen,f'127.0.0.1:{port}',build/(label+'-packets.jsonl'),full=True)
        client=net.Client(client_command,listen,out,log)
        for text in ['advancements on','chat on','flat-chat on','state on','join Alice','join Bob']:
            client.tell(text)
        for name in ['Alice','Bob']: server.wait_for(name+' joined the game',timeout=120)
        client.sync(['Alice','Bob'])
        for text in ['fill 0 199 0 15 199 15 stone','fill 0 200 0 15 203 15 air','gamerule announce_advancements false']:
            cmd(text)
        for name,x in [('Alice',4.5),('Bob',6.5)]:
            cmd('gamemode creative '+name);cmd(f'tp {name} {x} 200 4.5 0 0');client.tell('still '+name)
            for i in IDS: cmd(f'advancement revoke {name} only {i}')
        cmd('advancement grant Bob only '+IDS[0]);client.sync(['Alice','Bob']);snapshot('seed')
        admissions('seed'); initial_clock=clock()
        start=len(server.lines);client.tell('quit Bob');server.wait_for(r'Bob.*(left the game|lost connection)',timeout=30)
        results['prior-disconnect']=observe_file(directory,build,label,'prior-disconnect')
        assert results['prior-disconnect']['progress']==wanted((True,False)),results['prior-disconnect']
        client.tell('join Bob');client.sync(['Alice','Bob']);cmd('tp Bob 6.5 200 4.5 0 0');client.tell('still Bob');client.sync(['Alice','Bob'])
        admissions('reconnected');snapshot('reconnected')
        for phase,save,state in PHASES:
            client.tell('step '+phase)
            if phase=='save': cmd('advancement grant Bob only '+IDS[1])
            if phase=='flush': cmd('advancement revoke Bob only '+IDS[0])
            client.sync(['Alice','Bob']);snapshot(phase);admissions(phase)
            # Selector controls are supplementary to actual file/model progress authority.
            for i,yes in zip(IDS,state):
                guard(phase,'criterion'+str(IDS.index(i)),f'if entity @a[name=Bob,advancements={{{i}={str(yes).lower()}}}]')
            start=len(server.lines);text=cmd(save)
            assert 'Saved the game' in text,(save,text)
            trace=server.lines[start:]; model[phase]=[json.loads(line.split('advancement selector model raw: ',1)[1]) for line in trace if 'advancement selector model raw: ' in line]
            if label=='native':
                rows=[r for r in model[phase] if r['name']=='Bob'];assert len(rows)==1,model[phase]
                assert rows[0]['uuid']==offline('Bob') and progress(rows[0]['progress'])==wanted(state),rows
            results[phase]=observe_file(directory,build,label,phase)
            client.sync(['Alice','Bob']);admissions(phase+'After')
            assert clock()==initial_clock,'world clock advanced'
            if label=='java': assert results[phase]['progress']==wanted(state),(phase,results[phase])
        client.end();client=None
        results['final-disconnect']=observe_file(directory,build,label,'final-disconnect')
        assert results['final-disconnect']['progress']==wanted((False,True))
    finally:
        if client:
            try:client.end()
            except Exception as error:log.write('partial trace flush '+repr(error)+'\n')
            client.kill()
        if stop:stop.set()
        server.stop();log.close()
        (build/(label+'-model-audit.json')).write_text(json.dumps(model,indent=2)+'\n')
    transcript=out.read_text();assert not re.search(r'bytes left|larger than expected|\berror\b',transcript), 'official codec error'
    # Retain all packet facts, including unavailable progress; never turn absence into false.
    snapshots=[json.loads(line.split(' adv-snapshot ',1)[1]) for line in transcript.splitlines() if ' adv-snapshot ' in line]
    assert snapshots and all(r['uuid'] in [offline('Alice'),offline('Bob')] for r in snapshots),snapshots
    report={'admitted':True,'command_records':len(commands.rows),'guard_receipts':guard_reads,'snapshots':snapshots,'files':results,'model':model,'zero_world_steps':True,'nonowner_route':'actual dedicated console; no integrated harness owner'}
    (build/(label+'-report.json')).write_text(json.dumps(report,indent=2)+'\n')
    return report

def main():
    p=argparse.ArgumentParser();p.add_argument('--work-dir',type=Path,required=True);p.add_argument('--reference-only',action='store_true');p.add_argument('--reference-dir',type=Path);p.add_argument('--binary',type=Path);a=p.parse_args();a.work_dir.mkdir(parents=True,exist_ok=True)
    if a.reference_dir:
        meta=json.loads((a.reference_dir/'reference-client.json').read_text())
        actual=run_side('native',[str(a.binary),'--resources',str(net.lab.LOCAL/'horizon-client/26.3'),'--dedicated','--nogui'],meta['command'],a.work_dir,Path(meta['world']))
        compare={phase:actual['files'][phase]['progress']==wanted(state) for phase,_,state in PHASES}
        result={'connected_file_gates':compare,'all_pass':all(compare.values()),'admitted':True};(a.work_dir/'comparison.json').write_text(json.dumps(result,indent=2)+'\n');print(result);return 0 if result['all_pass'] else 1
    assert a.reference_only
    _,_,java,_,client=net.sides('online-guest-advancement-'+a.work_dir.name,a.work_dir/'not-built')
    world=generate(java,a.work_dir);report=run_side('java',java,client,a.work_dir,world)
    (a.work_dir/'reference-client.json').write_text(json.dumps({'command':list(map(str,client)),'world':str(world)},indent=2)+'\n');print({'fresh_java_admitted':True,'command_records':report['command_records']});return 0
if __name__=='__main__':raise SystemExit(main())
