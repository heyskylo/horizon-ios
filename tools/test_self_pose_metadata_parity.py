#!/usr/bin/env python3
"""Five-phase connected-player self Pose metadata, using official full packet codecs.
Initial default delivery is observational; flags never stand in for a typed Pose.
"""
import argparse,hashlib,json,os,re,time
from pathlib import Path
try:
    from . import net_capture,test_locator_bar_parity as net
    from .test_builtin_component_delivery_parity import Commands,generate,offline,uuid_words
except ImportError:
    import net_capture,test_locator_bar_parity as net
    from test_builtin_component_delivery_parity import Commands,generate,offline,uuid_words

PHASES=(('initial',None,None),('crouch-change','on','CROUCHING'),('crouch-repeat','on','CROUCHING'),('stand-change','off','STANDING'),('stand-repeat','off','STANDING'))

def dump(client,label):
    client.tell('dump '+label)
    while True:
        line=client.process.stdout.readline()
        assert line,'client exited before dump receipt'
        if line=='dumped '+label+'\n':return
        client.log.write(line)


def tracked_facts(path, target_uuid):
    lines=Path(path).read_text().splitlines()
    logins={}
    for line in lines:
        m=re.match(r'(\w+) .*pose-login ([0-9a-f-]+) (\d+) payload_hex=([0-9a-f]+)$',line)
        if m:logins[m[1]]={'uuid':m[2],'id':int(m[3]),'payload':m[4]}
    assert 'Alice' in logins and 'Bob' in logins,('actual logins missing',logins)
    assert logins['Alice']['uuid']==target_uuid
    assert logins['Alice']['id']!=logins['Bob']['id']
    mapped=None;pose=None;scale=None;adds=[]
    for line in lines:
        if not line.startswith('Bob '):continue
        m=re.search(r'track-add ([0-9a-f-]+) (\d+) login=(\d+) xyz=([^ ]+) payload_hex=([0-9a-f]+)$',line)
        if m and m[1]==target_uuid:
            assert int(m[2])==int(m[3])==logins['Alice']['id'],('actual target ID disagreement',m.groups(),logins)
            mapped=int(m[2]);pose=None;scale=None;adds.append(m.groups())
        m=re.search(r'track-remove ([0-9a-f-]+) (\d+) ',line)
        if m and m[1]==target_uuid:mapped=None;pose=None;scale=None
        m=re.search(r'track-pose ([0-9a-f-]+) (\d+) login=(\d+) metadata=(\d+) value=(\w+) payload_hex=([0-9a-f]+)$',line)
        if m and m[1]==target_uuid:
            assert mapped==int(m[2])==int(m[3])==logins['Alice']['id'];pose=m[5]
        m=re.search(r'track-scale ([0-9a-f-]+) (\d+) login=(\d+) base_bits=([0-9a-f]+) modifiers=(.*?) payload_hex=([0-9a-f]+)$',line)
        if m and m[1]==target_uuid:
            assert mapped==int(m[2])==int(m[3])==logins['Alice']['id'];scale={'base_bits':m[4],'modifiers':m[5],'payload':m[6]}
    return {'logins':logins,'observed_adds':adds,'mapped_target':mapped,'pose':pose,'scale':scale}

def drive(commands,client,clock,await_clock,read_track,dump,target_uuid_words,observer_uuid_words):
    names=['Alice','Bob']
    def cmd(s):return commands.reply(s)
    def guard(phase,name,condition):
        path=phase+'.guards.'+name
        cmd(f'data modify storage selfpose:proof {path} set value {{result:91,success:17b}}')
        cmd(f'execute store result storage selfpose:proof {path}.result int 1 '
            f'store success storage selfpose:proof {path}.success byte 1 run execute {condition}')
        cmd(f'data get storage selfpose:proof {path}')
    initial=clock()
    facts=[]
    for ordinal,(phase,key,want) in enumerate(PHASES):
        client.sync(names)
        client.tell('step '+phase)
        before=clock()
        if key is not None:
            client.tell('shift Alice '+key)
            client.sync(names)
            cmd('tick step 1')
            await_clock(before+1)
            client.sync(names)
        after=clock()
        assert after==initial+ordinal and after-before==(key is not None),(phase,before,after,initial)
        client.sync(names)
        dump(phase)
        actual=read_track()
        assert actual['scale'] and actual['scale']['base_bits']=='3ff0000000000000' and actual['scale']['modifiers']=='[]',('actual independent scale1 missing',phase,actual)
        if want is not None:
            assert actual['pose']==want,('actual typed tracked model missing',phase,actual)
        # Initial pose may be None on BOTH streams. Never fill it with STANDING.
        assert actual['mapped_target']==actual['logins']['Alice']['id']
        assert actual['logins']['Bob']['id']!=actual['logins']['Alice']['id']
        a=','.join(map(str,target_uuid_words));b=','.join(map(str,observer_uuid_words))
        guard(phase,'target',f'if entity @a[name=Alice,type=player,gamemode=survival,nbt={{UUID:[I;{a}],Health:20.0f,Pos:[6.5d,200.0d,4.5d]}}]')
        guard(phase,'observer',f'if entity @a[name=Bob,type=player,gamemode=spectator,nbt={{UUID:[I;{b}],Health:20.0f,Pos:[12.5d,200.0d,8.5d]}}]')
        guard(phase,'target_loaded','if loaded 6 200 4')
        guard(phase,'observer_loaded','if loaded 12 200 8')
        guard(phase,'floor','if block 6 199 4 minecraft:stone')
        guard(phase,'headroom','if block 6 201 4 minecraft:air')
        for who in names:
            cmd('attribute '+who+' minecraft:scale get')
            cmd('data get entity '+who)
        cmd(f'execute store result storage selfpose:proof {phase}.clock long 1 run time query gametime')
        cmd(f'data get storage selfpose:proof {phase}.clock')
        client.sync(names)
        dump(phase+'-complete')
        facts.append({'phase':phase,'before':before,'after':after,'actual_observer':actual,'initial_pose_not_inferred':phase=='initial'})
    assert clock()-initial==4
    return facts

def self_frames(transcript,phase,login):
    out=[]
    for line in transcript.splitlines():
        prefix='Alice step='+phase+' self-metadata '
        if not line.startswith(prefix):continue
        frame=json.loads(line[len(prefix):])
        assert frame['profile_uuid']==login['uuid']
        assert frame['packet_entity_id']==frame['login_entity_id']==login['id']
        out.append(frame) # Retain EVERY frame and ordered value, no coalescing.
    return out

def run_side(label,command,client_command,build,world):
    directory=build/label;port=net.server_dir(directory,world)
    server=net.Server(command,directory,label);commands=Commands(server,build/(label+'-commands.json'))
    out=build/(label+'-client.txt');log=(build/(label+'-client.log')).open('w');client=stop=None
    facts=[]
    try:
        server.wait_for(r'Done \(');commands.reply('tick freeze')
        listen=net.free_port();stop=net_capture.start_background(listen,f'127.0.0.1:{port}',build/(label+'-packets.jsonl'),full=True)
        client=net.Client(client_command,listen,out,log)
        for line in ['step initial','selfmetadata on','trackposes on','state on','chat on','join Alice']:
            client.tell(line)
        server.wait_for('Alice joined the game',timeout=120);client.sync(['Alice'])
        client.tell('join Bob');server.wait_for('Bob joined the game',timeout=120);client.sync(['Alice','Bob'])
        for cmd in ['fill 0 199 0 15 199 15 stone','fill 0 200 0 15 204 15 air','gamemode survival Alice','attribute Alice minecraft:scale base set 1','tp Alice 6.5 200 4.5 0 0','gamemode spectator Bob','attribute Bob minecraft:scale base set 1','tp Bob 12.5 200 8.5 0 0']:
            commands.reply(cmd)
        client.tell('still Alice');client.tell('still Bob');client.sync(['Alice','Bob'])
        def clock():
            commands.reply('execute store result storage selfpose:clock value long 1 run time query gametime')
            text=commands.reply('data get storage selfpose:clock value')
            m=re.findall(r'contents: (\d+)L',text);assert m,text;return int(m[-1])
        def await_clock(want):
            end=time.monotonic()+30
            while time.monotonic()<end:
                n=clock()
                if n>=want:assert n==want,(n,want);return
                time.sleep(.02)
            raise AssertionError('no exact real world tick')
        def track():return tracked_facts(out,offline('Alice'))
        facts=drive(commands,client,clock,await_clock,track,lambda phase:dump(client,phase),uuid_words('Alice'),uuid_words('Bob'))
        client.end();client=None
    finally:
        if client:client.kill()
        if stop:stop.set()
        server.stop();log.close()
    transcript=out.read_text()
    assert not [line for line in transcript.splitlines() if re.search(r'\berror\b|bytes left|larger than expected',line)]
    for phase,_,_ in PHASES:
        for guard in ['target','observer','target_loaded','observer_loaded','floor','headroom']:
            cmd=f'data get storage selfpose:proof {phase}.guards.{guard}'
            rows=[r for r in commands.rows if r['command']==cmd];assert len(rows)==1
            text='\n'.join(rows[0]['lines']);assert 'result: 1' in text and 'success: 1b' in text,(cmd,text)
    logins=tracked_facts(out,offline('Alice'))['logins']
    report={'version':1,'phases':{},'command_records':len(commands.rows),'admitted':True,'world_steps':4,'normalization':False,'logins':logins,'model_facts':facts}
    for phase,_,_ in PHASES:
        frames=self_frames(transcript,phase,logins['Alice'])
        relevant=[frame for frame in frames if any('pose' in v or (v['index']==0 and 'byte_bits' in v) for v in frame['ordered_values'])]
        report['phases'][phase]={'all_frames':frames,'relevant_frames':relevant,'ordered_pose_packets':[[v for v in frame['ordered_values'] if 'pose' in v] for frame in frames if any('pose' in v for v in frame['ordered_values'])],'flag_packets':[[v for v in frame['ordered_values'] if v['index']==0 and 'byte_bits' in v] for frame in frames if any(v['index']==0 and 'byte_bits' in v for v in frame['ordered_values'])]}
    (build/(label+'-report.json')).write_text(json.dumps(report,indent=2)+'\n')
    return report


def compare(reference,actual):
    rows=[]
    for phase,_,_ in PHASES:
        a=reference['phases'][phase];b=actual['phases'][phase]
        # Every actual frame stays in the report/raw. These ordered lists preserve
        # packet boundaries; extra/merged Pose or flag packets remain differences.
        def values(frames):return [f['ordered_values'] for f in frames]
        rows.append({'phase':phase,'pose_packets_equal':a['ordered_pose_packets']==b['ordered_pose_packets'],'flags_equal':a['flag_packets']==b['flag_packets'],'relevant_frame_boundaries_equal':values(a['relevant_frames'])==values(b['relevant_frames'])})
    return {'rows':rows,'all_pass':all(r['pose_packets_equal'] and r['flags_equal'] and r['relevant_frame_boundaries_equal'] for r in rows if r['phase']!='initial'),'initial_is_observation':True,'no_raw_rewrite':True}


def main():
    p=argparse.ArgumentParser();p.add_argument('--work-dir',type=Path,required=True);p.add_argument('--reference-only',action='store_true');p.add_argument('--reference-dir',type=Path);p.add_argument('--binary',type=Path);a=p.parse_args();a.work_dir.mkdir(parents=True,exist_ok=True)
    if a.reference_dir:
        assert a.binary
        meta=json.loads((a.reference_dir/'reference-client.json').read_text())
        client=meta['command'];world=Path(meta['world'])
        actual=run_side('native',[str(a.binary),'--resources',str(net.lab.LOCAL/'horizon-client/26.3'),'--dedicated','--nogui'],client,a.work_dir,world)
        ref=json.loads((a.reference_dir/'java-report.json').read_text());report=compare(ref,actual)
        (a.work_dir/'comparison.json').write_text(json.dumps(report,indent=2)+'\n');print(report)
        return 0 if report['all_pass'] else 1
    assert a.reference_only,'fresh reference first; native requires --reference-dir and --binary'
    assert not (a.work_dir/'java-commands.json').exists(),'preserve previous capture; use a new work directory'
    _,_,java,_,client=net.sides('self-pose-'+a.work_dir.name,a.binary or a.work_dir/'not-built-native')
    world=generate(java,a.work_dir)
    report=run_side('java',java,client,a.work_dir,world)
    # Fresh Java must admit the two changes and both no-change repeats. Initial
    # default Pose has no required packet or model inference.
    for phase,want in [('crouch-change','CROUCHING'),('stand-change','STANDING')]:
        poses=[v['pose'] for frame in report['phases'][phase]['ordered_pose_packets'] for v in frame]
        assert poses==[want],(phase,poses)
    for phase in ['crouch-repeat','stand-repeat']:assert report['phases'][phase]['ordered_pose_packets']==[],phase
    meta={'command':list(map(str,client)),'world':str(world),'source_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest()}
    (a.work_dir/'reference-client.json').write_text(json.dumps(meta,indent=2)+'\n');print({'java_admitted':True,'command_records':report['command_records'],'phases':5,'world_steps':4})
    return 0

if __name__=='__main__':raise SystemExit(main())
