"""Offline V1 reporting for the bounded forced free-point pitch proof.

The retained V5 capture functions below document the original engine actions.
Their early-id completion collector is known to fail on a torn snapshot and is
NOT the runnable public entry point. Immutable original V5 remains in evidence.
This command only re-projects completed captures; it starts no engine. Public
end-to-end capture replay has not been independently run.
"""
import argparse, gzip, hashlib, io, json, os, re, shutil, struct, subprocess, sys, time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'tools'))
import anvil_dump as nbt
import net_capture
import test_locator_bar_parity as net
from spawnpoint_respawn_client import prepare_client
from test_builtin_component_delivery_parity import Commands, generate, offline, uuid_words

def sha(p): return hashlib.sha256(p.read_bytes()).hexdigest()
def bits(x): return struct.unpack('>i',struct.pack('>f',x))[0]
def typed_dat(path):
    raw=path.read_bytes(); data=gzip.decompress(raw) if raw[:2]==b'\x1f\x8b' else raw
    f=io.BytesIO(data); assert f.read(1)==b'\x0a'; n=struct.unpack('>H',f.read(2))[0];f.read(n)
    value=nbt.read_tag(f,10);assert f.read()==b'', 'complete enclosing NBT consumption'
    return value

def run_side(label,command,client_command,build,world):
    directory=build/label; assert not directory.exists(), 'new per-side server directory required'
    port=net.server_dir(directory,world);server=net.Server(command,directory,label)
    commands=Commands(server,build/(label+'-commands.json'));raw=build/(label+'-client.txt')
    log=(build/(label+'-client.log')).open('w');client=stop=None; completion=None
    admissions=[]; gates={};phases={};files={}
    def cmd(text):return commands.reply(text)
    def guard(phase,key,condition,assigned=False):
        p=phase+'.'+key
        cmd(f'data modify storage respawnproof:proof {p} set value {{result:91,success:17b}}')
        cmd(f'execute store result storage respawnproof:proof {p}.result int 1 store success storage respawnproof:proof {p}.success byte 1 run execute {condition}')
        text=cmd(f'data get storage respawnproof:proof {p}')
        okay='result: 1' in text and 'success: 1b' in text
        if assigned:gates[p]=okay
        else:assert okay,(p,text);admissions.append(p)
    def receipt(line,expected):
        client.tell(line)
        while (s:=client.process.stdout.readline())!=expected+'\n':
            assert s,('client receipt',expected);log.write(s);log.flush()
    def snapshot(phase,health,position=None):
        words=','.join(map(str,uuid_words('Alice')))
        body=f'UUID:[I;{words}],Health:{health}.0f'
        if position is not None:body+=',Pos:['+','.join(str(x)+'d' for x in position)+']'
        guard(phase,'identity',f'if entity @a[name=Alice,type=player,gamemode=survival,nbt={{{body}}}]')
        guard(phase,'dimension','as Alice at @s if dimension minecraft:overworld')
        guard(phase,'floor','if block 12 99 12 stone')
        for y in [100,101]:guard(phase,'air'+str(y),f'if block 12 {y} 12 air')
        guard(phase,'loaded','if loaded 12 100 12')
        cmd('data get entity Alice');cmd('data get entity Alice Pos');cmd('data get entity Alice Rotation')
        cmd('execute store result storage respawnproof:clock value long 1 run time query gametime')
        clock=cmd('data get storage respawnproof:clock value')
        receipt('spawn-snapshot Alice '+phase,'spawn-snapshotted Alice '+phase)
        match=re.findall(r'contents: (-?\d+)[lL](?:\s|$)',clock)
        assert len(match)==1,('typed clock receipt',clock)
        phases[phase]={'clock_reply':clock,'world_time':int(match[0]),'command_index':len(commands.rows)}
    def save_config():
        text=cmd('save-all flush');assert 'Saved the game' in text
        path=directory/'world/players/data'/f'{offline("Alice")}.dat'
        assert path.is_file(),('declared actual player path',path)
        value=typed_dat(path);assert value['UUID']==('I',uuid_words('Alice')),value.get('UUID')
        target=build/(label+'-configured-player.dat');shutil.copy2(path,target)
        (build/(label+'-configured-player.typed.json')).write_text(json.dumps(value,indent=2)+'\n')
        files['configured']={'path':str(path),'copy':str(target),'sha256':sha(target),'full_consumed':True}
        config=value['respawn'];assert config['pos']==('I',[12,100,12]) and config['dimension']=='minecraft:overworld'
        assert config['forced']==('b',1) and config['yaw']==('f',45.0),config
        gates['saved_config_pitch']=config.get('pitch')==('f',10.0)
    try:
        server.wait_for(r'Done \(')
        text=cmd('tick freeze');assert 'frozen' in text.lower(),text
        admissions.append('actual-new-server-freeze-ack')
        listen=net.free_port();stop=net_capture.start_background(listen,f'127.0.0.1:{port}',build/(label+'-packets.jsonl'),full=True)
        client=net.Client(client_command,listen,raw,log);client.tell('chat on');client.tell('state on');client.tell('players on')
        client.tell('join Alice');client.sync(['Alice'])
        cmd('tp Alice 4.5 100 4.5 0 0');client.sync(['Alice']);cmd('gamemode survival Alice');client.sync(['Alice'])
        # There is no post-death or post-respawn still/move/teleport operation.
        snapshot('joined',20,[4.5,100.0,4.5])
        guard('world','spawncontrol','if block 4 99 4 stone')
        cmd('data modify storage respawnproof:proof spawnCommand set value {result:91,success:17b}')
        text=cmd('execute store result storage respawnproof:proof spawnCommand.result int 1 store success storage respawnproof:proof spawnCommand.success byte 1 run spawnpoint Alice 12 100 12 45 10')
        assert 'spawn point' in text.lower(),text
        result=cmd('data get storage respawnproof:proof spawnCommand')
        assert 'result: 1' in result and 'success: 1b' in result,result
        admissions.append('spawnpoint-actual-result-success')
        snapshot('configured',20,[4.5,100.0,4.5])
        guard('configured','model', 'if entity @a[name=Alice,nbt={respawn:{dimension:"minecraft:overworld",pos:[I;12,100,12],forced:1b,yaw:45.0f}}]')
        guard('configured','pitch','if entity @a[name=Alice,nbt={respawn:{pitch:10.0f}}]',assigned=True)
        save_config()
        text=cmd('kill Alice');assert 'Killed' in text,text
        client.sync(['Alice']);snapshot('dead',0,[4.5,100.0,4.5])
        receipt('respawn Alice','respawn-sent Alice');client.sync(['Alice'])
        # Bounded packet observation, not a movement or position mutation. Each dump is retained.
        deadline=time.monotonic()+30
        while True:
            receipt('dump beforeLoaded','dumped beforeLoaded')
            rows=[json.loads(x.split(' spawn-wire ',1)[1]) for x in raw.read_text().splitlines() if ' spawn-wire ' in x]
            requests=[x for x in rows if x['kind']=='request-before']
            acks=[x for x in rows if x['kind']=='position-ack']
            required_chunks={f'{x},{z}' for x in range(-2,3) for z in range(-2,3)}
            respawn_rows=[x for x in rows if x['kind']=='respawn']
            if requests and respawn_rows and acks and acks[-1]['teleport_id']!=requests[-1]['teleport_id'] and required_chunks<=set(acks[-1]['chunks']):
                completion=acks[-1];break
            # Chunk receipts can follow the position: record an actual received-state snapshot without mutating it.
            receipt('spawn-snapshot Alice arrival-pending','spawn-snapshotted Alice arrival-pending')
            receipt('dump pendingChunks','dumped pendingChunks')
            snapshots=[json.loads(x.split(' spawn-wire ',1)[1]) for x in raw.read_text().splitlines() if ' spawn-wire ' in x]
            candidates=[x for x in snapshots if x['kind']=='arrival-pending']
            if requests and respawn_rows and candidates and candidates[-1]['teleport_id']!=requests[-1]['teleport_id'] and required_chunks<=set(candidates[-1]['chunks']):
                completion=candidates[-1];break
            assert time.monotonic()<deadline,('new actual respawn teleport/loaded chunk prerequisite',rows)
            time.sleep(.05);client.sync(['Alice'])
        # Explicit server level-loaded ack; independent official decoded chunk0/position evidence is checked below.
        receipt('spawn-loaded Alice','spawn-loaded Alice');client.sync(['Alice'])
        snapshot('revived',20,[12.5,100.1,12.5])
        guard('revived','rotation','if entity @a[name=Alice,nbt={Rotation:[45.0f,10.0f]}]',assigned=True)
        client.end();client=None
    finally:
        if client:
            try:client.end()
            except Exception as error:log.write('partial '+repr(error)+'\n')
            client.kill()
        if stop:stop.set()
        server.stop();log.close()
        (build/(label+'-partial.json')).write_text(json.dumps({'phases':phases,'admissions':admissions,'gates':gates,'files':files},indent=2)+'\n')
    transcript=raw.read_text();assert not re.search(r'bytes left|larger than expected|\berror\b',transcript),'official complete codec failure'
    rows=[json.loads(x.split(' spawn-wire ',1)[1]) for x in transcript.splitlines() if ' spawn-wire ' in x]
    assert rows and all(x['profile_uuid']==offline('Alice') and x['profile_name']=='Alice' for x in rows)
    login=[x for x in rows if x['kind']=='login'];assert len(login)==1
    assert all(x['entity_id']==login[0]['entity_id'] for x in rows)
    request=[x for x in rows if x['kind']=='request-before'];assert len(request)==1
    rs=[x for x in rows if x['kind']=='respawn'];assert len(rs)==1
    final=[x for x in rows if x['kind']=='revived'];assert len(final)==1
    assert len({p['world_time'] for p in phases.values()})==1,'world game-time window advanced'
    f=final[0];assert f['position']==[12.5,100.1,12.5] and f['health_bits']==bits(20.0)
    assert completion is not None and f['teleport_id']==completion['teleport_id']!=request[0]['teleport_id']
    assert f['teleports']>request[0]['teleports'] and required_chunks<=set(f['chunks'])
    gates['client_rotation']=f['yaw_bits']==bits(45.0) and f['pitch_bits']==bits(10.0)
    # Full proxy payloads remain independent raw authority, with official codec read of the client stream.
    frames=net_capture.load(build/(label+'-packets.jsonl'));assert frames
    assert len(net_capture.play_sessions(frames))==1
    requests_wire=[i for i,x in enumerate(frames) if x['state']=='play' and x['dir']=='c2s' and x['name']=='client_command' and bytes.fromhex(x['data'])==b'\x00']
    assert len(requests_wire)==1
    positions=[];acks=[];respawn_frames=[]
    for i,x in enumerate(frames):
        if i<=requests_wire[0] or x['state']!='play':continue
        data=bytes.fromhex(x['data'])
        if x['dir']=='s2c' and x['name']=='respawn':respawn_frames.append(i)
        if x['name']=='player_position' and x['dir']=='s2c':
            nonce,off=net_capture.read_varint(data,0);assert len(data)-off==60
            positions.append({'index':i,'id':nonce,'raw':x,'fields':struct.unpack('>ddddddffi',data[off:])})
        if x['name']=='accept_teleportation' and x['dir']=='c2s':
            nonce,off=net_capture.read_varint(data,0);assert len(data)-off==32
            acks.append({'index':i,'id':nonce,'raw':x,'fields':struct.unpack('>dddff',data[off:])})
    selected=[x for x in positions if x['id']==f['teleport_id']];assert len(selected)==1
    accepted=[x for x in acks if x['id']==f['teleport_id']];assert len(accepted)==1
    chosen,ack=selected[0],accepted[0]
    assert respawn_frames and requests_wire[0]<respawn_frames[0]<chosen['index']<ack['index']
    assert list(ack['fields'][:3])==f['position'] and list(chosen['fields'][:3])==f['position']
    assert [bits(v) for v in ack['fields'][3:]]==[f['yaw_bits'],f['pitch_bits']]
    assert [bits(v) for v in chosen['fields'][6:8]]==[f['yaw_bits'],f['pitch_bits']]
    report={'admitted':True,'gates':gates,'all_pass':all(gates.values()),'admissions':admissions,'phases':phases,'files':files,'wire':rows,'arrival_completion':completion,'all_positions_after_request':positions,'all_acknowledgements_after_request':acks,'extra_frames_are_public_diagnostics_not_coalesced':True,'command_records':len(commands.rows),'native_path_adapter':'actual dedicated cwd/world/players/data/offline-login-UUID.dat'}
    (build/(label+'-report.json')).write_text(json.dumps(report,indent=2)+'\n')
    if label=='java':assert report['all_pass'],report
    return report

def legacy_capture_main_v5():
    p=argparse.ArgumentParser();p.add_argument('--work-dir',type=Path,required=True);p.add_argument('--reference-only',action='store_true');p.add_argument('--reference-dir',type=Path);p.add_argument('--binary',type=Path);p.add_argument('--prepared-input',type=Path);a=p.parse_args()
    a.work_dir.mkdir(exist_ok=True,parents=True)
    if a.reference_dir:
        m=json.loads((a.reference_dir/'reference-client.json').read_text())
        result=run_side('native',[str(a.binary),'--resources',str(net.lab.LOCAL/'horizon-client/26.3'),'--dedicated','--nogui'],m['client'],a.work_dir,Path(m['world']))
        return 0 if result['all_pass'] else 1
    assert a.reference_only
    source=prepare_client(ROOT,a.work_dir/'observer-source')
    if a.prepared_input:
        m=json.loads(a.prepared_input.read_text())
        for group in ['class_hashes','world_hashes']:
            for path,want in m[group].items():assert sha(Path(path))==want,path
        java,client,world=m['java'],m['client'],Path(m['world'])
    else:
        lock,jars,libraries=net.base.verify_artifacts();deps=os.pathsep.join(map(str,[jars['server'],*libraries]));classes=a.work_dir/'classes';classes.mkdir()
        subprocess.run(['javac','--release',str(lock['minecraft']['java_major']),'-proc:none','-nowarn','-sourcepath',str(ROOT/'mods/horizon/src/test/java'),'-cp',deps,'-d',str(classes),str(source)],check=True)
        java=['java','-Xmx2g','-Duser.timezone=UTC','-cp',deps,'net.minecraft.server.Main','nogui']
        client=['java','-Xmx1g','-cp',str(classes)+os.pathsep+deps,'dev.horizonlab.parity.LocatorBarClient']
        world=generate(java,a.work_dir)
    result=run_side('java',java,client,a.work_dir,world)
    (a.work_dir/'reference-client.json').write_text(json.dumps({'client':client,'world':str(world),'source_sha256':sha(source)},indent=2)+'\n')
    return 0
def main():
    from spawnpoint_respawn_report_v1 import main as report_completed_captures
    return report_completed_captures()

if __name__=='__main__':raise SystemExit(main())
