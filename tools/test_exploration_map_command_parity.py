#!/usr/bin/env python3
"""One real command modifier: map allocation, actual item, and complete saved data.

Normal seed42 world, structures enabled. Full raw packets, bodies, files and
absolute clocks are retained. Named loot and renderer repairs are outside scope.
"""
import argparse, gzip, hashlib, io, json, os, shutil, subprocess, time
from pathlib import Path
import anvil_dump as nbt
import net_capture
import test_locator_bar_parity as net
from test_disconnect_crafting_parity import offline, uuid_words
from test_machine_recipe_placement_parity import Commands, dump
from test_dedicated_server_parity import game_time

PLAYER='MapAlice'
MODIFIER='{type:"minecraft:exploration_map",destination:"#minecraft:on_ocean_monument_maps",decoration:"minecraft:monument",zoom:2,skip_existing_chunks:false}'
PHASES=['initial','plain','exploration','empty']

def program():
    return dict(version=3,player=PLAYER,seed=42,preset='minecraft:normal',structures=True,
                origin=[0,64,0],modifier=MODIFIER,steps=2,measurement_steps=0,
                primary='ONE inline item modify; actual selected item/map_id, saved map/counter',
                strict='guards, typed stack/count/components, callbacks/stores, all saved map headers/colors',
                diagnostics='complete packets/bodies/files, absolute clocks/epochs; missing map is allocation failure, not measured color mismatch',
                limits='no named loot/all aliases/structure search policy/renderer/packet/restart claim')

def read_dat(path):
    raw=path.read_bytes();raw=gzip.decompress(raw) if raw[:2]==b'\x1f\x8b' else raw
    f=io.BytesIO(raw);kind=f.read(1)[0];n=int.from_bytes(f.read(2),'big');name=f.read(n)
    value=nbt.read_tag(f,kind);assert f.tell()==len(raw),(path,'trailing bytes')
    return dict(root_type=kind,root_name_hex=name.hex(),body=value)

def clients(build):
    lock,jars,libraries=net.base.verify_artifacts();cp=os.pathsep.join(map(str,[jars['server'],*libraries]))
    classes=build/'classes'
    if not classes.exists():
        classes.mkdir()
        subprocess.run(['javac','--release',str(lock['minecraft']['java_major']),'-proc:none','-nowarn','-sourcepath',str(net.lab.ROOT/'mods/horizon/src/test/java'),'-cp',cp,'-d',str(classes),str(net.lab.ROOT/'mods/horizon/src/test/java/dev/horizonlab/parity/LocatorBarClient.java')],check=True)
    return ['java','-Xmx2g','-Duser.timezone=UTC','-cp',cp,'net.minecraft.server.Main','nogui'],['java','-Xmx1g','-cp',str(classes)+os.pathsep+cp,'dev.horizonlab.parity.LocatorBarClient']

def generate(java,build):
    directory=build/'generate';net.server_dir(directory);server=net.Server(java,directory,'generate')
    try:
        server.wait_for(r'Done \(');c=Commands(server,build/'generate-commands.json')
        for text in ['tick freeze','gamerule spawn_mobs false','gamerule advance_time false','gamerule respawn_radius 0','forceload add 0 0 15 15','fill 0 199 0 15 199 15 stone','fill 0 200 0 15 203 15 air','setworldspawn 4 200 4','save-all flush']:c.reply(text)
    finally:server.stop()
    level=read_dat(directory/'world/level.dat');(build/'initial-level.json').write_text(json.dumps(level,indent=2)+'\n')
    generation=read_dat(directory/'world/data/minecraft/world_gen_settings.dat')
    (build/'initial-world-gen-settings.json').write_text(json.dumps(generation,indent=2)+'\n')
    settings=generation['body']['data']
    assert settings['seed']==('l',42),settings
    assert settings['generate_structures']==('b',1),settings
    assert settings['dimensions']['minecraft:overworld']['generator']['type']=='minecraft:noise',settings


def run(side,command,client_command,build):
    directory=build/side;port=net.server_dir(directory,build/'generate/world')
    server=net.Server(command,directory,side);client=None;stop=None
    log=(build/f'{side}-client.log').open('w');wire=build/f'{side}-wire.txt';capture=build/f'{side}-packets.jsonl'
    c=Commands(server,build/f'{side}-commands.json')
    try:
        server.wait_for(r'Done \(');c.reply('tick freeze')
        listen=net.free_port();stop=net_capture.start_background(listen,f'127.0.0.1:{port}',capture,full=True)
        client=net.Client(client_command,listen,wire,log)
        for option in ['chat on','flat-chat on','state on','players on']:client.tell(option)
        client.tell(f'join {PLAYER}');client.sync([PLAYER])
        for text in ['gamerule show_advancement_messages false',f'gamemode creative {PLAYER}',f'tp {PLAYER} 4.5 200.0 4.5 0 0',f'clear {PLAYER}',f'item replace entity {PLAYER} hotbar.0 with map[custom_data={{map_command_sentinel:2109}}]',f'item replace entity {PLAYER} hotbar.1 with map 2']:c.reply(text)
        start=game_time(server);c.reply('tick step 2');deadline=time.monotonic()+30
        while game_time(server)<start+2:
            assert time.monotonic()<deadline;time.sleep(.02)
        assert game_time(server)==start+2;client.sync([PLAYER])
        def guard(key,condition):
            c.reply(f'data modify storage mapcmd:proof guards.{key} set value {{result:91,success:17b}}')
            c.reply(f'execute store result storage mapcmd:proof guards.{key}.result int 1 store success storage mapcmd:proof guards.{key}.success byte 1 run execute {condition}')
        guard('identity',f'if entity @a[name={PLAYER},nbt={{UUID:[I;{",".join(map(str,uuid_words(PLAYER)))}],Health:20.0f,playerGameType:1,Pos:[4.5d,200.0d,4.5d],Rotation:[0.0f,0.0f],SelectedItemSlot:0}}]')
        guard('floor','if block 4 199 4 minecraft:stone');guard('load','if loaded 4 200 4')
        guard('selected',f'if data entity {PLAYER} SelectedItem{{id:"minecraft:map",count:1,components:{{"minecraft:custom_data":{{map_command_sentinel:2109}}}}}}')
        def snapshot(phase):
            client.sync([PLAYER]);dump(client,phase)
            c.reply(f'data modify storage mapcmd:proof {phase}.player set from entity {PLAYER}')
            c.reply(f'execute store result storage mapcmd:proof {phase}.clock long 1 run time query gametime')
            c.reply('save-all flush')
            raw={str(p.relative_to(directory/'world')):read_dat(p) for p in (directory/'world/data').rglob('*.dat') if '/maps/' in str(p) or 'command_storage' in p.name}
            (build/f'{side}-{phase}-saved.json').write_text(json.dumps(raw,indent=2)+'\n')
            dest=build/f'{side}-{phase}-raw';dest.mkdir()
            for p in (directory/'world/data').rglob('*.dat'):
                if '/maps/' in str(p):
                    target=dest/p.relative_to(directory/'world/data');target.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(p,target)
        def action(phase,command):
            c.reply(f'data modify storage mapcmd:proof {phase}_result set value {{result:91,success:17b}}')
            c.reply(f'execute store result storage mapcmd:proof {phase}_result.result int 1 store success storage mapcmd:proof {phase}_result.success byte 1 run {command}')
            snapshot(phase)
        snapshot('initial')
        action('plain',f'item modify entity {PLAYER} hotbar.1 {{type:"minecraft:set_count",count:2}}')
        action('exploration',f'execute positioned 0.0 64.0 0.0 run item modify entity {PLAYER} hotbar.0 {MODIFIER}')
        action('empty',f'item modify entity {PLAYER} hotbar.2 {{type:"minecraft:set_count",count:2}}')
    finally:
        if client:
            try:client.end()
            finally:client.kill()
        try:server.stop()
        finally:
            if stop:stop.set()
            log.close()
    return project(side,build)

def project(side,build):
    saved={p:json.loads((build/f'{side}-{p}-saved.json').read_text()) for p in PHASES}
    proof=next(v['body']['data']['contents']['proof'] for v in saved['empty'].values() if 'proof' in v['body'].get('data',{}).get('contents',{}))
    facts={'guard/'+k:v for k,v in proof['guards'].items()};start=proof['initial']['clock'][1]
    for phase in PHASES:
        body=proof[phase]['player'];facts[phase+'/relative_clock']=['l',proof[phase]['clock'][1]-start]
        for k in ['UUID','Health','playerGameType','Pos','Rotation','SelectedItemSlot','SelectedItem','Inventory']:facts[phase+'/'+k]=body.get(k)
        if phase!='initial':facts[phase+'/callback']=proof[phase+'_result']
        for name in ['0.dat','last_id.dat']:
            facts[phase+'/mapfile/'+name]=saved[phase].get('data/minecraft/maps/'+name)
    (build/f'{side}-facts.json').write_text(json.dumps(facts,indent=2)+'\n');return facts

def admit(f):
    assert all(v=={'result':1,'success':['b',1]} for k,v in f.items() if k.startswith('guard/'))
    for p in PHASES:
        assert f[p+'/relative_clock']==['l',0]
        assert f[p+'/UUID']==['I',uuid_words(PLAYER)]
    for p in ['plain','exploration']:assert f[p+'/callback']=={'result':1,'success':['b',1]}
    assert f['empty/callback']=={'result':0,'success':['b',0]}
    assert f['initial/SelectedItem']['id']=='minecraft:map'
    assert f['plain/SelectedItem']==f['initial/SelectedItem']
    s=f['exploration/SelectedItem'];assert s['id']=='minecraft:map' and s['count']==1
    assert s['components']['minecraft:map_id']==0 and s['components']['minecraft:custom_data']=={'map_command_sentinel':2109}
    assert f['empty/SelectedItem']==s
    for p in ['initial','plain']:
        assert f[p+'/mapfile/0.dat'] is None and f[p+'/mapfile/last_id.dat'] is None
    for p in ['exploration','empty']:
        m=f[p+'/mapfile/0.dat'];assert m and m['root_type']==10
        d=m['body']['data'];assert d['dimension']=='minecraft:overworld' and d['scale']==['b',2]
        assert d['xCenter']==704 and d['zCenter']==-320
        assert d['colors'][0]=='B' and len(d['colors'][1])==16384 and any(d['colors'][1])
        assert f[p+'/mapfile/last_id.dat']['body']['data']['map']==0

def side(name,binary,build):
    net.PLAYERS=[PLAYER];net.PROPERTIES.update({'level-seed':'42','level-type':'minecraft\\:normal','generate-structures':'true','generator-settings':'{}','force-gamemode':'false'})
    (build/'program.json').write_text(json.dumps(program(),indent=2)+'\n')
    java,client=clients(build)
    if name=='java':
        generate(java,build);f=run('java',java,client,build);admit(f);(build/'reference.json').write_text(json.dumps(f,indent=2)+'\n');print('JAVA ADMITTED',len(f));return 0
    f=run('native',[str(Path(binary).resolve()),'--resources',str(net.lab.LOCAL/'horizon-client/26.3'),'--dedicated','--nogui'],client,build)
    ref=json.loads((build/'reference.json').read_text());diff={k:{'java':v,'native':f.get(k)} for k,v in ref.items() if f.get(k)!=v};report={'facts':len(ref),'matched':len(ref)-len(diff),'differences':diff,'reference_sha256':hashlib.sha256((build/'reference.json').read_bytes()).hexdigest()};(build/'strict-report.json').write_text(json.dumps(report,indent=2)+'\n');print('NATIVE',report['matched'],'/',report['facts'],list(diff));return int(bool(diff))

def main():
    p=argparse.ArgumentParser(description='With no arguments (as tools/parity.py runs it), builds the game and runs both sides into a fresh .local/exploration_map_command.')
    p.add_argument('--side',choices=['java','native']);p.add_argument('--binary',type=Path);p.add_argument('--build',type=Path);a=p.parse_args()
    if a.side:
        if a.build is None or (a.side=='native' and a.binary is None):p.error('--side needs --build, and --binary for native')
        a.build.mkdir(parents=True,exist_ok=True);return side(a.side,a.binary,a.build)
    build=net.lab.LOCAL/'exploration_map_command';shutil.rmtree(build,ignore_errors=True)
    # Builds the classes into build/classes and the game binary; each phase writes fresh folders under build.
    _,build,_,horizon,_=net.sides('exploration_map_command',a.binary)
    return side('java',None,build) or side('native',horizon[0],build)

if __name__=='__main__':raise SystemExit(main())
