#!/usr/bin/env python3
"""Two real holder clicks: pre-action lead on attach, empty item context on detach.

Saved advancement files, command replies, full bodies, packet payloads and absolute
clocks are retained. Only declared typed facts form the compact measured comparison.
"""
import argparse, hashlib, json, os, re, shutil, subprocess, time, uuid
from pathlib import Path
import anvil_dump as nbt
import net_capture
import test_locator_bar_parity as net
from test_disconnect_crafting_parity import generate, offline, uuid_words, wait_new
from test_machine_recipe_placement_parity import Commands, dump
from test_dedicated_server_parity import game_time

PLAYER='LeashAlice'
FROG='00000009-0000-0009-0000-000900000009'
ADV='minecraft:husbandry/leash_all_frog_variants'
CRITERION='minecraft:temperate'
PHASES=['initial','attached','revoked','detached']


def program():
    return dict(version=3,player=PLAYER,frog=FROG,actions=['attach','holder-detach'],
                actual_steps=[2,1,1],criterion=[ADV,CRITERION],lead_counts=[4,3,3,3],
                guards='UUID/type/variant/Health/NoAI/Pos/load/floor/mode/actual holder/real sends/clock',
                strict='typed saved state, actual criterion recipient/progress and callbacks/stores',
                diagnostics='full actor bodies/advancement timestamps/epochs/all chronological styled replies/packets',
                limits='survival holder, temperate frog, main hand only; no nonholder/creative/drop/other variant claim')


def clients(build):
    lock,jars,libraries=net.base.verify_artifacts()
    cp=os.pathsep.join(map(str,[jars['server'],*libraries]))
    source=(net.lab.ROOT/'mods/horizon/src/test/java/dev/horizonlab/parity/LocatorBarClient.java').read_text()
    needle='case "cmd" -> players.get(w[1]).command(w[2]);'
    replacement=needle+'''
                    case "leash-click" -> {
                        var buffer = new net.minecraft.network.FriendlyByteBuf(io.netty.buffer.Unpooled.buffer());
                        buffer.writeVarInt(Integer.parseInt(w[2]));
                        buffer.writeVarInt(0); buffer.writeVarInt(0); buffer.writeBoolean(false);
                        var packet = net.minecraft.network.protocol.game.ServerboundInteractPacket.STREAM_CODEC.decode(buffer);
                        if (buffer.isReadable()) throw new IllegalStateException("interact trailing bytes");
                        players.get(w[1]).send(packet);
                        buffer.release();
                    }
    '''
    assert source.count(needle)==1
    directory=build/'client-source';directory.mkdir(exist_ok=True)
    java=directory/'LocatorBarClient.java';java.write_text(source.replace(needle,replacement))
    classes=build/'classes';classes.mkdir(exist_ok=True)
    subprocess.run(['javac','--release',str(lock['minecraft']['java_major']),'-proc:none','-nowarn','-sourcepath',str(net.lab.ROOT/'mods/horizon/src/test/java'),'-cp',cp,'-d',str(classes),str(java)],check=True)
    return ['java','-Xmx2g','-Duser.timezone=UTC','-cp',cp,'net.minecraft.server.Main','nogui'],['java','-Xmx1g','-cp',str(classes)+os.pathsep+cp,'dev.horizonlab.parity.LocatorBarClient']


def run(side,command,client_command,build):
    directory=build/side;port=net.server_dir(directory,build/'generate/world')
    wire=build/f'{side}-wire.txt';log=(build/f'{side}-client.log').open('w')
    previous=os.environ.get('HORIZON_DEV_ADVANCEMENT_SELECTOR_AUDIT')
    if side=='native':os.environ['HORIZON_DEV_ADVANCEMENT_SELECTOR_AUDIT']='1'
    try:server=net.Server(command,directory,side)
    finally:
        if previous is None:os.environ.pop('HORIZON_DEV_ADVANCEMENT_SELECTOR_AUDIT',None)
        else:os.environ['HORIZON_DEV_ADVANCEMENT_SELECTOR_AUDIT']=previous
    client=None;stop=None
    capture=build/f'{side}-packets.jsonl';snapshots={};actions=[]
    commands=Commands(server,build/f'{side}-commands.json')
    try:
        server.wait_for(r'Done \(')
        commands.reply('tick freeze')
        listen=net.free_port();stop=net_capture.start_background(listen,f'127.0.0.1:{port}',capture,full=True)
        client=net.Client(client_command,listen,wire,log)
        for option in ['chat on','flat-chat on','state on','players on','advancements on']:client.tell(option)
        client.tell(f'join {PLAYER}');client.sync([PLAYER])
        def cmd(text):return commands.reply(text)
        def guard(key,condition):
            cmd(f'data modify storage leash:proof guards.{key} set value {{result:91,success:17b}}')
            cmd(f'execute store result storage leash:proof guards.{key}.result int 1 store success storage leash:proof guards.{key}.success byte 1 run execute {condition}')
        for text in ['gamerule spawn_mobs false','gamerule advance_time false','gamerule announce_advancements false','gamerule show_advancement_messages false','gamerule log_admin_commands false',f'gamemode survival {PLAYER}',f'tp {PLAYER} 4.5 100 4.5 0 0',f'clear {PLAYER}',f'advancement revoke {PLAYER} only {ADV}', 'kill @e[type=!player]','summon frog 4.5 100 6.5 {UUID:[I;9,9,9,9],NoAI:1b,variant:"minecraft:temperate"}',f'item replace entity {PLAYER} hotbar.0 with lead 4']:
            cmd(text)
        def step(n):
            start=game_time(server);cmd(f'tick step {n}')
            until=time.monotonic()+30
            while game_time(server)<start+n:
                assert time.monotonic()<until,'step timeout';time.sleep(.02)
            assert game_time(server)==start+n
            client.sync([PLAYER])
        step(2)
        def save(phase):
            client.sync([PLAYER]);client.tell('step evidence_'+phase)
            client.tell(f'adv-snapshot {PLAYER} {phase} {ADV}')
            while (line:=client.process.stdout.readline()) != f'adv-snapshotted {phase} {PLAYER}\n':
                assert line,'client exited before actual progress receipt';log.write(line)
            dump(client,'progress_'+phase)
            prefix=PLAYER+' step=evidence_'+phase+' adv-snapshot '
            packets=[json.loads(line[len(prefix):]) for line in wire.read_text().splitlines() if line.startswith(prefix)]
            assert len(packets)==1,(phase,packets)
            view=packets[0];assert view['uuid']==offline(PLAYER) and view['name']==PLAYER and view['reset_count']>=1
            start=len(server.lines);server.console('save-all flush');wait_new(server,start,'Saved the game')
            uid=offline(PLAYER);candidates=[directory/'world'/part/(uid+'.json') for part in ['advancements','players/advancements']]
            files=[p for p in candidates if p.exists()];assert len(files)<=1,files
            file_value=json.loads(files[0].read_text()) if files else None
            if files:shutil.copy2(files[0],build/f'{side}-{phase}-advancement.json')
            if side=='java':
                assert file_value is not None,'Java actual saved phase file required'
                value=file_value;origin='Java actual saved advancement file'
            else:
                rows=[json.loads(line.split('advancement selector model raw: ',1)[1]) for line in server.lines[start:] if 'advancement selector model raw: ' in line]
                rows=[row for row in rows if row['uuid']==uid and row['name']==PLAYER]
                assert len(rows)==1,('actual native save-boundary model audit',phase,rows)
                assert rows[0]['conn']>0
                value=rows[0]['progress'];origin='existing actual placed Guest save-boundary audit'
            granted=CRITERION in value.get(ADV,{}).get('criteria',{})
            pv=view['progress'][ADV];available=pv.get('present') and pv.get('progress_received')
            if available:assert (CRITERION in pv['criteria'])==granted,('packet/model prerequisite disagreement',phase,pv,value)
            snapshots[phase]={'uuid':uid,'criterion':granted,'full':value,'origin':origin,'packet_view':view,'packet_view_available':bool(available),'file_diagnostic':file_value,'audit':rows if side=='native' else None,'receipt':server.lines[start:]}
            (build/f'{side}-progress.json').write_text(json.dumps(snapshots,indent=2)+'\n')
        def state(phase):
            client.tell('step '+phase)
            words=','.join(map(str,uuid_words(PLAYER)))
            guard(phase+'_player',f'if entity @a[name={PLAYER},type=player,nbt={{UUID:[I;{words}],Health:20f,playerGameType:0,Pos:[4.5d,100d,4.5d]}}]')
            guard(phase+'_frog',f'if entity @e[type=frog,nbt={{UUID:[I;9,9,9,9],Health:10f,NoAI:1b,variant:"minecraft:temperate",Pos:[4.5d,100d,6.5d]}}]')
            for key,text in [('loaded','if loaded 4 100 6'),('floor','if block 4 99 6 stone')]:guard(phase+'_'+key,text)
            cmd(f'data modify storage leash:proof {phase}.player set from entity {PLAYER}')
            cmd(f'data modify storage leash:proof {phase}.frog set from entity {FROG}')
            cmd(f'execute store result storage leash:proof {phase}.clock long 1 run time query gametime')
            save(phase)
        state('initial')
        def click(label):
            client.sync([PLAYER]);dump(client,label)
            adds=[]
            for p in net_capture.load(capture):
                if p.get('dir')=='s2c' and p.get('state')=='play' and p.get('name')=='add_entity':
                    b=bytes.fromhex(p['data']);eid,i=net_capture.read_varint(b,0)
                    if uuid.UUID(bytes=b[i:i+16])==uuid.UUID(FROG):adds.append(eid)
            assert adds and len(set(adds))==1,('actual AddEntity UUID mapping',adds)
            actions.append({'label':label,'uuid':FROG,'entity_id':adds[-1]});(build/f'{side}-actions.json').write_text(json.dumps(actions,indent=2)+'\n')
            client.tell('step '+label);client.tell(f'leash-click {PLAYER} {adds[-1]}');client.sync([PLAYER])
        click('attach');step(1);state('attached')
        # A missing positive attach criterion invalidates this scene before detach.
        assert snapshots['attached']['criterion'],'attach criterion prerequisite'
        def revoke(label):
            cmd(f'data modify storage leash:proof {label} set value {{result:91,success:17b}}')
            client.tell('step '+label)
            client.tell(f'cmd {PLAYER} execute store result storage leash:proof {label}.result int 1 store success storage leash:proof {label}.success byte 1 run advancement revoke @s only {ADV} {CRITERION}')
            client.sync([PLAYER])
        revoke('attach_revoke');state('revoked')
        assert not snapshots['revoked']['criterion'],'revoke prerequisite'
        click('detach');step(1);state('detached');revoke('detach_revoke')
        client.tell('step final');client.sync([PLAYER]);cmd('save-all flush');client.end()
    finally:
        if client:client.kill()
        if stop:stop.set()
        server.stop();log.close()
    return facts_from_saved(build,side)


def facts_from_saved(build,side):
    directory=build/side;capture=build/f'{side}-packets.jsonl'
    snapshots=json.loads((build/f'{side}-progress.json').read_text())
    actions=json.loads((build/f'{side}-actions.json').read_text())
    raw={str(p.relative_to(directory)):nbt.read_dat(p) for p in (directory/'world/data').rglob('*command_storage*.dat')}
    (build/f'{side}-storage.json').write_text(json.dumps(raw,indent=2)+'\n')
    raw=json.loads((build/f'{side}-storage.json').read_text())
    proof=next(v['data']['contents']['proof'] for v in raw.values() if 'proof' in v['data'].get('contents',{}))
    facts={};start=proof['initial']['clock'][1]
    for key,v in proof['guards'].items():facts['guard/'+key]=v
    for phase in PHASES:
        v=proof[phase];facts[phase+'/clock']=['l',v['clock'][1]-start]
        for key in ['UUID','Health','playerGameType','Pos','Inventory']:facts[phase+'/player/'+key]=v['player'].get(key)
        for key in ['UUID','Health','NoAI','variant','Pos','leash']:facts[phase+'/frog/'+key]=v['frog'].get(key)
        facts[phase+'/criterion']=snapshots[phase]['criterion'];facts[phase+'/recipient']=snapshots[phase]['uuid']
        facts[phase+'/actual_progress_identity']=snapshots[phase]['packet_view']['uuid']==offline(PLAYER) and snapshots[phase]['uuid']==offline(PLAYER)
        pv=snapshots[phase]['packet_view']['progress'][ADV]
        facts[phase+'/packet_model_crosscheck']=(not snapshots[phase]['packet_view_available']) or ((CRITERION in pv['criteria'])==snapshots[phase]['criterion'])
    for key in ['attach_revoke','detach_revoke']:facts[key]=proof[key]
    requests=[p for p in net_capture.load(capture) if p.get('dir')=='c2s' and p.get('state')=='play' and p.get('name')=='interact']
    decoded=[]
    for p in requests:
        b=bytes.fromhex(p['data']);eid,i=net_capture.read_varint(b,0);kind,i=net_capture.read_varint(b,i);hand,i=net_capture.read_varint(b,i)
        assert i+1==len(b) and b[i]==0
        decoded.append([eid,kind,hand,False])
    facts['input/actual_two_clicks']=decoded==[[a['entity_id'],0,0,False] for a in actions] and len(actions)==2
    (build/f'{side}-facts.json').write_text(json.dumps(facts,indent=2)+'\n');return facts


def admit(f):
    assert all(v=={'result':1,'success':['b',1]} for k,v in f.items() if k.startswith('guard/'))
    assert f['input/actual_two_clicks']
    for phase,ticks,n,granted in [('initial',0,4,False),('attached',1,3,True),('revoked',1,3,False),('detached',2,3,False)]:
        assert f[phase+'/clock']==['l',ticks]
        assert f[phase+'/recipient']==offline(PLAYER)
        assert f[phase+'/actual_progress_identity'] and f[phase+'/packet_model_crosscheck']
        assert f[phase+'/criterion']==granted,(phase,f[phase+'/criterion'])
        inv=f[phase+'/player/Inventory'];assert sum(s['count'] for s in inv if s['id']=='minecraft:lead')==n,(phase,inv)
        leash=f[phase+'/frog/leash']
        assert leash==({'UUID':['I',uuid_words(PLAYER)]} if phase=='attached' or phase=='revoked' else None),(phase,leash)
    assert f['attach_revoke']=={'result':1,'success':['b',1]}
    assert f['detach_revoke']=={'result':0,'success':['b',0]}


def main():
    p=argparse.ArgumentParser();p.add_argument('--side',choices=['java','native'],required=True);p.add_argument('--binary',type=Path,required=True);p.add_argument('--build',type=Path,required=True);a=p.parse_args();a.build.mkdir(parents=True,exist_ok=True)
    (a.build/'program.json').write_text(json.dumps(program(),indent=2)+'\n');net.PROPERTIES['force-gamemode']='false';net.PLAYERS=[PLAYER]
    java,client=clients(a.build)
    if a.side=='java':
        generate(java,a.build);f=run('java',java,client,a.build);admit(f);(a.build/'reference.json').write_text(json.dumps(f,indent=2)+'\n');print('JAVA ADMITTED',len(f));return 0
    f=run('native',[str(a.binary.resolve()),'--resources',str(net.lab.LOCAL/'horizon-client/26.3'),'--dedicated','--nogui'],client,a.build)
    ref=json.loads((a.build/'reference.json').read_text());diff={k:{'java':v,'native':f.get(k)} for k,v in ref.items() if f.get(k)!=v};report={'facts':len(ref),'matched':len(ref)-len(diff),'differences':diff,'reference_sha256':hashlib.sha256((a.build/'reference.json').read_bytes()).hexdigest()};(a.build/'strict-report.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2));return bool(diff)

if __name__=='__main__':raise SystemExit(main())
